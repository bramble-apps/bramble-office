//! Scripter: Logic's JavaScript MIDI plug-in, on our own interpreter (js.rs). Written from
//! Logic's user guide (the Scripter API: HandleMIDI, ProcessMIDI, ParameterChanged, Reset,
//! GetParameter / SetParameter, PluginParameters, NeedsTimingInfo, GetTimingInfo, Trace, the
//! Event types and the MIDI object).
//!
//! - The unit's values: the script's parameter values (`MAX_PARAMS` floats), then the script,
//!   one character per float, so it saves with the project / setting, copies and undoes like
//!   any other MIDI plug-in state.
//! - A new script (Run Script) makes a fresh interpreter: the script runs once at the global
//!   level, PluginParameters is read, and ParameterChanged is called once per parameter.
//! - Each block: Reset() when the transport starts, then HandleMIDI(event) per incoming event
//!   (events pass through when the script has no HandleMIDI), then ProcessMIDI(). Events sent
//!   for later (sendAfterMilliseconds, sendAtBeat, sendAfterBeats) wait in the unit.
//! - The console (Trace, event.trace, errors with their line) and the parameters as the script
//!   last declared them reach the window through `Status::script`.

use crate::js::{obj, Abort, Interp, Kind, ObjRef, Value};
use crate::midifx::{cc_label, note_name, Ctx, Ev, MidiFx, Msg, Status, PPQ};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// Floats kept for the script's parameter values.
pub const MAX_PARAMS: usize = 64;
/// Where the script text starts in the values.
pub const SCRIPT_AT: usize = MAX_PARAMS;
/// Console lines kept.
const CONSOLE_MAX: usize = 400;
/// Lines one block may print before the console thins the rest out.
const THIN_AT: usize = 40;

pub const DEFAULT_SCRIPT: &str = "// Scripter: HandleMIDI is called for each MIDI event the plug-in receives.\n// Change the script, then click Run Script.\n\nfunction HandleMIDI(event)\n{\n\tevent.send();\n}\n";

/// The script kept in a unit's values (the default script when there is none).
pub fn script_of(values: &[f32]) -> String {
    if values.len() <= SCRIPT_AT {
        return DEFAULT_SCRIPT.to_string();
    }
    values[SCRIPT_AT..].iter().filter_map(|&f| char::from_u32(f.max(0.0) as u32)).collect()
}

/// `values` with its script replaced.
pub fn with_script(values: &[f32], script: &str) -> Vec<f32> {
    let mut v: Vec<f32> = values.iter().take(SCRIPT_AT).copied().collect();
    v.resize(SCRIPT_AT, 0.0);
    v.extend(script.chars().map(|c| c as u32 as f32));
    v
}

/// A new Scripter: the default script and its parameters' defaults.
pub fn defaults() -> Vec<f32> {
    with_script(&[], DEFAULT_SCRIPT)
}

/// A script and the parameter values it starts with (its parameters' defaults).
pub fn load_script(script: &str) -> Vec<f32> {
    let (ps, _) = read_params(script);
    let mut v = vec![0.0; MAX_PARAMS];
    for (i, p) in ps.iter().enumerate().take(MAX_PARAMS) {
        v[i] = p.default;
    }
    with_script(&v, script)
}

/// After Run Script: parameters that kept their name (and kind) keep their values, the others
/// start at their defaults.
pub fn rerun_values(old_values: &[f32], old_script: &str, new_script: &str) -> Vec<f32> {
    let (old, _) = read_params(old_script);
    let (new, _) = read_params(new_script);
    let mut v = vec![0.0; MAX_PARAMS];
    for (i, p) in new.iter().enumerate().take(MAX_PARAMS) {
        v[i] = match old.get(i) {
            Some(o) if o.name == p.name && o.kind == p.kind => old_values.get(i).copied().unwrap_or(p.default).clamp(p.min.min(p.max), p.max.max(p.min)),
            _ => p.default,
        };
    }
    with_script(&v, new_script)
}

// ---------------------------------------------------------------------------------------------
// parameters

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PKind {
    #[default]
    Lin,
    Log,
    Momentary,
    Menu,
    Checkbox,
    /// A header / divider (type "text"): no value.
    Text,
    /// A destination menu for TargetEvent.
    Target,
}

/// One control the script declared in PluginParameters.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SParam {
    pub name: String,
    pub kind: PKind,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub steps: Option<u32>,
    pub unit: String,
    pub strings: Vec<String>,
    pub hidden: bool,
    pub no_automation: bool,
}

impl SParam {
    /// A value snapped to the control's steps (menus and checkboxes to whole numbers).
    pub fn snap(&self, v: f32) -> f32 {
        let (lo, hi) = (self.min.min(self.max), self.max.max(self.min));
        let v = v.clamp(lo, hi);
        match self.kind {
            PKind::Menu | PKind::Target => v.round().clamp(0.0, (self.strings.len().max(1) - 1) as f32),
            PKind::Checkbox | PKind::Momentary => (v >= 0.5) as u8 as f32,
            _ => match self.steps {
                Some(n) if n > 0 && hi > lo => {
                    let st = (hi - lo) / n as f32;
                    lo + ((v - lo) / st).round() * st
                }
                _ => v,
            },
        }
    }
    /// The value as the control's readout shows it.
    pub fn text(&self, v: f32) -> String {
        match self.kind {
            PKind::Menu | PKind::Target => self.strings.get(v.round().max(0.0) as usize).cloned().unwrap_or_default(),
            PKind::Checkbox | PKind::Momentary => if v >= 0.5 { "On".into() } else { "Off".into() },
            PKind::Text => String::new(),
            _ => {
                let integral = self.steps.is_some_and(|n| n > 0 && ((self.max - self.min) / n as f32).fract() == 0.0);
                let num = if integral { format!("{}", v.round() as i64) } else if (self.max - self.min).abs() > 20.0 { format!("{v:.1}") } else { format!("{v:.2}") };
                if self.unit.is_empty() { num } else { format!("{num} {}", self.unit) }
            }
        }
    }
}

/// The destinations a target parameter's menu offers: MIDI CC 0…127 by name.
pub fn target_names() -> Vec<String> {
    (0..128u8).map(|n| if cc_label(n).is_empty() { format!("MIDI CC {n}") } else { format!("MIDI CC {n} ({})", cc_label(n)) }).collect()
}

fn parse_params(it: &mut Interp) -> Result<Vec<SParam>, String> {
    let pp = it.get_global("PluginParameters");
    let list = match &pp {
        Value::Undef | Value::Null => return Ok(Vec::new()),
        Value::Obj(o) => match &o.borrow().kind {
            Kind::Array(a) => a.clone(),
            _ => return Err("PluginParameters must be an array".into()),
        },
        _ => return Err("PluginParameters must be an array".into()),
    };
    let mut out = Vec::new();
    for (i, d) in list.iter().enumerate() {
        let g = |it: &mut Interp, k: &str| it.get(d, k).unwrap_or_default();
        let name = g(it, "name");
        if name.is_undef() {
            return Err(format!("PluginParameters[{i}] has no name (name is required)"));
        }
        let name = it.to_string(&name).unwrap_or_default();
        let ty = g(it, "type");
        let ty = if ty.is_undef() { "lin".to_string() } else { it.to_string(&ty).unwrap_or_default() };
        let kind = match ty.as_str() {
            "log" => PKind::Log,
            "momentary" => PKind::Momentary,
            "menu" => PKind::Menu,
            "checkbox" => PKind::Checkbox,
            "text" => PKind::Text,
            "target" => PKind::Target,
            _ => PKind::Lin,
        };
        let num = |it: &mut Interp, k: &str, d: f32| {
            let v = g(it, k);
            if v.is_undef() { d } else { it.to_num(&v) as f32 }
        };
        let mut strings: Vec<String> = Vec::new();
        if let Value::Obj(o) = g(it, "valueStrings") {
            let a = if let Kind::Array(a) = &o.borrow().kind { a.clone() } else { Vec::new() };
            for x in a {
                strings.push(it.to_string(&x).unwrap_or_default());
            }
        }
        if kind == PKind::Menu && strings.is_empty() {
            return Err(format!("Menu parameter \"{name}\" needs a valueStrings array"));
        }
        if kind == PKind::Target {
            strings = target_names();
        }
        let (min, max) = match kind {
            PKind::Menu | PKind::Target => (0.0, (strings.len().max(1) - 1) as f32),
            PKind::Checkbox | PKind::Momentary => (0.0, 1.0),
            _ => (num(it, "minValue", 0.0), num(it, "maxValue", 1.0)),
        };
        let steps = {
            let v = g(it, "numberOfSteps");
            if v.is_undef() { None } else { Some(it.to_num(&v).max(0.0) as u32) }
        };
        let unit = {
            let v = g(it, "unit");
            if v.is_undef() { String::new() } else { it.to_string(&v).unwrap_or_default() }
        };
        let mut p = SParam { name, kind, min, max, default: 0.0, steps, unit, strings, hidden: g(it, "hidden").truthy(), no_automation: g(it, "disableAutomation").truthy() };
        p.default = p.snap(num(it, "defaultValue", 0.0));
        out.push(p);
    }
    Ok(out)
}

/// The parameters a script declares and its first error (line, message), by running it in a
/// scratch interpreter (the window's view of a script that isn't running).
pub fn read_params(script: &str) -> (Vec<SParam>, Option<(u32, String)>) {
    let mut it = new_interp();
    match it.eval_source(script) {
        Ok(_) => match parse_params(&mut it) {
            Ok(p) => (p, None),
            Err(e) => (Vec::new(), Some((0, e))),
        },
        Err(e) => {
            let line = it.line;
            let msg = it.describe(&e);
            (parse_params(&mut it).unwrap_or_default(), Some((line, msg)))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// the host side of the interpreter

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EKind {
    NoteOn,
    NoteOff,
    PolyPressure,
    ControlChange,
    ProgramChange,
    ChannelPressure,
    PitchBend,
    TargetEvent,
}

const KINDS: [(EKind, &str); 8] = [
    (EKind::NoteOn, "NoteOn"),
    (EKind::NoteOff, "NoteOff"),
    (EKind::PolyPressure, "PolyPressure"),
    (EKind::ControlChange, "ControlChange"),
    (EKind::ProgramChange, "ProgramChange"),
    (EKind::ChannelPressure, "ChannelPressure"),
    (EKind::PitchBend, "PitchBend"),
    (EKind::TargetEvent, "TargetEvent"),
];

/// The host's transport, for GetTimingInfo (set by the engine each block: meter, cycle).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transport {
    pub meter: (u8, u8),
    pub cycling: bool,
    /// The cycle range, song ticks.
    pub cycle: (f64, f64),
}

impl Default for Transport {
    fn default() -> Self {
        Transport { meter: (4, 4), cycling: false, cycle: (0.0, 4.0 * PPQ * 4.0) }
    }
}

static TRANSPORT: Mutex<Transport> = Mutex::new(Transport { meter: (4, 4), cycling: false, cycle: (0.0, 15360.0) });

/// The engine tells Scripter the song's meter and cycle (TimingInfo).
pub fn set_transport(t: Transport) {
    if let Ok(mut g) = TRANSPORT.lock() {
        *g = t;
    }
}

struct Host {
    ctx: Ctx,
    transport: Transport,
    /// Tick of the event being handled (the block start in ProcessMIDI).
    now: f64,
    /// Events sent: song tick, message.
    out: Vec<(f64, Msg)>,
    values: Vec<f32>,
    params: Vec<SParam>,
    params_changed: bool,
    /// SetParameter calls since the window last looked.
    set_serial: u64,
    needs_timing: bool,
    protos: Vec<ObjRef>,
    event_proto: Option<ObjRef>,
    warned_timing: bool,
    warned_beatpos: bool,
}

fn host(it: &mut Interp) -> &mut Host {
    it.host.downcast_mut::<Host>().expect("scripter host")
}

fn beat_of(tick: f64) -> f64 {
    tick / PPQ + 1.0
}

fn tick_of(beat: f64) -> f64 {
    (beat - 1.0) * PPQ
}

fn kind_of(it: &mut Interp, v: &Value) -> Option<EKind> {
    let mut p = v.as_obj()?.borrow().proto.clone();
    let protos = host(it).protos.clone();
    while let Some(o) = p {
        if let Some(k) = protos.iter().position(|q| Rc::ptr_eq(q, &o)) {
            return Some(KINDS[k].0);
        }
        p = o.borrow().proto.clone();
    }
    None
}

fn prop(it: &mut Interp, v: &Value, k: &str) -> f64 {
    let x = it.get(v, k).unwrap_or_default();
    it.to_num(&x)
}

fn clampi(x: f64, lo: i32, hi: i32) -> i32 {
    if x.is_nan() { lo } else { (x.round() as i64).clamp(lo as i64, hi as i64) as i32 }
}

/// An event object as a MIDI message (None for a TargetEvent without a destination).
fn to_msg(it: &mut Interp, v: &Value) -> Option<Msg> {
    let k = kind_of(it, v)?;
    let ch = (clampi(prop(it, v, "channel"), 1, 16) - 1) as u8;
    let p7 = |it: &mut Interp, n: &str| clampi(prop(it, v, n), 0, 127) as u8;
    Some(match k {
        EKind::NoteOn => {
            let pitch = p7(it, "pitch");
            let vel = p7(it, "velocity");
            // a NoteOn with velocity 0 is a note off
            if vel == 0 { Msg::Off { ch, pitch } } else { Msg::On { ch, pitch, vel } }
        }
        EKind::NoteOff => Msg::Off { ch, pitch: p7(it, "pitch") },
        EKind::PolyPressure => Msg::PolyAt { ch, pitch: p7(it, "pitch"), val: p7(it, "value") },
        EKind::ControlChange => Msg::Cc { ch, num: p7(it, "number"), val: p7(it, "value") },
        EKind::ProgramChange => Msg::Program { ch, num: p7(it, "number") },
        EKind::ChannelPressure => Msg::Pressure { ch, val: p7(it, "value") },
        EKind::PitchBend => Msg::Bend { ch, val: clampi(prop(it, v, "value"), -8192, 8191) as i16 },
        EKind::TargetEvent => {
            // the target parameter's menu picks a MIDI CC
            let t = it.get(v, "target").unwrap_or_default();
            let name = it.to_string(&t).unwrap_or_default();
            let h = host(it);
            let idx = h.params.iter().position(|p| p.kind == PKind::Target && p.name == name)?;
            let num = h.values.get(idx).copied().unwrap_or(0.0).round().clamp(0.0, 127.0) as u8;
            let val = (prop(it, v, "value").clamp(0.0, 1.0) * 127.0).round() as u8;
            Msg::Cc { ch, num, val }
        }
    })
}

/// A message as a new event object of its type (beatPos set when timing info is on).
fn from_msg(it: &mut Interp, ev: &Ev) -> Value {
    let (k, fields): (usize, Vec<(&str, f64)>) = match ev.msg {
        Msg::On { pitch, vel, .. } => (0, vec![("pitch", pitch as f64), ("velocity", vel as f64)]),
        Msg::Off { pitch, .. } => (1, vec![("pitch", pitch as f64), ("velocity", 64.0)]),
        Msg::PolyAt { pitch, val, .. } => (2, vec![("pitch", pitch as f64), ("value", val as f64)]),
        Msg::Cc { num, val, .. } => (3, vec![("number", num as f64), ("value", val as f64)]),
        Msg::Program { num, .. } => (4, vec![("number", num as f64)]),
        Msg::Pressure { val, .. } => (5, vec![("value", val as f64)]),
        Msg::Bend { val, .. } => (6, vec![("value", val as f64)]),
    };
    let proto = host(it).protos[k].clone();
    let o = obj(Some(proto), Kind::Plain);
    {
        let mut b = o.borrow_mut();
        for (n, x) in fields {
            b.set_own(n, Value::Num(x));
        }
        b.set_own("channel", Value::Num(ev.msg.ch() as f64 + 1.0));
        b.set_own("articulationID", Value::Num(0.0));
        if k == 0 {
            b.set_own("detune", Value::Num(0.0));
        }
    }
    let needs = host(it).needs_timing;
    o.borrow_mut().set_own("beatPos", Value::Num(if needs { beat_of(ev.tick) } else { 0.0 }));
    Value::Obj(o)
}

/// Queues an event object at a song tick.
fn send_at(it: &mut Interp, ev: &Value, tick: f64) {
    let Some(m) = to_msg(it, ev) else {
        it.console.push("TargetEvent: its target names no target parameter, nothing sent".into());
        return;
    };
    // NoteOn detune (cents): a pitch bend on the note's channel just before it (±2 semitones)
    if let Msg::On { ch, .. } = m {
        let d = prop(it, ev, "detune");
        if d.is_finite() && d != 0.0 {
            let b = (d / 200.0 * 8192.0).round().clamp(-8192.0, 8191.0) as i16;
            host(it).out.push((tick, Msg::Bend { ch, val: b }));
        }
    }
    host(it).out.push((tick, m));
}

/// The tick an event goes at when sent now: its beatPos while timing info is on.
fn event_tick(it: &mut Interp, ev: &Value) -> f64 {
    let bp = prop(it, ev, "beatPos");
    let h = host(it);
    if h.needs_timing && bp.is_finite() && bp > 0.0 {
        return tick_of(bp);
    }
    let now = h.now;
    if !h.needs_timing && bp.is_finite() && bp != 0.0 && !h.warned_beatpos {
        h.warned_beatpos = true;
        it.console.push("Warning: beatPos is ignored unless NeedsTimingInfo is true".into());
    }
    now
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

fn event_string(it: &mut Interp, v: &Value) -> String {
    let Some(k) = kind_of(it, v) else { return "[Event]".into() };
    let n = |it: &mut Interp, s: &str| prop(it, v, s);
    let ch = n(it, "channel");
    let mut s = format!("[{} channel:{}", KINDS[k as usize].1, crate::js::num_to_string(ch));
    match k {
        EKind::NoteOn | EKind::NoteOff => {
            let p = n(it, "pitch");
            let _ = std::fmt::Write::write_fmt(&mut s, format_args!(" pitch:{} [{}] velocity:{}", crate::js::num_to_string(p), note_name(p.clamp(0.0, 127.0) as u8), crate::js::num_to_string(n(it, "velocity"))));
        }
        EKind::PolyPressure => s += &format!(" pitch:{} value:{}", crate::js::num_to_string(n(it, "pitch")), crate::js::num_to_string(n(it, "value"))),
        EKind::ControlChange => {
            let num = n(it, "number");
            let label = cc_label(num.clamp(0.0, 127.0) as u8);
            s += &format!(" number:{}{} value:{}", crate::js::num_to_string(num), if label.is_empty() { String::new() } else { format!(" [{label}]") }, crate::js::num_to_string(n(it, "value")));
        }
        EKind::ProgramChange => s += &format!(" number:{}", crate::js::num_to_string(n(it, "number"))),
        EKind::ChannelPressure | EKind::PitchBend => s += &format!(" value:{}", crate::js::num_to_string(n(it, "value"))),
        EKind::TargetEvent => {
            let t = it.get(v, "target").unwrap_or_default();
            let t = it.to_string(&t).unwrap_or_default();
            s += &format!(" target:{t} value:{}", crate::js::num_to_string(n(it, "value")));
        }
    }
    let bp = n(it, "beatPos");
    if bp != 0.0 {
        s += &format!(" beatPos:{}", crate::js::num_to_string((bp * 1000.0).round() / 1000.0));
    }
    s + "]"
}

/// Looks up a parameter by index or name.
fn param_index(it: &mut Interp, key: &Value) -> Option<usize> {
    match key {
        Value::Num(n) => {
            let i = *n as usize;
            (*n >= 0.0 && i < host(it).params.len()).then_some(i)
        }
        _ => {
            let name = it.to_string(key).ok()?;
            host(it).params.iter().position(|p| p.name == name)
        }
    }
}

fn install_api(it: &mut Interp) {
    // Event and its types
    let ep = obj(Some(it.object_proto.clone()), Kind::Plain);
    let method = |it: &Interp, name: &str, f: fn(&mut Interp, &Value, &[Value]) -> crate::js::Res| {
        let v = it.native(f);
        ep.borrow_mut().set_own(name, v);
    };
    method(it, "send", |it, this, _| {
        let t = event_tick(it, this);
        send_at(it, this, t);
        Ok(Value::Undef)
    });
    method(it, "sendAfterMilliseconds", |it, this, a| {
        let ms = it.to_num(&arg(a, 0)).max(0.0);
        let base = event_tick(it, this);
        let tps = host(it).ctx.ticks_per_sec();
        send_at(it, this, base + ms / 1000.0 * tps);
        Ok(Value::Undef)
    });
    method(it, "sendAtBeat", |it, this, a| {
        let b = it.to_num(&arg(a, 0));
        if b.is_finite() {
            send_at(it, this, tick_of(b));
        }
        Ok(Value::Undef)
    });
    method(it, "sendAfterBeats", |it, this, a| {
        let b = it.to_num(&arg(a, 0)).max(0.0);
        let base = event_tick(it, this);
        send_at(it, this, base + b * PPQ);
        Ok(Value::Undef)
    });
    method(it, "trace", |it, this, _| {
        let s = event_string(it, this);
        it.console.push(s);
        Ok(Value::Undef)
    });
    method(it, "toString", |it, this, _| Ok(Value::Str(Rc::from(event_string(it, this)))));
    let ec = it.native_ctor(ep.clone(), |_, this, _| Ok(this.clone()));
    it.set_global("Event", ec);
    let mut protos = Vec::new();
    for (k, name) in KINDS {
        let proto = obj(Some(ep.clone()), Kind::Plain);
        protos.push(proto.clone());
        let c = it.native_ctor(proto, move |it, this, a| {
            if !matches!(this, Value::Obj(_)) {
                return it.type_error(&format!("{name} must be called with new"));
            }
            let defaults: &[(&str, f64)] = match k {
                EKind::NoteOn => &[("pitch", 60.0), ("velocity", 100.0), ("detune", 0.0)],
                EKind::NoteOff => &[("pitch", 60.0), ("velocity", 64.0)],
                EKind::PolyPressure => &[("pitch", 60.0), ("value", 0.0)],
                EKind::ControlChange => &[("number", 1.0), ("value", 0.0)],
                EKind::ProgramChange => &[("number", 0.0)],
                EKind::ChannelPressure | EKind::PitchBend | EKind::TargetEvent => &[("value", 0.0)],
            };
            for (n, x) in defaults {
                it.put(this, n, Value::Num(*x))?;
            }
            it.put(this, "channel", Value::Num(1.0))?;
            it.put(this, "articulationID", Value::Num(0.0))?;
            it.put(this, "beatPos", Value::Num(0.0))?;
            if k == EKind::TargetEvent {
                it.put(this, "target", Value::str(""))?;
            }
            // new NoteOn(event) copies the event's properties; another type's beatPos isn't
            let src = arg(a, 0);
            if let Some(sk) = kind_of(it, &src) {
                for key in it.keys(&src) {
                    if &*key == "beatPos" && sk != k {
                        continue;
                    }
                    if matches!(&*key, "pitch" | "velocity" | "number" | "value" | "channel" | "articulationID" | "beatPos" | "detune" | "target") {
                        let x = it.get(&src, &key)?;
                        it.put(this, &key, x)?;
                    }
                }
            }
            Ok(this.clone())
        });
        it.set_global(name, c);
    }
    {
        let h = host(it);
        h.protos = protos;
        h.event_proto = Some(ep);
    }

    // Trace, timing, parameters
    let f = it.native(|it, _, a| {
        let s = a.iter().map(|x| it.show(x)).collect::<Vec<_>>().join(" ");
        it.console.push(s);
        Ok(Value::Undef)
    });
    it.set_global("Trace", f);
    let f = it.native(|it, _, _| {
        if !host(it).needs_timing {
            if !host(it).warned_timing {
                host(it).warned_timing = true;
                it.console.push("GetTimingInfo needs var NeedsTimingInfo = true; at the script's global level".into());
            }
            return Ok(Value::Undef);
        }
        let (c, t) = {
            let h = host(it);
            (h.ctx, h.transport)
        };
        let o = it.new_object();
        let set = |it: &mut Interp, k: &str, v: Value| it.put(&o, k, v);
        set(it, "playing", Value::Bool(c.playing))?;
        set(it, "blockStartBeat", Value::Num(beat_of(c.from)))?;
        set(it, "blockEndBeat", Value::Num(beat_of(c.to)))?;
        set(it, "blockLength", Value::Num((c.to - c.from) / PPQ))?;
        set(it, "tempo", Value::Num(c.bpm))?;
        set(it, "meterNumerator", Value::Num(t.meter.0 as f64))?;
        set(it, "meterDenominator", Value::Num(t.meter.1 as f64))?;
        set(it, "cycling", Value::Bool(t.cycling))?;
        set(it, "leftCycleBeat", Value::Num(beat_of(t.cycle.0)))?;
        set(it, "rightCycleBeat", Value::Num(beat_of(t.cycle.1)))?;
        Ok(o)
    });
    it.set_global("GetTimingInfo", f);
    let f = it.native(|it, _, a| {
        let Some(i) = param_index(it, &arg(a, 0)) else { return Ok(Value::Undef) };
        Ok(Value::Num(host(it).values.get(i).copied().unwrap_or(0.0) as f64))
    });
    it.set_global("GetParameter", f);
    let f = it.native(|it, _, a| {
        let Some(i) = param_index(it, &arg(a, 0)) else { return Ok(Value::Undef) };
        let v = it.to_num(&arg(a, 1)) as f32;
        let h = host(it);
        let v = h.params[i].snap(v);
        if i < h.values.len() && h.values[i] != v {
            h.values[i] = v;
            h.set_serial += 1;
        }
        Ok(Value::Undef)
    });
    it.set_global("SetParameter", f);
    let f = it.native(|it, _, _| {
        match parse_params(it) {
            Ok(p) => {
                let h = host(it);
                h.params = p;
                h.params_changed = true;
            }
            Err(e) => it.console.push(e),
        }
        Ok(Value::Undef)
    });
    it.set_global("UpdatePluginParameters", f);

    // MIDI
    let m = it.new_object();
    let mo = m.as_obj().cloned().unwrap_or_else(|| obj(None, Kind::Plain));
    let put = |it: &Interp, name: &str, f: fn(&mut Interp, &Value, &[Value]) -> crate::js::Res| {
        let v = it.native(f);
        mo.borrow_mut().set_own(name, v);
    };
    put(it, "noteNumber", |it, _, a| {
        let s = it.to_string(&arg(a, 0))?;
        Ok(note_number(&s).map(|n| Value::Num(n as f64)).unwrap_or_default())
    });
    put(it, "noteName", |it, _, a| {
        let n = it.to_num(&arg(a, 0));
        if !(0.0..=127.0).contains(&n) {
            return Ok(Value::Undef);
        }
        Ok(Value::Str(Rc::from(note_name(n as u8))))
    });
    put(it, "ccName", |it, _, a| {
        let n = it.to_num(&arg(a, 0));
        if !(0.0..=127.0).contains(&n) {
            return Ok(Value::Undef);
        }
        let l = cc_label(n as u8);
        Ok(Value::Str(Rc::from(if l.is_empty() { format!("Controller {}", n as u8) } else { l.to_string() })))
    });
    put(it, "allNotesOff", |it, _, _| {
        let now = host(it).now;
        for ch in 0..16 {
            host(it).out.push((now, Msg::Cc { ch, num: 123, val: 0 }));
        }
        Ok(Value::Undef)
    });
    put(it, "normalizeStatus", |it, _, a| Ok(Value::Num(clampi(it.to_num(&arg(a, 0)), 128, 239) as f64)));
    put(it, "normalizeChannel", |it, _, a| Ok(Value::Num(clampi(it.to_num(&arg(a, 0)), 1, 16) as f64)));
    put(it, "normalizeData", |it, _, a| Ok(Value::Num(clampi(it.to_num(&arg(a, 0)), 0, 127) as f64)));
    it.set_global("MIDI", m);
}

/// "C3" → 60, "F#-1" → 18 (sharps only: a flat name is no note).
pub fn note_number(s: &str) -> Option<u8> {
    let s = s.trim();
    let mut c = s.chars();
    let letter = c.next()?.to_ascii_uppercase();
    let base = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = &s[1..];
    let (sharp, oct) = if let Some(r) = rest.strip_prefix('#') { (1, r) } else { (0, rest) };
    let o: i32 = oct.parse().ok()?;
    let n = (o + 2) * 12 + base + sharp;
    (0..=127).contains(&n).then_some(n as u8)
}

fn new_host() -> Host {
    Host { ctx: Ctx { from: 0.0, to: 0.0, bpm: 120.0, playing: false }, transport: Transport::default(), now: 0.0, out: Vec::new(), values: vec![0.0; MAX_PARAMS], params: Vec::new(), params_changed: false, set_serial: 0, needs_timing: false, protos: Vec::new(), event_proto: None, warned_timing: false, warned_beatpos: false }
}

fn new_interp() -> Interp {
    let mut it = Interp::new(Box::new(new_host()));
    install_api(&mut it);
    it
}

// ---------------------------------------------------------------------------------------------
// the unit

/// What the window shows of a running Scripter.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct View {
    pub console: Vec<String>,
    /// The parameters as the script last declared them (UpdatePluginParameters included).
    pub params: Vec<SParam>,
    /// The script's error (line, text), if running it failed.
    pub error: Option<(u32, String)>,
    /// The parameter values, and a count of the script's SetParameter calls (the window
    /// takes the values when it changes).
    pub values: Vec<f32>,
    pub set_serial: u64,
}

pub struct Scripter {
    it: Interp,
    script: String,
    pending: Vec<(f64, Msg)>,
    was_playing: bool,
    view: Arc<View>,
    console: Vec<String>,
    error: Option<(u32, String)>,
    dirty: bool,
}

// SAFETY: the interpreter's values are `Rc`s, but every one of them lives inside this unit (the
// interpreter, its scopes and host); nothing reference counted is handed out (the window gets
// plain copies through `View`), so moving the whole unit to the audio thread is sound.
unsafe impl Send for Scripter {}

impl Scripter {
    pub fn new(values: &[f32]) -> Scripter {
        let mut s = Scripter { it: new_interp(), script: String::new(), pending: Vec::new(), was_playing: false, view: Arc::new(View::default()), console: Vec::new(), error: None, dirty: true };
        s.load(values);
        s
    }

    fn print(&mut self, lines: Vec<String>) {
        if lines.is_empty() {
            return;
        }
        let n = lines.len();
        // too many lines at once: the console thins them out
        let keep = n.min(THIN_AT);
        self.console.extend(lines.into_iter().take(keep));
        if n > keep {
            self.console.push(format!("… {} more lines not shown (too much output at once)", n - keep));
        }
        if self.console.len() > CONSOLE_MAX {
            let cut = self.console.len() - CONSOLE_MAX;
            self.console.drain(..cut);
        }
        self.dirty = true;
    }

    fn flush_console(&mut self) {
        let lines = std::mem::take(&mut self.it.console);
        self.print(lines);
    }

    fn report(&mut self, e: &Abort) {
        let m = self.it.describe(e);
        let line = match e {
            Abort::Throw(v) => v.as_obj().and_then(|o| o.borrow().get_own("line")).map(|l| self.it.to_num(&l) as u32).unwrap_or(self.it.line),
            Abort::Halt(_) => self.it.line,
        };
        self.flush_console();
        self.print(vec![m.clone()]);
        self.error = Some((line, m));
    }

    /// Calls a script function (if defined) with a fresh budget; errors go to the console.
    fn call(&mut self, name: &str, args: &[Value]) -> Option<Value> {
        self.it.budget = crate::js::BUDGET;
        let r = self.it.call_global(name, args)?;
        match r {
            Ok(v) => {
                self.flush_console();
                Some(v)
            }
            Err(e) => {
                self.report(&e);
                None
            }
        }
    }

    /// Runs a new script (Run Script): fresh interpreter, parameters, ParameterChanged.
    fn run_script(&mut self, values: &[f32]) {
        let script = script_of(values);
        self.it = new_interp();
        host(&mut self.it).values = values.iter().take(MAX_PARAMS).copied().chain(std::iter::repeat(0.0)).take(MAX_PARAMS).collect();
        self.error = None;
        self.script = script.clone();
        self.it.budget = crate::js::BUDGET;
        match self.it.eval_source(&script) {
            Ok(_) => self.flush_console(),
            Err(e) => {
                self.report(&e);
            }
        }
        let needs = self.it.get_global("NeedsTimingInfo").truthy();
        host(&mut self.it).needs_timing = needs;
        match parse_params(&mut self.it) {
            Ok(p) => host(&mut self.it).params = p,
            Err(e) => {
                self.print(vec![e.clone()]);
                self.error = Some((0, e));
            }
        }
        // once per parameter, as when a setting loads
        let n = host(&mut self.it).params.len();
        for i in 0..n.min(MAX_PARAMS) {
            if host(&mut self.it).params[i].kind == PKind::Text {
                continue;
            }
            let v = host(&mut self.it).values[i] as f64;
            self.call("ParameterChanged", &[Value::Num(i as f64), Value::Num(v)]);
        }
        self.dirty = true;
    }

    fn take_out(&mut self) -> Vec<(f64, Msg)> {
        std::mem::take(&mut host(&mut self.it).out)
    }

    fn refresh_view(&mut self) {
        let h = host(&mut self.it);
        if h.params_changed {
            h.params_changed = false;
            self.dirty = true;
        }
        if self.dirty || h.set_serial != self.view.set_serial {
            let h = host(&mut self.it);
            self.view = Arc::new(View { console: self.console.clone(), params: h.params.clone(), error: self.error.clone(), values: h.values.clone(), set_serial: h.set_serial });
            self.dirty = false;
        }
    }
}

impl MidiFx for Scripter {
    fn load(&mut self, values: &[f32]) {
        if script_of(values) != self.script {
            self.run_script(values);
            self.refresh_view();
            return;
        }
        // a parameter moved: ParameterChanged(index, value)
        for i in 0..MAX_PARAMS {
            let v = values.get(i).copied().unwrap_or(0.0);
            let old = host(&mut self.it).values[i];
            if v != old {
                host(&mut self.it).values[i] = v;
                if i < host(&mut self.it).params.len() {
                    self.call("ParameterChanged", &[Value::Num(i as f64), Value::Num(v as f64)]);
                }
            }
        }
        let out = self.take_out();
        self.pending.extend(out);
        self.refresh_view();
    }

    fn process(&mut self, input: &[Ev], ctx: &Ctx, out: &mut Vec<Ev>) {
        {
            let t = TRANSPORT.lock().map(|g| *g).unwrap_or_default();
            let h = host(&mut self.it);
            h.ctx = *ctx;
            h.transport = t;
            h.now = ctx.from;
        }
        if ctx.playing && !self.was_playing {
            self.call("Reset", &[]);
        }
        self.was_playing = ctx.playing;
        let has_handle = self.it.get_global("HandleMIDI").is_callable();
        for ev in input {
            if !has_handle {
                // no HandleMIDI: events pass through unchanged
                out.push(*ev);
                continue;
            }
            host(&mut self.it).now = ev.tick;
            let e = from_msg(&mut self.it, ev);
            self.call("HandleMIDI", &[e]);
        }
        host(&mut self.it).now = ctx.from;
        self.call("ProcessMIDI", &[]);
        let sent = self.take_out();
        self.pending.extend(sent);
        // what's due in this block goes now (late events at the block start)
        let mut keep = Vec::new();
        for (t, m) in self.pending.drain(..) {
            if t < ctx.to {
                out.push(Ev::new(t.max(ctx.from), m));
            } else {
                keep.push((t, m));
            }
        }
        self.pending = keep;
        self.refresh_view();
    }

    fn reset(&mut self, out: &mut Vec<Ev>) {
        // queued note offs still go, so nothing hangs
        for (_, m) in self.pending.drain(..) {
            if let Msg::Off { .. } = m {
                out.push(Ev::new(0.0, m));
            }
        }
        self.was_playing = false;
    }

    fn bypassed(&mut self, out: &mut Vec<Ev>) {
        self.reset(out);
        self.call("Reset", &[]);
        self.take_out();
        self.refresh_view();
    }

    fn status(&self) -> Status {
        Status { script: Some(self.view.clone()), ..Status::default() }
    }

    fn command(&mut self, name: &str) {
        if name == "run" {
            let mut v = host(&mut self.it).values.clone();
            v.truncate(MAX_PARAMS);
            let vals = with_script(&v, &self.script.clone());
            self.run_script(&vals);
        } else if name == "clear" {
            self.console.clear();
            self.dirty = true;
        } else if let Some(code) = name.strip_prefix("eval:") {
            // the Interactive Console: run the line in the script's global scope
            self.it.budget = crate::js::BUDGET;
            match self.it.eval_source(code) {
                Ok(v) => {
                    self.flush_console();
                    if !v.is_undef() {
                        let s = self.it.show(&v);
                        self.print(vec![s]);
                    }
                }
                Err(e) => {
                    let m = self.it.describe(&e);
                    self.flush_console();
                    self.print(vec![m]);
                }
            }
            let out = self.take_out();
            self.pending.extend(out);
        }
        self.refresh_view();
    }
}

// ---------------------------------------------------------------------------------------------
// the scripts that come with Scripter (our own)

/// Tutorial scripts: (name, script).
pub const TUTORIALS: &[(&str, &str)] = &[
    ("Tutorial Script 1: Pass Through", "// Every event goes on unchanged.\n\nfunction HandleMIDI(event)\n{\n\tevent.send();\n}\n"),
    ("Tutorial Script 2: Trace Events", "// Prints each event to the console and doesn't send it on.\n\nfunction HandleMIDI(event)\n{\n\tevent.trace();\n}\n"),
    ("Tutorial Script 3: Octave Echo", "// Sends each note, then the same note an octave up 100 ms later.\n\nfunction HandleMIDI(event)\n{\n\tevent.send();\n\tif (event instanceof NoteOn || event instanceof NoteOff) {\n\t\tevent.pitch += 12;\n\t\tevent.sendAfterMilliseconds(100);\n\t}\n}\n"),
    ("Tutorial Script 4: Detune", "// Detunes each note on by the Detune slider.\n\nvar PluginParameters = [{name:\"Detune\", type:\"lin\", unit:\"cents\", minValue:-100, maxValue:100, numberOfSteps:200, defaultValue:0}];\n\nfunction HandleMIDI(event)\n{\n\tif (event instanceof NoteOn)\n\t\tevent.detune = GetParameter(\"Detune\");\n\tevent.send();\n}\n"),
    ("Tutorial Script 5: Print Tempo", "// Prints the tempo to the console while the transport runs.\n\nvar NeedsTimingInfo = true;\nvar last = -1;\n\nfunction ProcessMIDI()\n{\n\tvar info = GetTimingInfo();\n\tif (info.playing && Math.floor(info.blockStartBeat) != last) {\n\t\tlast = Math.floor(info.blockStartBeat);\n\t\tTrace(\"tempo: \" + info.tempo);\n\t}\n}\n"),
    ("Tutorial Script 6: Velocity Slider", "// Every note plays at the Velocity slider's value.\n\nvar PluginParameters = [{name:\"Velocity\", type:\"lin\", minValue:1, maxValue:127, numberOfSteps:126, defaultValue:80}];\n\nfunction HandleMIDI(event)\n{\n\tif (event instanceof NoteOn)\n\t\tevent.velocity = GetParameter(\"Velocity\");\n\tevent.send();\n}\n"),
    ("Tutorial Script 7: Parameter Changes", "// Prints each parameter change to the console.\n\nvar PluginParameters = [\n\t{name:\"Amount\", type:\"lin\", minValue:0, maxValue:100, numberOfSteps:100, defaultValue:50},\n\t{name:\"Mode\", type:\"menu\", valueStrings:[\"Soft\", \"Medium\", \"Hard\"], defaultValue:1}\n];\n\nfunction ParameterChanged(param, value)\n{\n\tTrace(PluginParameters[param].name + \" changed to \" + value);\n}\n\nfunction HandleMIDI(event)\n{\n\tevent.send();\n}\n"),
    ("Tutorial Script 8: Event Creation", "// Each note on also plays a fifth above, made as new events.\n\nfunction HandleMIDI(event)\n{\n\tevent.send();\n\tif (event instanceof NoteOn) {\n\t\tvar fifth = new NoteOn(event);\n\t\tfifth.pitch += 7;\n\t\tfifth.velocity = Math.round(event.velocity * 0.8);\n\t\tfifth.send();\n\t} else if (event instanceof NoteOff) {\n\t\tvar off = new NoteOff(event);\n\t\toff.pitch += 7;\n\t\toff.send();\n\t}\n}\n"),
    ("Tutorial Script 9: Event Modification", "// Changes events on the way through: notes down an octave, the mod wheel inverted.\n\nfunction HandleMIDI(event)\n{\n\tif (event instanceof NoteOn || event instanceof NoteOff)\n\t\tevent.pitch = MIDI.normalizeData(event.pitch - 12);\n\telse if (event instanceof ControlChange && event.number == 1)\n\t\tevent.value = 127 - event.value;\n\tevent.send();\n}\n"),
    ("Tutorial Script 10: Beat Position", "// Prints the beat position of each block to the console while playing.\n\nvar NeedsTimingInfo = true;\n\nfunction ProcessMIDI()\n{\n\tvar info = GetTimingInfo();\n\tif (info.playing)\n\t\tTrace(\"beat \" + info.blockStartBeat.toFixed(2));\n}\n"),
    ("Tutorial Script 11: Event Detection", "// Controller 20 sends all notes off; everything else goes through.\n\nfunction HandleMIDI(event)\n{\n\tif (event instanceof ControlChange && event.number == 20) {\n\t\tMIDI.allNotesOff();\n\t\treturn;\n\t}\n\tevent.send();\n}\n"),
    ("Tutorial Script 12: Note Delay in Beats", "// Delays every event by the Delay slider, in beats.\n\nvar NeedsTimingInfo = true;\nvar PluginParameters = [{name:\"Delay\", type:\"lin\", unit:\"beats\", minValue:0, maxValue:4, numberOfSteps:16, defaultValue:1}];\n\nfunction HandleMIDI(event)\n{\n\tevent.sendAfterBeats(GetParameter(\"Delay\"));\n}\n"),
    ("Tutorial Script 13: Note Counter", "// Counts the notes played; Reset() sets the count back when the transport starts.\n\nvar count = 0;\n\nfunction Reset()\n{\n\tcount = 0;\n}\n\nfunction HandleMIDI(event)\n{\n\tif (event instanceof NoteOn) {\n\t\tcount++;\n\t\tTrace(\"note \" + count + \": \" + MIDI.noteName(event.pitch));\n\t}\n\tevent.send();\n}\n"),
    ("Tutorial Script 14: Hiding Controls", "// The Depth slider shows only when Effect is on (UpdatePluginParameters).\n\nvar PluginParameters = [\n\t{name:\"Effect\", type:\"checkbox\", defaultValue:0},\n\t{name:\"Depth\", type:\"lin\", minValue:0, maxValue:24, numberOfSteps:24, defaultValue:12, hidden:true}\n];\n\nfunction ParameterChanged(param, value)\n{\n\tif (param == 0) {\n\t\tPluginParameters[1].hidden = value == 0;\n\t\tUpdatePluginParameters();\n\t}\n}\n\nfunction HandleMIDI(event)\n{\n\tif (GetParameter(\"Effect\") && (event instanceof NoteOn || event instanceof NoteOff))\n\t\tevent.pitch += GetParameter(\"Depth\");\n\tevent.send();\n}\n"),
    ("Tutorial Script 15: Controlling Plug-ins", "// Each note's velocity drives a target: pick the destination in the Target menu.\n\nvar PluginParameters = [{name:\"Target\", type:\"target\"}];\n\nfunction HandleMIDI(event)\n{\n\tif (event instanceof NoteOn) {\n\t\tvar t = new TargetEvent();\n\t\tt.target = \"Target\";\n\t\tt.value = event.velocity / 127;\n\t\tt.send();\n\t}\n\tevent.send();\n}\n"),
    ("Slider Creation", "// A slider for each kind of control.\n\nvar PluginParameters = [\n\t{name:\"Sliders\", type:\"text\"},\n\t{name:\"Basic Slider\"},\n\t{name:\"Linear Slider\", type:\"lin\", minValue:0, maxValue:100, numberOfSteps:100, defaultValue:25},\n\t{name:\"Log Slider\", type:\"log\", unit:\"Hz\", minValue:20, maxValue:20000, numberOfSteps:1000, defaultValue:1000},\n\t{name:\"Buttons\", type:\"text\"},\n\t{name:\"Momentary\", type:\"momentary\"},\n\t{name:\"Checkbox\", type:\"checkbox\", defaultValue:1}\n];\n\nfunction ParameterChanged(param, value)\n{\n\tTrace(PluginParameters[param].name + \": \" + value);\n}\n"),
    ("Slider Ranges", "// Sliders with their own ranges, steps and units.\n\nvar PluginParameters = [\n\t{name:\"Semitones\", type:\"lin\", unit:\"st\", minValue:-24, maxValue:24, numberOfSteps:48, defaultValue:0},\n\t{name:\"Percent\", type:\"lin\", unit:\"%\", minValue:0, maxValue:100, numberOfSteps:100, defaultValue:50},\n\t{name:\"Fine\", type:\"lin\", minValue:0, maxValue:1, numberOfSteps:1000, defaultValue:0.5}\n];\n\nfunction ParameterChanged(param, value)\n{\n\tTrace(PluginParameters[param].name + \" = \" + value);\n}\n"),
    ("Menu Creation", "// A menu: its value is the index of the chosen item.\n\nvar PluginParameters = [{name:\"Direction\", type:\"menu\", valueStrings:[\"Up\", \"Down\", \"Off\"], defaultValue:0}];\n\nfunction HandleMIDI(event)\n{\n\tvar d = GetParameter(\"Direction\");\n\tif (event instanceof NoteOn || event instanceof NoteOff)\n\t\tevent.pitch += d == 0 ? 12 : d == 1 ? -12 : 0;\n\tevent.send();\n}\n"),
];

/// Ready-made script processors: (name, script).
pub const FACTORY: &[(&str, &str)] = &[
    ("Octave Doubler", "// Doubles every note an octave up or down.\n\nvar PluginParameters = [{name:\"Octave\", type:\"menu\", valueStrings:[\"-2\", \"-1\", \"+1\", \"+2\"], defaultValue:2}];\nvar SHIFT = [-24, -12, 12, 24];\n\nfunction HandleMIDI(event)\n{\n\tevent.send();\n\tif (event instanceof NoteOn || event instanceof NoteOff) {\n\t\tevent.pitch = MIDI.normalizeData(event.pitch + SHIFT[GetParameter(\"Octave\")]);\n\t\tevent.send();\n\t}\n}\n"),
    ("Velocity Limiter", "// Keeps note velocities between Low and High.\n\nvar PluginParameters = [\n\t{name:\"Low\", type:\"lin\", minValue:1, maxValue:127, numberOfSteps:126, defaultValue:1},\n\t{name:\"High\", type:\"lin\", minValue:1, maxValue:127, numberOfSteps:126, defaultValue:110}\n];\n\nfunction HandleMIDI(event)\n{\n\tif (event instanceof NoteOn)\n\t\tevent.velocity = Math.min(Math.max(event.velocity, GetParameter(\"Low\")), GetParameter(\"High\"));\n\tevent.send();\n}\n"),
    ("Random Velocity", "// Varies each note's velocity at random by up to Amount.\n\nvar PluginParameters = [{name:\"Amount\", type:\"lin\", minValue:0, maxValue:64, numberOfSteps:64, defaultValue:16}];\n\nfunction HandleMIDI(event)\n{\n\tif (event instanceof NoteOn) {\n\t\tvar a = GetParameter(\"Amount\");\n\t\tevent.velocity = MIDI.normalizeData(event.velocity + Math.round((Math.random() * 2 - 1) * a));\n\t\tif (event.velocity < 1) event.velocity = 1;\n\t}\n\tevent.send();\n}\n"),
    ("Channel Router", "// Sends every event on one MIDI channel.\n\nvar PluginParameters = [{name:\"Channel\", type:\"lin\", minValue:1, maxValue:16, numberOfSteps:15, defaultValue:1}];\n\nfunction HandleMIDI(event)\n{\n\tevent.channel = GetParameter(\"Channel\");\n\tevent.send();\n}\n"),
    ("Simple Arpeggiator", "// Plays the held notes one after another, upward, in 1/16 notes.\n\nvar NeedsTimingInfo = true;\nvar held = [];\nvar step = 0;\nvar PluginParameters = [{name:\"Rate\", type:\"menu\", valueStrings:[\"1/4\", \"1/8\", \"1/16\"], defaultValue:2}];\nvar DIV = [1, 0.5, 0.25];\n\nfunction HandleMIDI(event)\n{\n\tif (event instanceof NoteOn) {\n\t\theld.push(event.pitch);\n\t\theld.sort(function (a, b) { return a - b; });\n\t} else if (event instanceof NoteOff) {\n\t\tvar i = held.indexOf(event.pitch);\n\t\tif (i >= 0) held.splice(i, 1);\n\t} else {\n\t\tevent.send();\n\t}\n}\n\nfunction Reset()\n{\n\theld = [];\n\tstep = 0;\n}\n\nfunction ProcessMIDI()\n{\n\tvar info = GetTimingInfo();\n\tif (!info.playing || held.length == 0) return;\n\tvar div = DIV[GetParameter(\"Rate\")];\n\tvar next = Math.ceil((info.blockStartBeat - 1) / div) * div + 1;\n\twhile (next < info.blockEndBeat) {\n\t\tvar on = new NoteOn;\n\t\ton.pitch = held[step % held.length];\n\t\ton.velocity = 100;\n\t\ton.sendAtBeat(next);\n\t\tvar off = new NoteOff(on);\n\t\toff.sendAtBeat(next + div * 0.9);\n\t\tstep++;\n\t\tnext += div;\n\t}\n}\n"),
    ("Note Filter", "// Lets through only notes between Lowest and Highest.\n\nvar PluginParameters = [\n\t{name:\"Lowest\", type:\"lin\", minValue:0, maxValue:127, numberOfSteps:127, defaultValue:36},\n\t{name:\"Highest\", type:\"lin\", minValue:0, maxValue:127, numberOfSteps:127, defaultValue:96}\n];\n\nfunction HandleMIDI(event)\n{\n\tif ((event instanceof NoteOn || event instanceof NoteOff) && (event.pitch < GetParameter(\"Lowest\") || event.pitch > GetParameter(\"Highest\")))\n\t\treturn;\n\tevent.send();\n}\n"),
];

#[cfg(test)]
#[path = "scripter_tests.rs"]
mod tests;
