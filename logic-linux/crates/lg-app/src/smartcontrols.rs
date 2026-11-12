//! Smart Controls (Y), as Logic's: the selected track's screen controls in a layout chosen for
//! its patch (Keyboard, Synthesizer, Drums, Orchestral, Guitar & Bass, Audio Effects…), each
//! control mapped to one or more channel strip or plug-in parameters with a range, and the
//! Channel EQ's curve at the right (a click opens it). The header has the Smart Controls
//! Inspector button, Track / Master (the Stereo Out's effects), the routing destination shown
//! (the track, or an aux it sends to), Compare (the edits against the settings as they were) and
//! the Arpeggiator (its button and menu: note order, rate, octave range, latch, note length,
//! velocity). The inspector chooses the layout, shows and edits the selected control's mappings
//! (parameter, range, Add Mapping, Learn) and its external assignment (Learn, then move a MIDI
//! controller). Window ▸ Open Smart Controls shows it as a window.

use std::collections::HashMap;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::arp;
use crate::look;
use crate::widgets;

/// The key commands and menu items this file runs.
pub const COMMANDS: &[&str] = &[
    "Toggle Track Master View", "Cycle Through Routing Destination Views", "Cycle Through Subviews", "Add Mapping", "Learn Parameter Mapping", "Learn External Assignment",
    "Show Smart Controls", "Open Smart Controls", "Open Smart Controls…", "Toggle Channel Strip Arpeggiator",
    "Map all Controls", "Map all Unmapped Controls", "Copy Mapping", "Paste Mapping", "Copy All Mappings", "Paste Mappings", "Delete Mapping", "Automatic Smart Controls",
];

/// What a screen control moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Volume,
    Pan,
    /// A send's level (index in the track's sends).
    Send(usize),
    /// A Channel EQ band's gain.
    EqGain(usize),
    /// The instrument's parameter (id).
    Instrument(u32),
    /// An Audio FX's parameter: insert index, parameter id (on the view's strip).
    Effect(usize, u32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mapping {
    pub target: Target,
    /// The range used, as shares of the parameter's range (lo > hi: inverted).
    pub lo: f32,
    pub hi: f32,
    /// The parameter mapping graph (Scaling): the control's value through it first.
    pub graph: crate::scgraph::Graph,
}

impl Mapping {
    pub fn new(target: Target) -> Mapping {
        Mapping { target, lo: 0.0, hi: 1.0, graph: Default::default() }
    }
    /// The parameter's share for the control at `v` (0..1).
    pub fn param_share(&self, v: f32) -> f32 {
        self.lo + (self.hi - self.lo) * self.graph.apply(v.clamp(0.0, 1.0))
    }
    /// The control's value (0..1) for the parameter's share.
    pub fn control_value(&self, share: f32) -> f32 {
        if (self.hi - self.lo).abs() < 1e-6 { 0.0 } else { self.graph.inverse(((share - self.lo) / (self.hi - self.lo)).clamp(0.0, 1.0)) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Knob,
    Button,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub label: String,
    pub kind: Kind,
    pub maps: Vec<Mapping>,
    /// External assignment: a MIDI controller number.
    pub cc: Option<u8>,
}

/// Layouts: name, knobs, buttons.
pub const LAYOUTS: &[(&str, usize, usize)] = &[
    ("Basic", 8, 0),
    ("Keyboard", 8, 4),
    ("Synthesizer", 12, 0),
    ("Drums", 8, 2),
    ("Orchestral", 8, 2),
    ("Guitar & Bass", 8, 2),
    ("Audio Effects", 8, 0),
    ("Master Effects", 8, 0),
    ("Faders", 16, 0),
];

/// Which routing destination the Smart Controls show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    Track(usize),
    Aux(usize),
    Master,
}

impl View {
    /// The mixer strip id (mixstate).
    pub fn strip(self) -> usize {
        match self {
            View::Track(t) => t,
            View::Aux(k) => crate::mixer::AUX + k,
            View::Master => crate::mixstate::MASTER,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Screen {
    pub layout: usize,
    pub controls: Vec<Control>,
    /// The page shown (Cycle Through Subviews), when the plug-in parameters run over a page.
    pub subview: usize,
    /// Compare: the values as saved (before the first edit), the edits while comparing.
    pub saved: Option<Vec<(Target, f32)>>,
    pub edited: Option<Vec<(Target, f32)>>,
    pub comparing: bool,
}

#[derive(Default)]
pub struct State {
    pub screens: HashMap<View, Screen>,
    /// Master view (the Stereo Out) instead of the track.
    pub master: bool,
    /// An aux the track sends to, shown instead of the track (its index in the aux list).
    pub dest: Option<usize>,
    pub inspector: bool,
    pub selected: usize,
    /// Learning: the next parameter moved maps to this control (the values when Learn began).
    pub learn: Option<(View, usize, Vec<(Target, f32)>)>,
    /// Learning an external assignment: the next MIDI controller moved is assigned.
    pub learn_cc: Option<(View, usize)>,
    pub window: bool,
    /// The tracks' Arpeggiators.
    pub arps: HashMap<usize, arp::Settings>,
    /// A drag on a channel control has its undo step.
    drag_step: bool,
    /// The pane's rect (its key commands under the pointer).
    pub rect: Option<Rect>,
    /// The parameter mapping graph window (scgraph.rs).
    pub graph: crate::scgraph::State,
    /// Copy Mapping / Copy All Mappings.
    pub map_clip: Option<Vec<Mapping>>,
    /// A screen control's shortcut menu (Control-click): the view, the control, where.
    ctx_menu: Option<(View, usize, egui::Pos2)>,
}

/// The view the pane shows for the selected track.
pub fn view(app: &App) -> Option<View> {
    if app.sc.master {
        return Some(View::Master);
    }
    let t = app.selected_track.filter(|t| app.project.as_ref().is_some_and(|p| *t < p.tracks.len()))?;
    if let Some(k) = app.sc.dest.filter(|k| *k < app.auxes.len()) {
        return Some(View::Aux(k));
    }
    Some(View::Track(t))
}

/// A parameter: name, range, default, unit, value now.
#[derive(Clone, Debug)]
pub struct ParamInfo {
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: lg_dsp::Unit,
    pub value: f32,
}

fn effect_list(app: &App, v: View) -> Vec<(usize, String, Vec<lg_dsp::Param>, Vec<f32>)> {
    let s = v.strip();
    let plugins = crate::mixstate::inserts(app, s);
    let e = app.engine.lock().ok();
    plugins.iter().enumerate().filter_map(|(k, u)| {
        let (params, values) = match e.as_ref().and_then(|e| e.effect_params(crate::mixer::fx_strip(s), k)) {
            Some(x) => x,
            None => {
                let mut fx = crate::units::effect(&u.name, crate::audio::RATE as f32)?;
                if !u.params.is_empty() {
                    fx.load_floats(&u.params);
                }
                let params = fx.params().to_vec();
                let values = params.iter().map(|p| fx.get(p.id)).collect();
                (params, values)
            }
        };
        Some((k, u.name.clone(), params, values))
    }).collect()
}

/// Every parameter a control on view `v` can map: (target, path, info).
pub fn parameters(app: &App, v: View) -> Vec<(Target, String, ParamInfo)> {
    let mut out = Vec::new();
    let info = |name: &str, min: f32, max: f32, default: f32, unit, value| ParamInfo { name: name.into(), min, max, default, unit, value };
    if let View::Track(t) = v {
        let (vol, pan) = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| (tr.volume_db, tr.pan)).unwrap_or((0.0, 0.0));
        out.push((Target::Volume, "Channel Strip ▸ Volume".into(), info("Volume", -60.0, 6.0, 0.0, lg_dsp::Unit::Db, vol)));
        out.push((Target::Pan, "Channel Strip ▸ Pan".into(), info("Pan", -1.0, 1.0, 0.0, lg_dsp::Unit::Pan, pan)));
        for (k, s) in app.sends.get(&t).into_iter().flatten().enumerate() {
            out.push((Target::Send(k), format!("Channel Strip ▸ Send {} (Bus {})", k + 1, s.bus + 1), info(&format!("Send {}", k + 1), -60.0, 6.0, -10.0, lg_dsp::Unit::Db, s.level_db)));
        }
        let eq = app.eqs.get(&t).copied().unwrap_or_default();
        for (b, name) in [(1usize, "Low Shelf Gain"), (2, "Low Mid Gain"), (3, "Mid Gain"), (4, "High Mid Gain"), (5, "Mid 2 Gain"), (6, "High Shelf Gain")] {
            out.push((Target::EqGain(b), format!("Channel EQ ▸ {name}"), info(name, -24.0, 24.0, 0.0, lg_dsp::Unit::Db, eq.bands[b].gain)));
        }
        let inst = app.instrument_for(t).map(|n| crate::soundlib::plugin_of(&n).to_string()).unwrap_or_default();
        if let Some((params, values)) = app.engine.lock().ok().and_then(|e| e.instrument_params(t)) {
            for (p, val) in params.into_iter().zip(values) {
                out.push((Target::Instrument(p.id), format!("{inst} ▸ {}", p.name), info(p.name, p.min, p.max, p.default, p.unit, val)));
            }
        }
    }
    for (k, name, params, values) in effect_list(app, v) {
        for (p, val) in params.into_iter().zip(values) {
            out.push((Target::Effect(k, p.id), format!("Audio FX {} {name} ▸ {}", k + 1, p.name), info(p.name, p.min, p.max, p.default, p.unit, val)));
        }
    }
    out
}

fn param_of(all: &[(Target, String, ParamInfo)], t: Target) -> Option<&ParamInfo> {
    all.iter().find(|x| x.0 == t).map(|x| &x.2)
}

/// Sets a parameter (in its own units).
pub fn set_target(app: &mut App, v: View, target: Target, value: f32) {
    match (v, target) {
        (View::Track(t), Target::Volume) => {
            if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                tr.volume_db = value;
            }
            app.sync_mix();
            app.dirty = true;
        }
        (View::Track(t), Target::Pan) => {
            if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                tr.pan = value;
            }
            app.sync_mix();
            app.dirty = true;
        }
        (View::Track(t), Target::Send(k)) => {
            if let Some(s) = app.sends.get_mut(&t).and_then(|v| v.get_mut(k)) {
                s.level_db = value;
            }
            app.dirty = true;
            app.load_engine();
        }
        (View::Track(t), Target::EqGain(b)) => {
            let mut eq = app.eqs.get(&t).copied().unwrap_or_default();
            eq.bands[b].gain = value;
            eq.bands[b].on = true;
            app.eqs.insert(t, eq);
            if let Ok(mut e) = app.engine.lock() {
                if let Some(tr) = e.tracks.get_mut(t) {
                    tr.eq = Some(eq);
                }
            }
            app.dirty = true;
        }
        (View::Track(t), Target::Instrument(id)) => {
            if let Ok(mut e) = app.engine.lock() {
                e.set_instrument_param(t, id, value);
            }
            app.dirty = true;
        }
        (_, Target::Effect(k, id)) => {
            let s = v.strip();
            if let Ok(mut e) = app.engine.lock() {
                e.set_effect_param(crate::mixer::fx_strip(s), k, id, value);
            }
            crate::mixstate::edit_inserts(app, s, |list| {
                if let Some(u) = list.get_mut(k) {
                    let i = id as usize;
                    if u.params.len() <= i {
                        u.params.resize(i + 1, 0.0);
                    }
                    u.params[i] = value;
                }
            });
            app.dirty = true;
        }
        _ => {}
    }
}

/// The parameter keywords a layout looks for first, in its order.
fn wanted(layout: &str) -> &'static [&'static str] {
    match layout {
        "Keyboard" => &["drive", "tone", "bell", "tremolo", "chorus", "phaser", "vibrato", "reverb", "attack", "release", "drawbar", "click", "percussion"],
        "Synthesizer" => &["cutoff", "resonance", "drive", "env", "attack", "decay", "sustain", "release", "glide", "lfo", "detune", "chorus", "reverb", "delay"],
        "Drums" => &["kick", "snare", "hat", "tom", "cymbal", "overhead", "room", "tune", "decay", "comp"],
        "Orchestral" => &["attack", "release", "vibrato", "expression", "dynamics", "brightness", "room", "reverb"],
        "Guitar & Bass" => &["tone", "drive", "gain", "attack", "release", "pickup", "mute", "comp", "reverb"],
        _ => &["cutoff", "drive", "tone", "attack", "release", "reverb", "mix", "level"],
    }
}

/// The layout for a patch (by its plug-in and Logic category).
pub fn layout_for(app: &App, v: View) -> usize {
    let idx = |n: &str| LAYOUTS.iter().position(|l| l.0 == n).unwrap_or(0);
    let t = match v {
        View::Track(t) => t,
        View::Master => return idx("Master Effects"),
        View::Aux(_) => return idx("Audio Effects"),
    };
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return 0 };
    if tr.kind == lg_core::TrackKind::Audio {
        return idx("Audio Effects");
    }
    if tr.kind == lg_core::TrackKind::Drummer {
        return idx("Drums");
    }
    let name = app.instrument_for(t).unwrap_or_default().to_lowercase();
    let cat = crate::browser::patches().iter().find(|p| p.2.eq_ignore_ascii_case(&name)).map(|p| p.0.clone()).unwrap_or_default();
    let has = |w: &[&str]| w.iter().any(|x| name.contains(x));
    if cat == "Drums & Percussion" || has(&["drum", "kit", "808", "beat"]) {
        idx("Drums")
    } else if cat == "Keyboard" || has(&["piano", "organ", "clav", "suitcase", "wurli", "electric piano", "b3", "keys", "mellotron"]) {
        idx("Keyboard")
    } else if cat == "Strings" || cat == "Brass & Woodwind" || has(&["string", "orchestra", "horn", "ensemble", "violin", "cello", "brass", "flute"]) {
        idx("Orchestral")
    } else if cat == "Bass" || cat == "Guitar" || has(&["bass", "guitar"]) {
        idx("Guitar & Bass")
    } else if cat == "Synthesizer" || has(&["synth", "alchem", "retro", "pad", "lead", "sculpt", "es2", "trio", "poly"]) {
        idx("Synthesizer")
    } else {
        0
    }
}

/// A view's default screen controls for a layout: the patch's main parameters (by the
/// layout's keywords, then in order), switches on the buttons; the channel's own controls when
/// the sound has no plug-in parameters (a SoundFont sound).
pub fn default_controls(app: &App, v: View, layout: usize) -> Vec<Control> {
    let (lname, knobs, buttons) = LAYOUTS.get(layout).copied().unwrap_or(LAYOUTS[0]);
    let all = parameters(app, v);
    let plug: Vec<&(Target, String, ParamInfo)> = all.iter().filter(|x| matches!(x.0, Target::Instrument(_) | Target::Effect(..))).collect();
    let mut used: Vec<Target> = Vec::new();
    let mut knob_list: Vec<Control> = Vec::new();
    let mut button_list: Vec<Control> = Vec::new();
    let is_switch = |i: &ParamInfo| i.unit == lg_dsp::Unit::OnOff;
    let is_knob = |i: &ParamInfo| !matches!(i.unit, lg_dsp::Unit::OnOff | lg_dsp::Unit::Menu);
    let push = |list: &mut Vec<Control>, used: &mut Vec<Target>, x: &(Target, String, ParamInfo), kind: Kind| {
        used.push(x.0);
        list.push(Control { label: x.2.name.clone(), kind, maps: vec![Mapping::new(x.0)], cc: None });
    };
    if plug.is_empty() && matches!(v, View::Track(_)) {
        for t in [Target::Volume, Target::Pan, Target::EqGain(1), Target::EqGain(3), Target::EqGain(6), Target::Send(0), Target::Send(1)] {
            if let Some(x) = all.iter().find(|x| x.0 == t) {
                if knob_list.len() < knobs {
                    push(&mut knob_list, &mut used, x, Kind::Knob);
                }
            }
        }
        for c in knob_list.iter_mut() {
            c.label = c.label.replace(" Shelf Gain", "").replace(" Gain", "");
        }
    } else {
        for w in wanted(lname) {
            if knob_list.len() >= knobs {
                break;
            }
            if let Some(x) = plug.iter().find(|x| is_knob(&x.2) && !used.contains(&x.0) && x.2.name.to_lowercase().contains(w)) {
                push(&mut knob_list, &mut used, x, Kind::Knob);
            }
        }
        for x in plug.iter() {
            if knob_list.len() >= knobs {
                break;
            }
            if is_knob(&x.2) && !used.contains(&x.0) {
                push(&mut knob_list, &mut used, x, Kind::Knob);
            }
        }
        for x in plug.iter() {
            if button_list.len() >= buttons {
                break;
            }
            if is_switch(&x.2) && !used.contains(&x.0) {
                push(&mut button_list, &mut used, x, Kind::Button);
            }
        }
    }
    while knob_list.len() < knobs {
        knob_list.push(Control { label: String::new(), kind: Kind::Knob, maps: Vec::new(), cc: None });
    }
    while button_list.len() < buttons {
        button_list.push(Control { label: String::new(), kind: Kind::Button, maps: Vec::new(), cc: None });
    }
    knob_list.extend(button_list);
    knob_list
}

/// The view's screen (made with the patch's layout the first time it's shown).
pub fn screen(app: &mut App, v: View) -> Screen {
    if let Some(s) = app.sc.screens.get(&v) {
        return s.clone();
    }
    let layout = layout_for(app, v);
    let s = Screen { layout, controls: default_controls(app, v, layout), ..Default::default() };
    app.sc.screens.insert(v, s.clone());
    s
}

/// A control's value (0..1) from its first mapping's parameter.
pub fn control_value(c: &Control, all: &[(Target, String, ParamInfo)]) -> Option<f32> {
    let m = c.maps.first()?;
    let i = param_of(all, m.target)?;
    let share = if i.max > i.min { (i.value - i.min) / (i.max - i.min) } else { 0.0 };
    Some(m.control_value(share))
}

/// Moves a control: every mapping's parameter follows within its range.
pub fn move_control(app: &mut App, v: View, ci: usize, value: f32) {
    let s = screen(app, v);
    let Some(c) = s.controls.get(ci) else { return };
    let all = parameters(app, v);
    // Compare: the first edit keeps the values as they were
    if !s.comparing && s.saved.is_none() {
        let snap = snapshot(&s, &all);
        if let Some(sc) = app.sc.screens.get_mut(&v) {
            sc.saved = Some(snap);
        }
    }
    for m in c.maps.clone() {
        let Some(i) = param_of(&all, m.target).cloned() else { continue };
        let mut x = i.min + m.param_share(value) * (i.max - i.min);
        if i.unit == lg_dsp::Unit::OnOff {
            x = if x >= (i.min + i.max) / 2.0 { i.max } else { i.min };
        }
        set_target(app, v, m.target, x);
    }
    crate::autoext::screen_control_moved(app, v, ci, value);
}

fn snapshot(s: &Screen, all: &[(Target, String, ParamInfo)]) -> Vec<(Target, f32)> {
    let mut v: Vec<(Target, f32)> = Vec::new();
    for c in &s.controls {
        for m in &c.maps {
            if let Some(i) = param_of(all, m.target) {
                if !v.iter().any(|x| x.0 == m.target) {
                    v.push((m.target, i.value));
                }
            }
        }
    }
    v
}

/// Compare: shows the saved values (the edits kept), or goes back to the edits.
pub fn compare(app: &mut App, v: View) {
    let s = screen(app, v);
    let all = parameters(app, v);
    let (apply, next) = if s.comparing {
        (s.edited.clone(), Screen { comparing: false, edited: None, ..s.clone() })
    } else {
        let Some(saved) = s.saved.clone() else {
            app.tell("Nothing to compare: no edits since the settings were loaded");
            return;
        };
        (Some(saved), Screen { comparing: true, edited: Some(snapshot(&s, &all)), ..s.clone() })
    };
    app.sc.screens.insert(v, next);
    for (t, val) in apply.unwrap_or_default() {
        set_target(app, v, t, val);
    }
}

/// The routing destinations of the selected track: the track, then each aux it sends to.
fn destinations(app: &App) -> Vec<Option<usize>> {
    let mut v = vec![None];
    if let Some(t) = app.selected_track {
        for s in app.sends.get(&t).into_iter().flatten() {
            if let Some(k) = app.auxes.iter().position(|a| a.bus == s.bus) {
                if !v.contains(&Some(k)) {
                    v.push(Some(k));
                }
            }
        }
    }
    v
}

/// Smart Controls' key commands.
pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Show Smart Controls" => {
            app.bottom = if app.bottom == crate::app::Bottom::SmartControls { crate::app::Bottom::None } else { crate::app::Bottom::SmartControls };
        }
        "Open Smart Controls" | "Open Smart Controls…" => app.sc.window = true,
        "Toggle Track Master View" => {
            app.sc.master = !app.sc.master;
            app.sc.selected = 0;
        }
        "Cycle Through Routing Destination Views" => {
            app.sc.master = false;
            let d = destinations(app);
            let i = d.iter().position(|x| *x == app.sc.dest).unwrap_or(0);
            app.sc.dest = d[(i + 1) % d.len()];
            app.sc.selected = 0;
            if d.len() == 1 {
                app.tell("The track sends to no aux");
            }
        }
        "Cycle Through Subviews" => {
            let Some(v) = view(app) else { return true };
            screen(app, v);
            let n = pages(app, v);
            if let Some(s) = app.sc.screens.get_mut(&v) {
                s.subview = (s.subview + 1) % n.max(1);
            }
        }
        "Add Mapping" => {
            let Some(v) = view(app) else { return true };
            screen(app, v);
            let sel = app.sc.selected;
            app.sc.inspector = true;
            learn(app, v, sel);
            app.tell("Add Mapping: move the parameter to add, or choose it in the Smart Controls inspector");
        }
        "Learn Parameter Mapping" => {
            let Some(v) = view(app) else { return true };
            let sel = app.sc.selected;
            if app.sc.learn.is_some() {
                app.sc.learn = None;
            } else {
                learn(app, v, sel);
            }
        }
        "Learn External Assignment" => {
            let Some(v) = view(app) else { return true };
            app.sc.learn_cc = if app.sc.learn_cc.is_some() { None } else { Some((v, app.sc.selected)) };
            if app.sc.learn_cc.is_some() {
                app.tell("Move a control on your MIDI controller to assign it");
            }
        }
        "Map all Controls" | "Map all Unmapped Controls" | "Automatic Smart Controls" => {
            let Some(v) = view(app) else { return true };
            let mut s = screen(app, v);
            if name == "Automatic Smart Controls" {
                s.layout = layout_for(app, v);
            }
            let fresh = default_controls(app, v, s.layout);
            if name == "Automatic Smart Controls" {
                s.controls = fresh;
            } else {
                for (c, f) in s.controls.iter_mut().zip(fresh) {
                    if name == "Map all Controls" || c.maps.is_empty() {
                        c.maps = f.maps;
                        if c.label.is_empty() || name == "Map all Controls" {
                            c.label = f.label;
                        }
                    }
                }
            }
            app.sc.screens.insert(v, s);
            app.dirty = true;
        }
        "Copy Mapping" | "Copy All Mappings" | "Paste Mapping" | "Paste Mappings" | "Delete Mapping" => {
            let Some(v) = view(app) else { return true };
            let mut s = screen(app, v);
            let sel = app.sc.selected;
            let Some(c) = s.controls.get_mut(sel) else { return true };
            match name {
                "Copy Mapping" => app.sc.map_clip = c.maps.first().map(|m| vec![*m]),
                "Copy All Mappings" => app.sc.map_clip = Some(c.maps.clone()),
                "Paste Mapping" => {
                    if let Some(m) = app.sc.map_clip.as_ref().and_then(|x| x.first()).copied() {
                        if c.maps.is_empty() {
                            c.maps.push(m);
                        } else {
                            c.maps[0] = m;
                        }
                    }
                }
                "Paste Mappings" => {
                    if let Some(ms) = app.sc.map_clip.clone() {
                        c.maps = ms;
                    }
                }
                _ => {
                    if !c.maps.is_empty() {
                        c.maps.remove(0);
                    }
                }
            }
            app.sc.screens.insert(v, s);
            app.dirty = true;
        }
        "Toggle Channel Strip Arpeggiator" => {
            let Some(t) = app.selected_track else { return true };
            let a = app.sc.arps.entry(t).or_default();
            a.on = !a.on;
            let on = a.on;
            app.dirty = true;
            app.load_engine();
            app.tell(if on { "Arpeggiator on" } else { "Arpeggiator off" });
        }
        _ => return false,
    }
    true
}

fn learn(app: &mut App, v: View, ci: usize) {
    let snap: Vec<(Target, f32)> = parameters(app, v).into_iter().map(|x| (x.0, x.2.value)).collect();
    app.sc.learn = Some((v, ci, snap));
}

/// Each frame: Learn watches for the first parameter that moves (in a plug-in window, the
/// channel strip or the Mixer) and maps it to the control.
pub fn update(app: &mut App) {
    let Some((v, ci, snap)) = app.sc.learn.clone() else { return };
    let now = parameters(app, v);
    if now.len() != snap.len() {
        learn(app, v, ci);
        return;
    }
    let moved = now.iter().find(|x| snap.iter().find(|s| s.0 == x.0).is_some_and(|s| (s.1 - x.2.value).abs() > 1e-5 * (x.2.max - x.2.min).abs().max(1e-3)));
    if let Some((t, path, info)) = moved.cloned() {
        screen(app, v);
        if let Some(c) = app.sc.screens.get_mut(&v).and_then(|s| s.controls.get_mut(ci)) {
            if !c.maps.iter().any(|m| m.target == t) {
                c.maps.push(Mapping::new(t));
            }
            if c.label.is_empty() {
                c.label = info.name.clone();
            }
        }
        app.sc.learn = None;
        app.tell(format!("Mapped to {path}"));
    }
}

/// A MIDI controller moved: assigned during Learn External Assignment; afterwards it drives
/// the screen controls it's assigned to (and isn't passed on to the instrument). True when used.
pub fn midi_cc(app: &mut App, cc: u8, value: u8) -> bool {
    if let Some((v, _)) = app.sc.learn_cc {
        // (learning goes on: select another control and move another hardware control to
        // assign that too; Learn again ends it)
        let ci = app.sc.selected;
        app.sc.learn_cc = Some((v, ci));
        screen(app, v);
        if let Some(s) = app.sc.screens.get_mut(&v) {
            for c in s.controls.iter_mut() {
                if c.cc == Some(cc) {
                    c.cc = None;
                }
            }
            if let Some(c) = s.controls.get_mut(ci) {
                c.cc = Some(cc);
            }
        }
        app.tell(format!("Assigned controller {cc}"));
        return true;
    }
    let Some(v) = view(app) else { return false };
    let Some(s) = app.sc.screens.get(&v) else { return false };
    let hits: Vec<usize> = s.controls.iter().enumerate().filter(|(_, c)| c.cc == Some(cc)).map(|(i, _)| i).collect();
    for &i in &hits {
        move_control(app, v, i, value as f32 / 127.0);
    }
    !hits.is_empty()
}

fn page_params(app: &App, v: View) -> Vec<(Target, String, ParamInfo)> {
    parameters(app, v).into_iter().filter(|x| matches!(x.0, Target::Instrument(_) | Target::Effect(..)) && !matches!(x.2.unit, lg_dsp::Unit::Menu)).collect()
}

/// How many subviews the view has: the layout's page, then pages of the plug-in parameters
/// it doesn't show.
fn pages(app: &App, v: View) -> usize {
    let Some(s) = app.sc.screens.get(&v) else { return 1 };
    let per = s.controls.len().max(1);
    1 + page_params(app, v).len().saturating_sub(per).div_ceil(per)
}

/// A subview's controls: page 0 is the layout's own; later pages the next parameters.
fn page_controls(app: &App, v: View, s: &Screen) -> Vec<Control> {
    if s.subview == 0 {
        return s.controls.clone();
    }
    let per = s.controls.len().max(1);
    page_params(app, v).iter().skip(s.subview * per).take(per).map(|x| Control { label: x.2.name.clone(), kind: if x.2.unit == lg_dsp::Unit::OnOff { Kind::Button } else { Kind::Knob }, maps: vec![Mapping::new(x.0)], cc: None }).collect()
}

fn format_value(v: f32, unit: lg_dsp::Unit) -> String {
    use lg_dsp::Unit::*;
    match unit {
        Db => format!("{v:.1} dB"),
        Hz => if v >= 1000.0 { format!("{:.2} kHz", v / 1000.0) } else { format!("{v:.0} Hz") },
        Ms => format!("{v:.0} ms"),
        Seconds => format!("{v:.2} s"),
        Percent => format!("{v:.0} %"),
        Ratio => format!("{v:.1} : 1"),
        Semitones => format!("{v:+.0} st"),
        Cents => format!("{v:+.0} ct"),
        Octaves => format!("{v:.2} oct"),
        Pan => format!("{:+.0}", v * 64.0),
        OnOff => if v >= 0.5 { "On".into() } else { "Off".into() },
        _ => format!("{v:.2}"),
    }
}

fn header_button(ui: &mut egui::Ui, r: Rect, id: &str, label: &str, on: bool, tip: &str) -> egui::Response {
    let resp = ui.interact(r, ui.scope_id().with(("sch", id)), Sense::click()).on_hover_text(tip);
    let p = ui.painter();
    p.rect_filled(r, 5.0, if on { look::accent() } else if resp.hovered() { look::button_on() } else { look::group_bg() });
    if label.chars().count() == 1 && !label.is_ascii() {
        crate::glyphs::paint(p, r, label, 14.0, if on { Color32::WHITE } else { look::text() });
    } else {
        p.text(r.center(), Align2::CENTER_CENTER, label, look::regular(12.0), if on { Color32::WHITE } else { look::text() });
    }
    resp
}

/// Keys while the pointer is over Smart Controls: the Smart Controls group.
pub fn key_press(app: &mut App, key: egui::Key, m: egui::Modifiers) -> bool {
    let over = app.ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| app.sc.rect.is_some_and(|r| r.contains(p))) && (app.sc.window || app.bottom == crate::app::Bottom::SmartControls);
    if !over {
        return false;
    }
    for b in crate::keymap::bindings().iter().filter(|b| b.area == "Smart Controls") {
        if b.key == key && b.cmd == m.command && b.alt == m.alt && b.shift == m.shift {
            let c = b.command.clone();
            return command(app, &c);
        }
    }
    false
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    app.sc.rect = Some(area);
    update(app);
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let Some(v) = view(app) else {
        p.text(area.center(), Align2::CENTER_CENTER, "Select a track to see its Smart Controls", look::regular(13.0), look::text_dim());
        return;
    };
    let s = screen(app, v);
    // ---- the header
    let head = Rect::from_min_size(area.min, vec2(area.width(), 34.0));
    let mut x = head.min.x + 10.0;
    let b = |x: f32, w: f32| Rect::from_min_size(pos2(x, head.center().y - 11.0), vec2(w, 22.0));
    if header_button(ui, b(x, 26.0), "insp", crate::glyphs::ph::INFO, app.sc.inspector, "Smart Controls Inspector").clicked() {
        app.sc.inspector = !app.sc.inspector;
    }
    x += 34.0;
    if header_button(ui, b(x, 50.0), "track", "Track", !app.sc.master, "Show the track's Smart Controls").clicked() && app.sc.master {
        command(app, "Toggle Track Master View");
    }
    if header_button(ui, b(x + 52.0, 56.0), "master", "Master", app.sc.master, "Show the Smart Controls for the master effects").clicked() && !app.sc.master {
        command(app, "Toggle Track Master View");
    }
    // the Master Effects pop-up: Master and the auxes the track sends to
    let dests = destinations(app);
    if dests.len() > 1 || app.sc.master {
        let dr = Rect::from_min_size(pos2(x + 110.0, head.center().y - 11.0), vec2(16.0, 22.0));
        let dresp = ui.interact(dr, ui.scope_id().with("sc-dest"), Sense::click()).on_hover_text("Master Effects: the master or an aux the track sends to");
        crate::glyphs::paint(&p, dr, crate::glyphs::ph::CARET_DOWN, 11.0, look::text());
        egui::Popup::menu(&dresp).show(|ui| {
            if ui.add(egui::Button::selectable(app.sc.master, "Master")).clicked() {
                app.sc.master = true;
                app.sc.dest = None;
                ui.close();
            }
            for d in dests.iter().flatten() {
                let n = app.auxes.get(*d).map(|a| a.name.clone()).unwrap_or_default();
                if ui.add(egui::Button::selectable(!app.sc.master && app.sc.dest == Some(*d), n)).clicked() {
                    app.sc.master = false;
                    app.sc.dest = Some(*d);
                    app.sc.selected = 0;
                    ui.close();
                }
            }
        });
    }
    x += 130.0;
    // audio tracks: the Tuner button
    if let View::Track(t) = v {
        if app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.kind == lg_core::TrackKind::Audio) {
            if header_button(ui, b(x, 50.0), "tuner", "Tuner", app.show_tuner, "Open the Tuner").clicked() {
                app.show_tuner = !app.show_tuner;
            }
            x += 58.0;
        }
    }
    // a track with an Articulation Set: the Keyswitches button (artic.rs)
    if let View::Track(t) = v {
        if app.artic.tracks.contains_key(&t) {
            let on = app.artic.keys_view;
            if header_button(ui, b(x, 92.0), "keys", "Keyswitches", on, "Show the Keyswitches view").clicked() {
                app.artic.keys_view = !on;
            }
            x += 100.0;
            if app.artic.keys_view {
                crate::artic::keyswitch_view(app, ui, Rect::from_min_max(pos2(area.min.x, head.max.y), area.max), t);
                return;
            }
        }
    }
    let title = match v {
        View::Track(t) => {
            let name = app.project.as_ref().map(|p| p.tracks[t].name.clone()).unwrap_or_default();
            let inst = app.instrument_for(t).map(|n| crate::soundlib::plugin_of(&n).to_string()).unwrap_or_default();
            if inst.is_empty() || inst == name { name } else { format!("{name} — {inst}") }
        }
        View::Aux(k) => app.auxes.get(k).map(|a| a.name.clone()).unwrap_or_default(),
        View::Master => "Stereo Out".into(),
    };
    let title_r = Rect::from_min_max(pos2(x, head.min.y), pos2(head.max.x - 260.0, head.max.y));
    p.with_clip_rect(title_r).text(pos2(x, head.center().y), Align2::LEFT_CENTER, &title, look::semibold(13.0), look::text());
    let lw = p.layout_no_wrap(title.clone(), look::semibold(13.0), look::text()).size().x;
    p.with_clip_rect(title_r).text(pos2(x + lw + 10.0, head.center().y), Align2::LEFT_CENTER, LAYOUTS[s.layout.min(LAYOUTS.len() - 1)].0, look::regular(11.5), look::text_dim());
    // at the right: Compare, the subview, the Arpeggiator
    // (narrow: the right-hand buttons stay clear of Track / Master)
    let right = (head.max.x - 10.0).max(x + 250.0);
    let can_compare = s.saved.is_some() || s.comparing;
    if header_button(ui, b(right - 240.0, 70.0), "compare", "Compare", s.comparing, "Compare the edits with the saved settings").clicked() && can_compare {
        compare(app, v);
    }
    let np = pages(app, v);
    if np > 1 && header_button(ui, b(right - 164.0, 54.0), "sub", &format!("{}/{}", s.subview + 1, np), false, "Cycle Through Subviews (⌃⇧O)").clicked() {
        command(app, "Cycle Through Subviews");
    }
    if let View::Track(t) = v {
        let a = app.sc.arps.get(&t).copied().unwrap_or_default();
        let ab = b(right - 104.0, 80.0);
        let resp = header_button(ui, ab, "arp", "Arpeggiator", a.on, "Arpeggiator: click to turn it on or off; the arrow for its settings");
        if resp.clicked() {
            command(app, "Toggle Channel Strip Arpeggiator");
        }
        let arrow = Rect::from_min_size(pos2(ab.max.x + 2.0, ab.min.y), vec2(20.0, 22.0));
        let aresp = ui.interact(arrow, ui.scope_id().with("arp-menu"), Sense::click()).on_hover_text("Arpeggiator settings");
        crate::glyphs::paint(&p, arrow, crate::glyphs::ph::CARET_DOWN, 12.0, look::text());
        egui::Popup::menu(&aresp).show(|ui| arp_menu(app, ui, t));
    }
    p.hline(head.x_range(), head.max.y, Stroke::new(1.0, look::line()));
    let mut body = Rect::from_min_max(pos2(area.min.x, head.max.y + 1.0), area.max);
    if app.sc.inspector {
        let ir = Rect::from_min_max(body.min, pos2(body.min.x + 250.0, body.max.y));
        inspector(app, ui, ir, v);
        body.min.x = ir.max.x + 1.0;
    }
    // the EQ's curve on the right (a track's own strip)
    if let View::Track(t) = v {
        let eq_w = (body.width() * 0.2).clamp(110.0, 300.0);
        let eq_r = Rect::from_min_max(pos2(body.max.x - eq_w - 12.0, body.min.y + 14.0), pos2(body.max.x - 12.0, body.max.y - 14.0));
        eq_thumb(app, ui, eq_r, t);
        body.max.x = eq_r.min.x - 6.0;
    }
    // ---- the screen controls
    let s = screen(app, v);
    let controls = page_controls(app, v, &s);
    let all = parameters(app, v);
    let knobs: Vec<(usize, &Control)> = controls.iter().enumerate().filter(|c| c.1.kind == Kind::Knob).collect();
    let buttons: Vec<(usize, &Control)> = controls.iter().enumerate().filter(|c| c.1.kind == Kind::Button).collect();
    let inner = body.shrink2(vec2(14.0, 10.0));
    let btn_w = if buttons.is_empty() { 0.0 } else { 116.0 };
    let kn = Rect::from_min_max(inner.min, pos2(inner.max.x - btn_w, inner.max.y));
    let fit = ((kn.width() / 64.0).floor() as usize).max(1);
    let per_row = (if knobs.len() > 8 { knobs.len().div_ceil(2) } else { knobs.len().max(1) }).min(fit);
    let rows = knobs.len().div_ceil(per_row).max(1);
    let kw = (kn.width() / per_row as f32).min(100.0);
    let kh = (kn.height() / rows as f32).min(116.0);
    let mut moves: Vec<(usize, f32)> = Vec::new();
    let pointer_down = ui.input(|i| i.pointer.any_down());
    let insp = app.sc.inspector;
    let knobs: Vec<(usize, &Control)> = if kw < 34.0 || kh < 40.0 { Vec::new() } else { knobs };
    for (n, (ci, c)) in knobs.iter().enumerate() {
        let r = Rect::from_min_size(kn.min + vec2((n % per_row) as f32 * kw, (n / per_row) as f32 * kh), vec2(kw, kh)).shrink(6.0);
        if s.subview == 0 && app.sc.selected == *ci && insp {
            p.rect_stroke(r.expand(3.0), 6.0, Stroke::new(1.5, look::accent()), egui::StrokeKind::Inside);
        }
        let learning = app.sc.learn.as_ref().is_some_and(|l| l.0 == v && l.1 == *ci) || app.sc.learn_cc.is_some_and(|l| l.0 == v && l.1 == *ci);
        if learning {
            p.rect_filled(r.expand(3.0), 6.0, look::record_red().gamma_multiply(0.25));
        }
        // (with the inspector open a click selects the control to edit)
        if insp && s.subview == 0 && ui.interact(r, ui.scope_id().with(("sc-sel", *ci)), Sense::click()).clicked() {
            app.sc.selected = *ci;
        }
        match control_value(c, &all) {
            Some(val) => {
                let mut x = val;
                let first = c.maps.first().and_then(|m| param_of(&all, m.target).cloned().map(|i| (*m, i)));
                let fmt = move |v: f32| first.as_ref().map(|(m, i)| format_value(i.min + m.param_share(v) * (i.max - i.min), i.unit)).unwrap_or_default();
                if widgets::knob(ui, r, ui.scope_id().with(("sc", v, s.subview, *ci)), &mut x, 0.0, 1.0, control_value_default(c, &all), &c.label, &fmt) {
                    moves.push((*ci, x));
                    if s.subview == 0 {
                        app.sc.selected = *ci;
                    }
                }
            }
            None => {
                // an unmapped control
                let d = r.width().min(r.height() - 18.0) - 4.0;
                p.circle_stroke(pos2(r.center().x, r.min.y + d / 2.0 + 2.0), d / 2.0 - 2.0, Stroke::new(1.0, look::line()));
                p.text(pos2(r.center().x, r.max.y - 8.0), Align2::CENTER_CENTER, if c.label.is_empty() { "—" } else { c.label.as_str() }, look::regular(11.0), look::text_dim());
                if !insp && ui.interact(r, ui.scope_id().with(("sc-un", *ci)), Sense::click()).clicked() {
                    app.sc.selected = *ci;
                    app.sc.inspector = true;
                }
            }
        }
    }
    // Control-click (right-click) a screen control: its shortcut menu
    if let Some(pp) = ui.input(|i| i.pointer.secondary_clicked().then(|| i.pointer.interact_pos()).flatten()) {
        for (n, (ci, _)) in knobs.iter().enumerate() {
            let r = Rect::from_min_size(kn.min + vec2((n % per_row) as f32 * kw, (n / per_row) as f32 * kh), vec2(kw, kh)).shrink(6.0);
            if r.contains(pp) {
                app.sc.ctx_menu = Some((v, *ci, pp));
            }
        }
        for (n, (ci, _)) in buttons.iter().enumerate() {
            let r = Rect::from_min_size(pos2(inner.max.x - btn_w + 10.0, inner.min.y + n as f32 * 30.0), vec2(btn_w - 10.0, 24.0));
            if r.contains(pp) {
                app.sc.ctx_menu = Some((v, *ci, pp));
            }
        }
    }
    for (n, (ci, c)) in buttons.iter().enumerate() {
        let r = Rect::from_min_size(pos2(inner.max.x - btn_w + 10.0, inner.min.y + n as f32 * 30.0), vec2(btn_w - 10.0, 24.0));
        let on = control_value(c, &all).is_some_and(|v| v >= 0.5);
        let resp = ui.interact(r, ui.scope_id().with(("sc-btn", v, *ci)), Sense::click());
        p.rect_filled(r, 5.0, if on { look::accent() } else { look::group_bg() });
        if s.subview == 0 && app.sc.selected == *ci && insp {
            p.rect_stroke(r.expand(2.0), 6.0, Stroke::new(1.5, look::accent()), egui::StrokeKind::Inside);
        }
        p.with_clip_rect(r).text(r.center(), Align2::CENTER_CENTER, if c.label.is_empty() { "—" } else { c.label.as_str() }, look::regular(11.5), if on { Color32::WHITE } else { look::text() });
        if resp.clicked() {
            app.sc.selected = *ci;
            if !c.maps.is_empty() {
                moves.push((*ci, if on { 0.0 } else { 1.0 }));
            }
        }
    }
    if !moves.is_empty() {
        // a channel control's drag is one undo step (plug-in parameters stay out of the
        // project's undo, as in the plug-in windows)
        let channel = moves.iter().any(|(ci, _)| controls.get(*ci).is_some_and(|c| c.maps.iter().any(|m| matches!(m.target, Target::Volume | Target::Pan))));
        if channel && !app.sc.drag_step {
            app.checkpoint("Smart Control");
            app.sc.drag_step = true;
        }
        for (ci, x) in moves {
            if s.subview == 0 {
                move_control(app, v, ci, x);
            } else if let Some(c) = controls.get(ci) {
                for m in &c.maps {
                    if let Some(i) = param_of(&all, m.target) {
                        set_target(app, v, m.target, i.min + m.param_share(x) * (i.max - i.min));
                    }
                }
            }
        }
    }
    if !pointer_down {
        app.sc.drag_step = false;
    }
    if knobs.is_empty() && buttons.is_empty() {
        p.text(inner.center(), Align2::CENTER_CENTER, if v == View::Master { "No master effects: add one to the Stereo Out to see its Smart Controls" } else { "No Smart Controls for this channel strip" }, look::regular(12.0), look::text_dim());
    }
}

fn control_value_default(c: &Control, all: &[(Target, String, ParamInfo)]) -> f32 {
    c.maps.first().and_then(|m| param_of(all, m.target).map(|i| m.control_value(if i.max > i.min { (i.default - i.min) / (i.max - i.min) } else { 0.0 }))).unwrap_or(0.0)
}

/// The Arpeggiator's menu: Off, the note orders, Rate, Octave Range, Latch, Note Length,
/// Velocity.
fn arp_menu(app: &mut App, ui: &mut egui::Ui, t: usize) {
    let mut a = app.sc.arps.get(&t).copied().unwrap_or_default();
    let before = a;
    ui.set_min_width(200.0);
    if ui.add(egui::Button::selectable(!a.on, "Off")).clicked() {
        a.on = false;
    }
    ui.separator();
    ui.label(egui::RichText::new("Note Order").color(look::text_dim()));
    for o in arp::Order::ALL {
        if ui.add(egui::Button::selectable(a.on && a.order == o, o.name())).clicked() {
            a.order = o;
            a.on = true;
        }
    }
    ui.separator();
    ui.menu_button(format!("Rate: {}", arp::RATES[a.rate.min(arp::RATES.len() - 1)].0), |ui| {
        for (k, (n, _)) in arp::RATES.iter().enumerate() {
            if ui.add(egui::Button::selectable(a.rate == k, *n)).clicked() {
                a.rate = k;
            }
        }
    });
    ui.menu_button(format!("Octave Range: {}", a.octaves), |ui| {
        for o in 1..=4u8 {
            if ui.add(egui::Button::selectable(a.octaves == o, o.to_string())).clicked() {
                a.octaves = o;
            }
        }
    });
    ui.menu_button(format!("Latch: {}", a.latch.name()), |ui| {
        for l in arp::Latch::ALL {
            if ui.add(egui::Button::selectable(a.latch == l, l.name())).clicked() {
                a.latch = l;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Note Length");
        let mut g = a.gate * 100.0;
        if ui.add(egui::DragValue::new(&mut g).range(5.0..=100.0).suffix(" %")).changed() {
            a.gate = g / 100.0;
        }
    });
    ui.horizontal(|ui| {
        let mut fixed = a.velocity.is_some();
        if ui.checkbox(&mut fixed, "Fixed Velocity").changed() {
            a.velocity = if fixed { Some(100) } else { None };
        }
        if let Some(v) = a.velocity.as_mut() {
            ui.add(egui::DragValue::new(v).range(1..=127));
        }
    });
    if a != before {
        app.sc.arps.insert(t, a);
        app.dirty = true;
        app.load_engine();
    }
}

/// The Smart Controls inspector: the layout, the selected control's name, its mappings (each
/// with its parameter and range), Add Mapping, Learn; the external assignment.
fn inspector(app: &mut App, ui: &mut egui::Ui, r: Rect, v: View) {
    let p = ui.painter_at(r);
    p.rect_filled(r, 0.0, look::header_bg());
    p.vline(r.max.x, r.y_range(), Stroke::new(1.0, look::line()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink(8.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    let ui = &mut child;
    let mut s = screen(app, v);
    let before = s.clone();
    let mut run: Option<&str> = None;
    let sel = app.sc.selected.min(s.controls.len().saturating_sub(1));
    let learning = app.sc.learn.as_ref().is_some_and(|l| l.0 == v && l.1 == sel);
    let learning_cc = app.sc.learn_cc.is_some_and(|l| l.0 == v && l.1 == sel);
    let all = parameters(app, v);
    let mut run_open: Option<Vec<Target>> = None;
    egui::ScrollArea::vertical().id_salt("sc-insp").auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("Layout");
            egui::ComboBox::from_id_salt("sc-layout").selected_text(LAYOUTS[s.layout.min(LAYOUTS.len() - 1)].0).show_ui(ui, |ui| {
                // (Logic chooses the layout for the patch and maps every control)
                if ui.button("Automatic Smart Controls").clicked() {
                    s.layout = layout_for(app, v);
                    s.controls = default_controls(app, v, s.layout);
                    s.subview = 0;
                }
                ui.separator();
                for (k, l) in LAYOUTS.iter().enumerate() {
                    if ui.add(egui::Button::selectable(s.layout == k, l.0)).clicked() && s.layout != k {
                        s.layout = k;
                        s.controls = default_controls(app, v, k);
                        s.subview = 0;
                    }
                }
            });
        });
        ui.separator();
        let Some(c) = s.controls.get_mut(sel) else { return };
        ui.label(egui::RichText::new(format!("Screen Control {}", sel + 1)).color(look::text_dim()));
        ui.add(egui::TextEdit::singleline(&mut c.label).hint_text("Name"));
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Parameter Mapping").strong());
        let mut remove = None;
        let mut swap: Option<(usize, usize)> = None;
        let n_maps = c.maps.len();
        let mut open_plugin: Option<Target> = None;
        let mut open_graph: Option<usize> = None;
        let mut clip_op: Option<(&str, usize)> = None;
        if c.maps.is_empty() {
            // the Name field: Unmapped (a click chooses the parameter)
            ui.menu_button("Unmapped", |ui| {
                let mut t = Target::Volume;
                if param_menu(ui, &all, &mut t) {
                    c.maps.push(Mapping::new(t));
                    if c.label.is_empty() {
                        c.label = all.iter().find(|x| x.0 == t).map(|x| x.2.name.clone()).unwrap_or_default();
                    }
                }
                ui.separator();
                mapping_menu_items(ui, &mut clip_op, 0, false);
            });
        }
        for (k, m) in c.maps.iter_mut().enumerate() {
            let path = all.iter().find(|x| x.0 == m.target).map(|x| x.1.clone()).unwrap_or_else(|| "Unmapped".into());
            ui.push_id(("map", k), |ui| {
                ui.horizontal(|ui| {
                    // the three-dot handle: drag up or down to reorder (the top one shows the
                    // control's values)
                    if n_maps > 1 {
                        let h = ui.add(egui::Label::new("⋮").sense(Sense::drag())).on_hover_text("Drag to reorder the mappings");
                        if h.dragged() {
                            let dy = h.total_drag_delta().map_or(0.0, |d| d.y);
                            if dy < -40.0 && k > 0 {
                                swap = Some((k, k - 1));
                            } else if dy > 40.0 && k + 1 < n_maps {
                                swap = Some((k, k + 1));
                            }
                        }
                    }
                    ui.menu_button(crate::tracks::ellipsize(&path, 30), |ui| {
                        param_menu(ui, &all, &mut m.target);
                        ui.separator();
                        mapping_menu_items(ui, &mut clip_op, k, true);
                    });
                    if matches!(m.target, Target::Instrument(_) | Target::Effect(..)) && ui.small_button("🔌").on_hover_text("Open the plug-in window").clicked() {
                        open_plugin = Some(m.target);
                    }
                });
                let mut lo = m.lo.min(m.hi) * 100.0;
                let mut hi = m.lo.max(m.hi) * 100.0;
                let mut inv = m.lo > m.hi;
                let r1 = ui.add(egui::Slider::new(&mut hi, 0.0..=100.0).suffix("%").text("Range Max"));
                let r2 = ui.add(egui::Slider::new(&mut lo, 0.0..=100.0).suffix("%").text("Range Min"));
                let r3 = ui.checkbox(&mut inv, "Invert Range");
                if r1.changed() || r2.changed() || r3.changed() {
                    let (a, b) = (lo.min(hi) / 100.0, lo.max(hi) / 100.0);
                    (m.lo, m.hi) = if inv { (b, a) } else { (a, b) };
                }
                ui.horizontal(|ui| {
                    ui.label(if m.graph.is_linear() { "Scaling: Linear" } else { "Scaling: Curve" });
                    if ui.small_button("Open").on_hover_text("The parameter mapping graph").clicked() {
                        open_graph = Some(k);
                    }
                });
                ui.add_space(4.0);
            });
        }
        if let Some((a, b)) = swap {
            c.maps.swap(a, b);
        }
        if let Some(t) = open_plugin {
            run_open = Some(vec![t]);
        }
        if let Some(k) = open_graph {
            app.sc.graph.open = Some((v, sel, k));
        }
        if let Some((op, k)) = clip_op {
            match op {
                "Copy Mapping" => app.sc.map_clip = c.maps.get(k).map(|m| vec![*m]),
                "Copy All Mappings" => app.sc.map_clip = Some(c.maps.clone()),
                "Paste Mapping" => {
                    if let Some(m) = app.sc.map_clip.as_ref().and_then(|v| v.first()).copied() {
                        if k < c.maps.len() {
                            c.maps[k] = m;
                        } else {
                            c.maps.push(m);
                        }
                    }
                }
                "Paste Mappings" => {
                    if let Some(ms) = app.sc.map_clip.clone() {
                        c.maps = ms;
                    }
                }
                "Delete Mapping" => remove = Some(k),
                other => run = Some(if other == "Map all Controls" { "Map all Controls" } else { "Map all Unmapped Controls" }),
            }
        }
        if let Some(k) = remove.filter(|k| *k < c.maps.len()) {
            c.maps.remove(k);
        }
        ui.horizontal(|ui| {
            ui.menu_button("Add Mapping", |ui| {
                let mut t = Target::Volume;
                if param_menu(ui, &all, &mut t) {
                    if !c.maps.iter().any(|m| m.target == t) {
                        c.maps.push(Mapping::new(t));
                    }
                    if c.label.is_empty() {
                        c.label = all.iter().find(|x| x.0 == t).map(|x| x.2.name.clone()).unwrap_or_default();
                    }
                }
            });
            if ui.add(egui::Button::selectable(learning, "Learn")).on_hover_text("Learn Parameter Mapping: then move a parameter in a plug-in window or the channel strip").clicked() {
                run = Some("Learn Parameter Mapping");
            }
        });
        ui.separator();
        ui.label(egui::RichText::new("External Assignment").strong());
        ui.horizontal(|ui| {
            ui.label(match c.cc { Some(n) => format!("Controller {n}"), None => "Unassigned".into() });
            if ui.add(egui::Button::selectable(learning_cc, "Learn")).on_hover_text("Learn External Assignment: then move a control on your MIDI controller").clicked() {
                run = Some("Learn External Assignment");
            }
            if c.cc.is_some() && ui.small_button("✕").clicked() {
                c.cc = None;
            }
        });
    });
    if s != before {
        app.sc.screens.insert(v, s);
        app.dirty = true;
    }
    if let Some(cmd) = run {
        app.sc.selected = sel;
        command(app, cmd);
    }
    if let Some(ts) = run_open {
        open_plugin_windows(app, v, &ts);
    }
}

/// The mapping pop-up's commands (Add Mapping is its own menu).
fn mapping_menu_items<'a>(ui: &mut egui::Ui, op: &mut Option<(&'a str, usize)>, k: usize, mapped: bool) {
    for (name, needs) in [("Copy Mapping", true), ("Paste Mapping", false), ("Copy All Mappings", true), ("Paste Mappings", false), ("Delete Mapping", true), ("Map all Controls", false), ("Map all Unmapped Controls", false)] {
        let name: &'a str = name;
        if ui.add_enabled(mapped || !needs, egui::Button::new(name)).clicked() {
            *op = Some((name, k));
            ui.close();
        }
    }
}

/// Open Plug-in Window for a screen control: every plug-in its mappings reach (the instrument,
/// the Audio FX).
pub fn open_plugin_windows(app: &mut App, v: View, targets: &[Target]) {
    let mut any = false;
    for t in targets {
        match (*t, v) {
            (Target::Instrument(_), View::Track(tr)) => {
                app.plugin_window = Some(tr);
                any = true;
            }
            (Target::Effect(k, _), _) => {
                app.mix.fx_window = Some((v.strip(), k));
                any = true;
            }
            _ => {}
        }
    }
    if !any {
        app.tell("This screen control moves no plug-in parameter");
    }
}

/// A screen control's shortcut menu (Control-click): Open Plug-in Window.
fn control_menu(app: &mut App, ctx: &egui::Context) {
    let Some((v, ci, pos)) = app.sc.ctx_menu else { return };
    let targets: Vec<Target> = app.sc.screens.get(&v).and_then(|s| s.controls.get(ci)).map_or(Vec::new(), |c| c.maps.iter().map(|m| m.target).filter(|t| matches!(t, Target::Instrument(_) | Target::Effect(..))).collect());
    let mut close = false;
    let resp = egui::Area::new(egui::Id::unique("sc-ctl-menu")).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::menu(ui.style()).show(ui, |ui| {
            ui.set_min_width(180.0);
            if ui.add_enabled(!targets.is_empty(), egui::Button::new("Open Plug-in Window")).clicked() {
                open_plugin_windows(app, v, &targets);
                close = true;
            }
        });
    });
    if close || (ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer()) {
        app.sc.ctx_menu = None;
    }
}

/// The parameter chooser (grouped by the path's first part); true when one was picked.
fn param_menu(ui: &mut egui::Ui, all: &[(Target, String, ParamInfo)], target: &mut Target) -> bool {
    let mut groups: Vec<String> = Vec::new();
    for x in all {
        let g = x.1.split(" ▸ ").next().unwrap_or("").to_string();
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    let mut picked = false;
    for g in groups {
        ui.menu_button(g.clone(), |ui| {
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for x in all.iter().filter(|x| x.1.starts_with(&format!("{g} ▸ "))) {
                    if ui.add(egui::Button::selectable(*target == x.0, &x.2.name)).clicked() {
                        *target = x.0;
                        picked = true;
                        ui.close();
                    }
                }
            });
        });
    }
    picked
}

/// Window ▸ Open Smart Controls.
pub fn window(app: &mut App, ctx: &egui::Context) {
    crate::scgraph::window(app, ctx);
    control_menu(app, ctx);
    if !app.sc.window {
        return;
    }
    let mut open = true;
    egui::Window::new("Smart Controls").open(&mut open).default_size(vec2(860.0, 230.0)).resizable(true).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().max(640.0), ui.available_height().max(200.0)), Sense::hover());
        show(app, ui, r);
    });
    if !open {
        app.sc.window = false;
    }
}

/// The Channel EQ's curve in a dark box (a click opens the EQ).
fn eq_thumb(app: &mut App, ui: &mut egui::Ui, r: Rect, t: usize) {
    let p = ui.painter();
    p.rect_filled(r, 6.0, look::lcd_bg());
    let e = app.eqs.get(&t).copied();
    for k in 1..4 {
        let y = r.min.y + r.height() * k as f32 / 4.0;
        p.hline(r.x_range(), y, Stroke::new(1.0, Color32::from_white_alpha(14)));
    }
    let curve = e.unwrap_or_default();
    let pts: Vec<egui::Pos2> = (0..=80).map(|k| {
        let f = 20.0 * 1000f32.powf(k as f32 / 80.0);
        pos2(r.min.x + k as f32 / 80.0 * r.width(), r.center().y - crate::eq::response(&curve, f).clamp(-24.0, 24.0) / 24.0 * (r.height() / 2.0 - 6.0))
    }).collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.6, Color32::from_rgb(0x9c, 0xe0, 0xff))));
    p.text(r.left_top() + vec2(8.0, 6.0), Align2::LEFT_TOP, "EQ", look::semibold(11.0), look::lcd_dim());
    if ui.interact(r, ui.scope_id().with(("sc-eqopen", t)), Sense::click()).on_hover_text("Open the Channel EQ").clicked() {
        app.eqs.entry(t).or_default();
        app.eq_window = Some(t);
        app.load_engine();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_ranges() {
        let m = Mapping { lo: 0.25, hi: 0.75, ..Mapping::new(Target::Volume) };
        assert_eq!(m.param_share(0.0), 0.25);
        assert_eq!(m.param_share(1.0), 0.75);
        assert!((m.control_value(0.5) - 0.5).abs() < 1e-6);
        // inverted
        let m = Mapping { lo: 1.0, hi: 0.0, ..Mapping::new(Target::Volume) };
        assert_eq!(m.param_share(0.0), 1.0);
        assert!((m.control_value(0.25) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn layouts_and_views() {
        assert!(LAYOUTS.iter().all(|l| l.1 + l.2 > 0));
        assert!(LAYOUTS.iter().any(|l| l.0 == "Synthesizer" && l.1 == 12));
        assert_eq!(View::Master.strip(), crate::mixstate::MASTER);
        assert_eq!(View::Aux(2).strip(), crate::mixer::AUX + 2);
    }
}
