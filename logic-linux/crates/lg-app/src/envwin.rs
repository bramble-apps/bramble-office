//! The MIDI Environment window (Window ▸ Open MIDI Environment, ⌘0; lg_core::environment does
//! the MIDI): the Layer pop-up and Object inspector on the left, the New / Edit / Options / View
//! menus and the tools on top, the objects and their cables on the layer. MIDI from every input
//! port enters at the Physical Input and reaches the selected track only through the Sequencer
//! Input; objects send to MIDI output ports, tracks' channel strips, and each other.
//!
//! Each project keeps its Environment beside it (~/.config/bramble-studio/environments).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke};
use lg_core::environment::{self as envm, Cable, Clock, Env, Ev, Id, Kind, Obj, Out, Widget};

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Open Environment…", "Open MIDI Environment", "Show/Hide Environment", "Open Environment Mixer…",
    "New MIDI Instrument", "New Multi-Instrument", "New Mapped Instrument", "New Fader/Knobs/…", "New Keyboard", "New Monitor", "New Arpeggiator", "New Transformer", "New Delay Line", "New Voice Limiter", "New Channel Splitter", "New Chord Memorizer",
    "Clear Cables only", "Clean Up Positions", "Reset Sizes", "Align Objects", "Object move left", "Object move right", "Object move up", "Object move down", "Object Width -1 Pixel", "Object Width +1 Pixel", "Object Height -1 Pixel", "Object Height +1 Pixel",
    "Show/Hide Cables", "Protect Cabling/Positions", "Define Custom Bank Messages…", "Select Used Instruments", "Select Unused Instruments", "Select Cable Destination", "Select Cable Origin",
    "Select Next Object Above", "Select Next Object Below", "Select Next Object Left", "Select Next Object Right", "Go to Layer of Object", "Go to Previous Layer", "Send Selected Fader Values", "Send All Current Fader Values", "Delete Layer", "Create Layer", "Rename Layer…", "Cable serially",
    "Apply Buffer Template to Position and Size", "Apply Buffer Template to Position", "Apply Buffer Template to Size", "Apply Buffer Template to Definition", "Apply Buffer Template to Definition, channel increment", "Apply Buffer Template to Definition, number increment", "Apply Buffer Template to Cables",
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tool {
    Pointer,
    Pencil,
    Eraser,
    Text,
    MidiThru,
}

#[derive(Clone, Debug, PartialEq)]
enum Drag {
    None,
    Move { start: Pos2, orig: Vec<(Id, f32, f32)>, copy: bool },
    Resize { id: Id, start: Pos2, w: f32, h: f32 },
    Cable { from: Id, outlet: u16, at: Pos2 },
    /// A selected cable dragged to a new destination (several: "connect all?").
    Recable { cables: Vec<Cable>, at: Pos2 },
    Band { start: Pos2, at: Pos2 },
    Operate { id: Id, start: Pos2, v0: i32, v1: i32 },
}

/// One Environment window (Logic opens another when one is open).
#[derive(Clone, Debug)]
pub struct Win {
    pub open: bool,
    pub layer: usize,
    prev_layer: usize,
    pub sel: Vec<Id>,
    pub sel_cables: Vec<Cable>,
    pub tool: Tool,
    pub by_text: bool,
    pub inspector: bool,
    pub frameless: bool,
    scroll: egui::Vec2,
    drag: Drag,
    rename: Option<(Id, String)>,
    canvas: Rect,
}

impl Default for Win {
    fn default() -> Win {
        Win { open: false, layer: 2, prev_layer: 2, sel: Vec::new(), sel_cables: Vec::new(), tool: Tool::Pointer, by_text: false, inspector: true, frameless: false, scroll: egui::Vec2::ZERO, drag: Drag::None, rename: None, canvas: Rect::NOTHING }
    }
}

/// Object windows opened by double-clicking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sub {
    Chord,
    Mapped,
    Multi,
    Touch,
    Transformer,
    TextFader,
    Banks,
    SysEx,
}

/// Confirmations shown over the Environment.
#[derive(Clone, Debug)]
enum Dlg {
    DeleteLayer(usize),
    RenameLayer(usize, String),
    /// A cable onto an object with a direct output: Cancel, No, Remove.
    ReplacePort { from: Id, outlet: u16, to: Id },
    /// Several selected cables dragged to one destination.
    ConnectAll { cables: Vec<Cable>, to: Id },
    PasteReplace,
    ImportCustom(Box<Env>),
}

/// A touch tracks region playing.
#[derive(Clone, Debug)]
struct TouchPlay {
    obj: Id,
    key: u8,
    track: usize,
    group: u8,
    /// (start ms from the play's start, end ms, pitch, velocity)
    notes: Vec<(f64, f64, u8, u8)>,
    start: f64,
    len: f64,
    looping: bool,
    gate: bool,
    sounding: Vec<(f64, u8)>,
    next: usize,
}

pub struct State {
    pub env: Env,
    pub wins: [Win; 2],
    front: usize,
    undo: Vec<Env>,
    redo: Vec<Env>,
    clip: Option<Env>,
    pub cables_shown: bool,
    pub protect: bool,
    pub colored: bool,
    pub snap: bool,
    /// Tracks reassigned to objects (Reassign Track, the MIDI Thru tool).
    pub track_obj: BTreeMap<usize, Id>,
    t0: Instant,
    pub sub: Option<(Sub, Id)>,
    sub_sel: Vec<u8>,
    listen: bool,
    dlg: Option<Dlg>,
    pub hovered: bool,
    plays: Vec<TouchPlay>,
    /// The subchannel of a multi-instrument selected (inspector).
    pub subch: Option<(Id, u8)>,
}

impl Default for State {
    fn default() -> State {
        State { env: Env::new(), wins: [Win::default(), Win::default()], front: 0, undo: Vec::new(), redo: Vec::new(), clip: None, cables_shown: true, protect: false, colored: true, snap: true, track_obj: BTreeMap::new(), t0: Instant::now(), sub: None, sub_sel: Vec::new(), listen: false, dlg: None, hovered: false, plays: Vec::new(), subch: None }
    }
}

const GRID: f32 = 8.0;

pub(crate) fn clock(app: &App) -> Clock {
    Clock { ms: app.hub.env.t0.elapsed().as_secs_f64() * 1000.0, bpm: app.tempo(), playing: app.playing(), song: app.position() }
}

fn file_for(app: &App) -> Option<PathBuf> {
    let project = app.project.as_ref().map(|p| p.path.clone()).filter(|p| !app.demo && p.exists())?;
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let id: String = project.canonicalize().unwrap_or(project).display().to_string().chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    Some(base.join("bramble-studio").join("environments").join(format!("{id}.txt")))
}

/// A project opened: its Environment (a new one as Logic makes it when it has none).
pub fn after_open(app: &mut App) {
    let text = file_for(app).and_then(|f| std::fs::read_to_string(f).ok());
    let s = &mut app.hub.env;
    s.track_obj.clear();
    s.undo.clear();
    s.redo.clear();
    s.env = text.as_deref().and_then(Env::from_text).unwrap_or_default();
    if let Some(t) = text {
        for l in t.lines() {
            if let Some(r) = l.strip_prefix("track\t") {
                if let Some((a, b)) = r.split_once('\t') {
                    if let (Ok(a), Ok(b)) = (a.parse(), b.parse()) {
                        s.track_obj.insert(a, b);
                    }
                }
            }
        }
    }
    sync_strips(app);
}

/// The project saved: its Environment beside it.
pub fn save(app: &mut App) {
    if app.automated {
        return;
    }
    let Some(f) = file_for(app) else { return };
    let s = &app.hub.env;
    let mut t = s.env.to_text();
    for (k, v) in &s.track_obj {
        t += &format!("track\t{k}\t{v}\n");
    }
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, t);
}

/// Project Settings ▸ Audio ▸ Automatic management of channel strip objects: one channel strip
/// object per track on the Mixer layer, named after it.
pub fn sync_strips(app: &mut App) {
    if !app.proj.on("Audio / Channel Strips / Automatic management of channel strip objects") {
        return;
    }
    let tracks: Vec<(String, bool)> = app.project.as_ref().map(|p| p.tracks.iter().map(|t| (t.name.clone(), t.kind == lg_core::TrackKind::Audio)).collect()).unwrap_or_default();
    let env = &mut app.hub.env.env;
    let mixer = env.layers.iter().position(|l| l == "Mixer").unwrap_or_else(|| {
        env.layers.push("Mixer".into());
        env.layers.len() - 1
    });
    let naming = app.proj.on("Audio / Channel Strips / Automatic naming of channel strip objects");
    let stale: Vec<Id> = env.objects.iter().filter(|o| o.kind == Kind::ChannelStrip && o.p("trk") as usize >= tracks.len()).map(|o| o.id).collect();
    env.delete(&stale);
    for (i, (name, audio)) in tracks.iter().enumerate() {
        let id = match env.objects.iter().find(|o| o.kind == Kind::ChannelStrip && o.p("trk") == i as i32) {
            Some(o) => o.id,
            None => {
                let id = env.add(Kind::ChannelStrip, mixer, 20.0 + i as f32 * 66.0, 20.0);
                env.obj_mut(id).unwrap().set("trk", i as i32);
                id
            }
        };
        let o = env.obj_mut(id).unwrap();
        o.set("type", if *audio { 0 } else { 3 });
        if naming {
            o.name = name.clone();
        }
    }
}

pub(crate) fn checkpoint(app: &mut App) {
    let s = &mut app.hub.env;
    s.undo.push(s.env.clone());
    if s.undo.len() > 100 {
        s.undo.remove(0);
    }
    s.redo.clear();
    app.dirty = true;
}

// ---- MIDI ----

/// A message from input port `port` into the Physical Input.
pub fn physical_in(app: &mut App, ctx: &egui::Context, port: usize, status: u8, a: u8, b: u8) {
    let Some(ev) = Ev::from_bytes(&[status, a, b]) else { return };
    let c = clock(app);
    let port = if port == usize::MAX { 63 } else { port };
    let outs = app.hub.env.env.input(port, ev, c);
    dispatch(app, ctx, outs);
}

/// Each frame: delay lines, arpeggios, touch tracks.
pub fn tick(app: &mut App, ctx: &egui::Context) {
    let c = clock(app);
    let outs = app.hub.env.env.tick(c);
    dispatch(app, ctx, outs);
    touch_frame(app, c.ms);
    if app.hub.env.env.busy() || !app.hub.env.plays.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_millis(4));
    }
}

/// What leaves the Environment goes to the sequencer, ports, tracks.
pub fn dispatch(app: &mut App, ctx: &egui::Context, outs: Vec<Out>) {
    for o in outs {
        match o {
            Out::Seq { ev, channelize: _ } => {
                let Some(b) = ev.bytes() else { continue };
                // (a track reassigned to an object plays through it)
                if let Some(&obj) = app.selected_track.and_then(|t| app.hub.env.track_obj.get(&t)) {
                    if app.hub.env.env.obj(obj).is_some_and(|o| o.kind != Kind::ChannelStrip) {
                        let c = clock(app);
                        let more = app.hub.env.env.deliver(obj, ev, c);
                        dispatch(app, ctx, more);
                        continue;
                    }
                }
                let st = if ev.st == 0x80 { 0x80 } else { ev.st | ev.ch };
                crate::live::sequencer_in(app, ctx, st, b[1], b.get(2).copied().unwrap_or(0));
            }
            Out::Port { port, ev } => {
                if let Some(b) = ev.bytes() {
                    app.hub.outs.send(&port, &b);
                }
            }
            Out::Track { track, ev } => {
                if let Ok(mut e) = app.engine.lock() {
                    match ev.st {
                        0x90 => e.live(track, ev.d1, ev.d2),
                        0x80 => e.live(track, ev.d1, 0),
                        0xB0 => e.live_control(track, ev.d1, ev.d2 as u16),
                        0xE0 => e.live_control(track, 128, ((ev.d2 as u16) << 7) | ev.d1 as u16),
                        _ => {}
                    }
                }
            }
            Out::Meta { num, val } => match num {
                49 => {
                    let _ = app.key_command(ctx, &format!("Recall Screenset {}", val.clamp(1, 9)));
                }
                51 => {
                    let _ = app.key_command(ctx, &format!("Go to Marker Number {}", val.max(1)));
                }
                52 => app.halt(),
                // (the tempo fader: ignored while following external time code)
                100 if !app.hub.sync.ext => app.set_tempo(val as f64 + 50.0),
                _ => {}
            },
            Out::Touch { obj, ev } => touch(app, obj, ev),
            Out::MmcArm { mask } => crate::syncmidi::send_arm(app, mask as u64),
            Out::Raw { port, bytes } => app.hub.outs.send(&port, &bytes),
        }
    }
}

/// Touch tracks: a note starts, stops or toggles its row's region.
fn touch(app: &mut App, obj: Id, ev: Ev) {
    let Some(o) = app.hub.env.env.obj(obj).cloned() else { return };
    let now = clock(app);
    let key = ev.d1;
    if ev.is_note_off() {
        // Gate: until the key is released
        for p in app.hub.env.plays.iter_mut().filter(|p| p.obj == obj && p.key == key && p.gate) {
            p.len = 0.0;
            p.looping = false;
        }
        return;
    }
    if !ev.is_note_on() {
        return;
    }
    // (a row with nothing set: the nearest row above it, transposed — Logic's grey line)
    let (row, shift) = match (0..=key).rev().find_map(|k| o.touch_row(k).map(|r| (r, key as i32 - k as i32))) {
        Some(x) => x,
        None => return,
    };
    let Some((t, r)) = row.region else { return };
    let Some(region) = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.get(r)).cloned() else { return };
    let trigger = row.trigger;
    let playing_same = app.hub.env.plays.iter().any(|p| p.obj == obj && p.key == key);
    // Toggle: a second press stops it; Single: stops and restarts
    if playing_same && (trigger == 4 || trigger == 5 || trigger == 1) {
        stop_plays(app, |p| p.obj == obj && p.key == key);
        if trigger != 1 {
            return;
        }
    }
    if row.group > 0 {
        let g = row.group;
        stop_plays(app, |p| p.obj == obj && p.group == g);
    }
    let mpt = now.ms_per_tick();
    let vel_scale = match row.velocity {
        0 => ev.d2 as f32 / 100.0,
        1 => 0.5 + ev.d2 as f32 / 254.0,
        _ => 1.0,
    };
    let notes: Vec<(f64, f64, u8, u8)> = region.notes.iter().map(|n| {
        let s = (n.pos.saturating_sub(region.start)) as f64 * mpt;
        let p = (n.pitch as i32 + row.transpose + shift).clamp(0, 127) as u8;
        (s, s + n.len as f64 * mpt, p, ((n.velocity as f32 * vel_scale).round() as i32).clamp(1, 127) as u8)
    }).collect();
    // Start: Free, or the next 1/16, 1/4, 1/1 while playing; then Delay
    let q = match row.start {
        1 => lg_core::events::PPQ as f64 / 4.0,
        2 => lg_core::events::PPQ as f64,
        3 => app.bar_ticks() as f64,
        _ => 0.0,
    };
    let mut start = now.ms;
    if q > 0.0 && now.playing {
        let next = (now.song / q).ceil() * q;
        start += (next - now.song) * mpt;
    }
    start += row.delay as f64 * mpt;
    app.hub.env.plays.push(TouchPlay { obj, key, track: t, group: row.group, notes, start, len: region.length as f64 * mpt, looping: matches!(trigger, 3 | 5), gate: matches!(trigger, 2 | 3), sounding: Vec::new(), next: 0 });
}

fn stop_plays(app: &mut App, f: impl Fn(&TouchPlay) -> bool) {
    let mut keep = Vec::new();
    for p in std::mem::take(&mut app.hub.env.plays) {
        if f(&p) {
            if let Ok(mut e) = app.engine.lock() {
                for (_, pitch) in &p.sounding {
                    e.live(p.track, *pitch, 0);
                }
            }
        } else {
            keep.push(p);
        }
    }
    app.hub.env.plays = keep;
}

fn touch_frame(app: &mut App, ms: f64) {
    if app.hub.env.plays.is_empty() {
        return;
    }
    let Ok(mut e) = app.engine.lock() else { return };
    for p in app.hub.env.plays.iter_mut() {
        let t = ms - p.start;
        if t < 0.0 {
            continue;
        }
        while p.next < p.notes.len() && p.notes[p.next].0 <= t {
            let (_, end, pitch, vel) = p.notes[p.next];
            e.live(p.track, pitch, vel);
            p.sounding.push((p.start + end, pitch));
            p.next += 1;
        }
        let track = p.track;
        p.sounding.retain(|(end, pitch)| {
            if *end <= ms {
                e.live(track, *pitch, 0);
                false
            } else {
                true
            }
        });
        if t >= p.len && p.looping && p.len > 0.0 {
            p.start += p.len;
            p.next = 0;
        }
    }
    let track_off: Vec<(usize, u8)> = app.hub.env.plays.iter().filter(|p| ms - p.start >= p.len && !p.looping).flat_map(|p| p.sounding.iter().map(move |s| (p.track, s.1))).collect();
    for (t, pitch) in track_off {
        e.live(t, pitch, 0);
    }
    app.hub.env.plays.retain(|p| ms - p.start < p.len || p.looping);
}

/// The MIDI click object follows the song: called on each bar / beat / division crossed.
pub fn click(app: &mut App, ctx: &egui::Context, which: u8) {
    let c = clock(app);
    let rec = app.recording();
    let outs = app.hub.env.env.click(which, rec, c);
    dispatch(app, ctx, outs);
}

// ---- commands ----

fn front(app: &App) -> usize {
    app.hub.env.front
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    let w = front(app);
    match name {
        "Open Environment…" | "Open MIDI Environment" => {
            let s = &mut app.hub.env;
            // (open already: another window)
            if s.wins[0].open && !s.wins[1].open {
                s.wins[1] = Win { open: true, layer: s.wins[0].layer, ..Default::default() };
                s.front = 1;
            } else {
                s.wins[0].open = true;
                s.front = 0;
            }
            sync_strips(app);
        }
        "Open Environment Mixer…" => {
            sync_strips(app);
            let s = &mut app.hub.env;
            s.wins[0].open = true;
            s.wins[0].layer = s.env.layers.iter().position(|l| l == "Mixer").unwrap_or(2);
            s.front = 0;
        }
        "Show/Hide Environment" => {
            let s = &mut app.hub.env;
            s.wins[0].open = !s.wins[0].open;
            if s.wins[0].open {
                s.front = 0;
            }
            sync_strips(app);
        }
        _ => {
            if !COMMANDS.contains(&name) {
                return false;
            }
            if !app.hub.env.wins[w].open {
                app.tell("Open the MIDI Environment (⌘0) first");
                return true;
            }
            env_command(app, ctx, w, name);
        }
    }
    true
}

fn new_kind(name: &str) -> Option<Kind> {
    Some(match name {
        "New MIDI Instrument" | "Instrument" => Kind::Instrument,
        "New Multi-Instrument" | "Multi-Instrument" => Kind::MultiInstrument,
        "New Mapped Instrument" | "Mapped Instrument" => Kind::MappedInstrument,
        "New Fader/Knobs/…" => Kind::Fader,
        "New Keyboard" | "Keyboard" => Kind::Keyboard,
        "New Monitor" | "Monitor" => Kind::Monitor,
        "New Arpeggiator" | "Arpeggiator" => Kind::Arpeggiator,
        "New Transformer" | "Transformer" => Kind::Transformer,
        "New Delay Line" | "Delay Line" => Kind::DelayLine,
        "New Voice Limiter" | "Voice Limiter" => Kind::VoiceLimiter,
        "New Channel Splitter" | "Channel Splitter" => Kind::ChannelSplitter,
        "New Chord Memorizer" | "Chord Memorizer" => Kind::ChordMemorizer,
        "Touch Tracks" => Kind::TouchTracks,
        "Ornament" => Kind::Ornament,
        "MIDI Metronome Click" => Kind::MidiClick,
        "MMC Record Buttons" => Kind::MmcRecord,
        "GM Mixer" => Kind::GmMixer,
        "Physical Input" => Kind::PhysicalInput,
        "Sequencer Input" => Kind::SequencerInput,
        _ => return None,
    })
}

/// A new object on the window's layer, at a free spot, selected.
fn add_object(app: &mut App, w: usize, kind: Kind, at: Option<(f32, f32)>) -> Id {
    checkpoint(app);
    let s = &mut app.hub.env;
    let layer = s.wins[w].layer.max(2);
    // (a free spot: under everything on the layer)
    let (x, y) = at.unwrap_or_else(|| {
        let bottom = s.env.objects.iter().filter(|o| o.layer == layer).map(|o| o.y + o.h + 22.0).fold(0.0f32, f32::max);
        (40.0, (bottom + 16.0).max(20.0))
    });
    let id = s.env.add(kind, layer, x, y);
    s.wins[w].sel = vec![id];
    if kind == Kind::MappedInstrument {
        s.sub = Some((Sub::Mapped, id));
    }
    id
}

fn env_command(app: &mut App, ctx: &egui::Context, w: usize, name: &str) {
    if let Some(k) = new_kind(name) {
        add_object(app, w, k, None);
        return;
    }
    let sel = app.hub.env.wins[w].sel.clone();
    let c = clock(app);
    match name {
        "Clear Cables only" => {
            checkpoint(app);
            let s = &mut app.hub.env;
            for cab in std::mem::take(&mut s.wins[w].sel_cables) {
                s.env.disconnect(cab);
            }
        }
        "Clean Up Positions" => {
            checkpoint(app);
            for id in sel {
                if let Some(o) = app.hub.env.env.obj_mut(id) {
                    o.x = (o.x / GRID).round() * GRID;
                    o.y = (o.y / GRID).round() * GRID;
                }
            }
        }
        "Reset Sizes" => {
            checkpoint(app);
            for id in sel {
                if let Some(o) = app.hub.env.env.obj_mut(id) {
                    (o.w, o.h) = o.kind.default_size();
                }
            }
        }
        "Align Objects" => {
            checkpoint(app);
            app.hub.env.env.align(&sel);
        }
        "Object move left" | "Object move right" | "Object move up" | "Object move down" | "Object Width -1 Pixel" | "Object Width +1 Pixel" | "Object Height -1 Pixel" | "Object Height +1 Pixel" => {
            if app.hub.env.protect {
                return;
            }
            checkpoint(app);
            for id in sel {
                if let Some(o) = app.hub.env.env.obj_mut(id) {
                    match name {
                        "Object move left" => o.x -= 1.0,
                        "Object move right" => o.x += 1.0,
                        "Object move up" => o.y -= 1.0,
                        "Object move down" => o.y += 1.0,
                        "Object Width -1 Pixel" => o.w = (o.w - 1.0).max(8.0),
                        "Object Width +1 Pixel" => o.w += 1.0,
                        "Object Height -1 Pixel" => o.h = (o.h - 1.0).max(8.0),
                        _ => o.h += 1.0,
                    }
                }
            }
        }
        "Show/Hide Cables" => app.hub.env.cables_shown = !app.hub.env.cables_shown,
        "Protect Cabling/Positions" => app.hub.env.protect = !app.hub.env.protect,
        "Define Custom Bank Messages…" => {
            if let Some(&id) = sel.first() {
                if app.hub.env.env.obj(id).is_some_and(|o| matches!(o.kind, Kind::Instrument | Kind::MultiInstrument | Kind::MappedInstrument)) {
                    app.hub.env.sub = Some((Sub::Banks, id));
                }
            }
        }
        "Select Used Instruments" | "Select Unused Instruments" => {
            let s = &app.hub.env;
            let mut used: Vec<Id> = app.selected_track.and_then(|t| s.track_obj.get(&t).copied()).into_iter().collect();
            if name == "Select Unused Instruments" {
                // (used anywhere in the main window: every track's object and channel strip)
                used = s.track_obj.values().copied().collect();
                used.extend(s.env.objects.iter().filter(|o| o.kind == Kind::ChannelStrip).map(|o| o.id));
                let all_used = s.env.used_closure(&used);
                let layer = s.wins[w].layer;
                let pick: Vec<Id> = s.env.on_layer(layer).into_iter().filter(|id| !all_used.contains(id)).collect();
                app.hub.env.wins[w].sel = pick;
            } else {
                if let Some(t) = app.selected_track {
                    if let Some(o) = s.env.objects.iter().find(|o| o.kind == Kind::ChannelStrip && o.p("trk") == t as i32) {
                        used.push(o.id);
                    }
                }
                app.hub.env.wins[w].sel = app.hub.env.env.used_closure(&used);
            }
        }
        "Select Cable Destination" | "Select Cable Origin" => {
            let s = &mut app.hub.env;
            let cab = s.wins[w].sel_cables.first().copied().or_else(|| sel.first().and_then(|id| s.env.cables_from(*id).first().copied()));
            if let Some(cab) = cab {
                let id = if name == "Select Cable Destination" { cab.to } else { cab.from };
                s.wins[w].sel = vec![id];
                s.wins[w].sel_cables.clear();
                if let Some(o) = s.env.obj(id) {
                    if o.layer != 1 && o.layer != s.wins[w].layer && s.wins[w].layer != 0 {
                        s.wins[w].prev_layer = s.wins[w].layer;
                        s.wins[w].layer = o.layer;
                    }
                }
            }
        }
        "Select Next Object Above" | "Select Next Object Below" | "Select Next Object Left" | "Select Next Object Right" => {
            let s = &mut app.hub.env;
            let ids = s.env.on_layer(s.wins[w].layer);
            let cur = sel.first().and_then(|id| s.env.obj(*id)).map(|o| (o.x, o.y));
            let pick = ids.iter().filter_map(|id| s.env.obj(*id)).filter(|o| match cur {
                None => true,
                Some((x, y)) => match name {
                    "Select Next Object Above" => o.y < y,
                    "Select Next Object Below" => o.y > y,
                    "Select Next Object Left" => o.x < x,
                    _ => o.x > x,
                },
            }).min_by(|a, b| {
                let d = |o: &Obj| cur.map_or(o.x + o.y, |(x, y)| (o.x - x).abs() + (o.y - y).abs());
                d(a).partial_cmp(&d(b)).unwrap_or(std::cmp::Ordering::Equal)
            }).map(|o| o.id);
            if let Some(id) = pick {
                s.wins[w].sel = vec![id];
            }
        }
        "Go to Layer of Object" => {
            let s = &mut app.hub.env;
            if let Some(o) = sel.first().and_then(|id| s.env.obj(*id)) {
                let l = o.layer;
                s.wins[w].prev_layer = s.wins[w].layer;
                s.wins[w].layer = l;
            }
        }
        "Go to Previous Layer" => {
            let win = &mut app.hub.env.wins[w];
            std::mem::swap(&mut win.layer, &mut win.prev_layer);
        }
        "Send Selected Fader Values" => {
            let outs = app.hub.env.env.send_fader_values(Some(&sel), c);
            dispatch(app, ctx, outs);
        }
        "Send All Current Fader Values" => {
            let outs = app.hub.env.env.send_fader_values(None, c);
            dispatch(app, ctx, outs);
        }
        "Delete Layer" => {
            let l = app.hub.env.wins[w].layer;
            if l >= 2 {
                app.hub.env.dlg = Some(Dlg::DeleteLayer(l));
            }
        }
        "Rename Layer…" => {
            let l = app.hub.env.wins[w].layer;
            if l >= 2 {
                app.hub.env.dlg = Some(Dlg::RenameLayer(l, app.hub.env.env.layers[l].clone()));
            }
        }
        "Create Layer" => {
            checkpoint(app);
            let s = &mut app.hub.env;
            let l = s.env.create_layer(s.wins[w].layer);
            s.wins[w].prev_layer = s.wins[w].layer;
            s.wins[w].layer = l;
        }
        "Cable serially" => {
            checkpoint(app);
            app.hub.env.env.cable_serially(&sel);
        }
        x if x.starts_with("Apply Buffer Template to ") => {
            apply_template(app, w, &x["Apply Buffer Template to ".len()..]);
        }
        _ => {}
    }
}

/// Options ▸ Apply Buffer Template to: the copied objects' size, position, definition or
/// cabling given to the selected ones.
fn apply_template(app: &mut App, w: usize, what: &str) {
    let Some(clip) = app.hub.env.clip.clone() else {
        app.tell("Copy objects first: they are the template");
        return;
    };
    checkpoint(app);
    let s = &mut app.hub.env;
    let mut sel: Vec<Id> = s.wins[w].sel.clone();
    sel.sort_by(|a, b| {
        let p = |id: &Id| s.env.obj(*id).map_or(0.0, |o| o.y * 10000.0 + o.x);
        p(a).partial_cmp(&p(b)).unwrap_or(std::cmp::Ordering::Equal)
    });
    let tmpl = &clip.objects;
    if tmpl.is_empty() {
        return;
    }
    let (ox, oy) = (tmpl.iter().map(|o| o.x).fold(f32::MAX, f32::min), tmpl.iter().map(|o| o.y).fold(f32::MAX, f32::min));
    for (k, id) in sel.iter().enumerate() {
        let t = &tmpl[k % tmpl.len()];
        let Some(o) = s.env.obj_mut(*id) else { continue };
        match what {
            "Size" => (o.w, o.h) = (t.w, t.h),
            "Position" => (o.x, o.y) = (20.0 + t.x - ox, 20.0 + t.y - oy),
            "Position and Size" => (o.x, o.y, o.w, o.h) = (20.0 + t.x - ox, 20.0 + t.y - oy, t.w, t.h),
            "Definition" | "Definition, channel increment" | "Definition, number increment" => {
                // (the closest-sized template object of the same kind)
                let Some(best) = tmpl.iter().filter(|x| x.kind == o.kind).min_by(|a, b| ((a.w - o.w).abs() + (a.h - o.h).abs()).partial_cmp(&((b.w - o.w).abs() + (b.h - o.h).abs())).unwrap_or(std::cmp::Ordering::Equal)) else { continue };
                o.params = best.params.clone();
                o.texts = best.texts.clone();
                if what.ends_with("channel increment") {
                    for key in ["ch", "in_ch", "out_ch"] {
                        if o.params.contains_key(key) || key == "ch" {
                            let v = (best.p(key) - 1 + k as i32) % 16 + 1;
                            o.params.insert(key.into(), v);
                        }
                    }
                }
                if what.ends_with("number increment") {
                    for key in ["in_d1", "out_d1"] {
                        o.params.insert(key.into(), (best.p(key) + k as i32).min(127));
                    }
                }
            }
            _ => {}
        }
    }
    if what == "Cables" {
        // the template's cables between its objects, onto the selection in the same order
        let mut map = BTreeMap::new();
        for (k, t) in tmpl.iter().enumerate() {
            if let Some(id) = sel.get(k) {
                if s.env.obj(*id).is_some_and(|o| o.kind == t.kind) {
                    map.insert(t.id, *id);
                }
            }
        }
        for c in &clip.cables {
            if let (Some(&a), Some(&b)) = (map.get(&c.from), map.get(&c.to)) {
                s.env.connect(a, c.outlet, b);
            }
        }
    }
}

/// Edit menu operations on the front window.
fn edit_op(app: &mut App, ctx: &egui::Context, w: usize, op: &str) {
    let sel = app.hub.env.wins[w].sel.clone();
    match op {
        "Undo" => {
            let s = &mut app.hub.env;
            if let Some(e) = s.undo.pop() {
                s.redo.push(std::mem::replace(&mut s.env, e));
            }
        }
        "Redo" => {
            let s = &mut app.hub.env;
            if let Some(e) = s.redo.pop() {
                s.undo.push(std::mem::replace(&mut s.env, e));
            }
        }
        "Cut" | "Copy" => {
            let s = &mut app.hub.env;
            let mut clip = Env::new();
            clip.layers = s.env.layers.clone();
            clip.objects = s.env.objects.iter().filter(|o| sel.contains(&o.id)).cloned().collect();
            // (a group keeps its cables; a single object its output cables)
            clip.cables = s.env.cables.iter().filter(|c| sel.contains(&c.from) && (sel.contains(&c.to) || sel.len() == 1)).copied().collect();
            s.clip = Some(clip);
            if op == "Cut" {
                checkpoint(app);
                app.hub.env.env.delete(&sel);
                app.hub.env.wins[w].sel.clear();
            }
        }
        "Paste" => {
            if !app.hub.env.wins[w].sel.is_empty() {
                app.hub.env.dlg = Some(Dlg::PasteReplace);
                return;
            }
            paste(app, w, false);
        }
        "Delete" => {
            checkpoint(app);
            let s = &mut app.hub.env;
            for c in std::mem::take(&mut s.wins[w].sel_cables) {
                s.env.disconnect(c);
            }
            s.env.delete(&sel);
            s.wins[w].sel.clear();
        }
        "Select All" => {
            let s = &mut app.hub.env;
            s.wins[w].sel = s.env.on_layer(s.wins[w].layer);
        }
        "Invert Selection" => {
            let s = &mut app.hub.env;
            s.wins[w].sel = s.env.on_layer(s.wins[w].layer).into_iter().filter(|id| !sel.contains(id)).collect();
        }
        "Deselect All" => {
            app.hub.env.wins[w].sel.clear();
            app.hub.env.wins[w].sel_cables.clear();
        }
        x => env_command(app, ctx, w, x),
    }
}

/// Paste: the copied objects onto the window's layer (`replace`: in place of the selection,
/// keeping its cables).
fn paste(app: &mut App, w: usize, replace: bool) {
    let Some(clip) = app.hub.env.clip.clone() else { return };
    checkpoint(app);
    let s = &mut app.hub.env;
    let layer = s.wins[w].layer.max(2);
    let mut map = BTreeMap::new();
    let mut new = Vec::new();
    let old_sel = s.wins[w].sel.clone();
    for (k, o) in clip.objects.iter().enumerate() {
        if o.kind.unique() && s.env.find(o.kind).is_some() {
            continue;
        }
        let id = s.env.next_id;
        s.env.next_id += 1;
        let mut n = o.clone();
        n.id = id;
        n.layer = layer;
        if replace {
            if let Some(old) = old_sel.get(k).and_then(|x| s.env.obj(*x)).cloned() {
                n.x = old.x;
                n.y = old.y;
                for c in s.env.cables.iter_mut() {
                    if c.from == old.id {
                        c.from = id;
                    }
                    if c.to == old.id {
                        c.to = id;
                    }
                }
            }
        } else {
            n.x += 16.0;
            n.y += 16.0;
        }
        s.env.objects.push(n);
        map.insert(o.id, id);
        new.push(id);
    }
    if replace {
        s.env.delete(&old_sel);
    }
    for c in &clip.cables {
        match (map.get(&c.from), map.get(&c.to)) {
            (Some(&a), Some(&b)) => s.env.cables.push(Cable { from: a, outlet: c.outlet, to: b }),
            (Some(&a), None) if s.env.obj(c.to).is_some() => s.env.cables.push(Cable { from: a, outlet: c.outlet, to: c.to }),
            _ => {}
        }
    }
    s.wins[w].sel = new;
}

/// Keys while the pointer is over an Environment window: its key commands (Logic's MIDI
/// Environment set, Delete, ⌘C / ⌘X / ⌘V / ⌘Z, I for the inspector). True when used.
pub fn key_press(app: &mut App, ctx: &egui::Context, key: egui::Key, m: egui::Modifiers) -> bool {
    if !app.hub.env.hovered || ctx.egui_wants_keyboard_input() {
        return false;
    }
    let w = front(app);
    use egui::Key as K;
    let name: Option<&str> = match (key, m.command, m.ctrl && !m.command, m.alt, m.shift) {
        (K::ArrowLeft, false, false, true, true) => Some("Object Width -1 Pixel"),
        (K::ArrowRight, false, false, true, true) => Some("Object Width +1 Pixel"),
        (K::ArrowUp, false, false, true, true) => Some("Object Height -1 Pixel"),
        (K::ArrowDown, false, false, true, true) => Some("Object Height +1 Pixel"),
        (K::ArrowLeft, false, false, true, false) => Some("Object move left"),
        (K::ArrowRight, false, false, true, false) => Some("Object move right"),
        (K::ArrowUp, false, false, true, false) => Some("Object move up"),
        (K::ArrowDown, false, false, true, false) => Some("Object move down"),
        (K::ArrowRight, false, false, false, true) => Some("Select Cable Destination"),
        (K::ArrowLeft, false, false, false, true) => Some("Select Cable Origin"),
        (K::ArrowUp, false, false, false, false) => Some("Select Next Object Above"),
        (K::ArrowDown, false, false, false, false) => Some("Select Next Object Below"),
        (K::ArrowLeft, false, false, false, false) => Some("Select Next Object Left"),
        (K::ArrowRight, false, false, false, false) => Some("Select Next Object Right"),
        (K::Backspace, false, true, false, false) => Some("Clear Cables only"),
        (K::C, false, true, false, false) => Some("Show/Hide Cables"),
        (K::P, false, true, false, false) => Some("Protect Cabling/Positions"),
        (K::S, false, true, false, false) => Some("Cable serially"),
        (K::V, false, true, false, false) => Some("Send Selected Fader Values"),
        (K::I, false, false, false, true) => Some("Define Custom Bank Messages…"),
        (K::U, false, false, false, true) => Some("Select Unused Instruments"),
        (K::Backspace, true, false, false, false) => Some("Delete Layer"),
        _ => None,
    };
    if let Some(n) = name {
        // (the edited key command set may give these other keys; these are Logic's defaults)
        env_command(app, ctx, w, n);
        return true;
    }
    let op = match (key, m.command, m.shift) {
        (K::Z, true, false) => "Undo",
        (K::Z, true, true) => "Redo",
        (K::C, true, false) => "Copy",
        (K::X, true, false) => "Cut",
        (K::V, true, false) => "Paste",
        (K::A, true, false) => "Select All",
        (K::Delete | K::Backspace, false, _) => "Delete",
        (K::I, false, true) => "Invert Selection",
        (K::I, false, false) => {
            let win = &mut app.hub.env.wins[w];
            win.inspector = !win.inspector;
            return true;
        }
        (K::Escape, _, _) => "Deselect All",
        _ => return false,
    };
    edit_op(app, ctx, w, op);
    true
}

// ---- the window ----

pub fn window(app: &mut App, ctx: &egui::Context) {
    app.hub.env.hovered = false;
    for w in 0..2 {
        if app.hub.env.wins[w].open {
            one_window(app, ctx, w);
        }
    }
    sub_windows(app, ctx);
    dialogs(app, ctx);
}

fn one_window(app: &mut App, ctx: &egui::Context, w: usize) {
    let mut open = true;
    let layer_name = app.hub.env.env.layers.get(app.hub.env.wins[w].layer).cloned().unwrap_or_default();
    let title = format!("{} - {} - Environment", app.project.as_ref().map_or("Untitled".to_string(), |p| p.name.clone()), layer_name);
    let frameless = app.hub.env.wins[w].frameless;
    let resp = egui::Window::new(title).id(egui::Id::unique(("environment", w))).open(&mut open).title_bar(!frameless).resizable(true).collapsible(false).default_size(vec2(980.0, 600.0)).default_pos(pos2(120.0 + w as f32 * 40.0, 90.0 + w as f32 * 40.0)).show(ctx, |ui| {
        ui.set_min_size(vec2(520.0, 320.0));
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        let inspector = app.hub.env.wins[w].inspector;
        let left_w = if inspector { 230.0 } else { 0.0 };
        if inspector {
            let left = Rect::from_min_size(full.min, vec2(left_w, full.height()));
            let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(left.shrink(6.0)));
            left_panel(app, &mut lui, w);
            ui.painter().vline(left.max.x, full.y_range(), Stroke::new(1.0, Color32::BLACK));
        }
        let right = Rect::from_min_max(pos2(full.min.x + left_w + 1.0, full.min.y), full.max);
        let bar = Rect::from_min_size(right.min, vec2(right.width(), 34.0));
        let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(8.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        top_bar(app, &mut bui, ctx, w);
        let canvas = Rect::from_min_max(pos2(right.min.x, bar.max.y), right.max);
        app.hub.env.wins[w].canvas = canvas;
        if app.hub.env.wins[w].by_text || app.hub.env.wins[w].layer == 0 {
            let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(canvas.shrink(6.0)));
            list_view(app, &mut cui, w);
        } else {
            canvas_ui(app, ui, ctx, w, canvas);
        }
    });
    if let Some(r) = resp {
        if r.response.rect.contains(ctx.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO)) {
            app.hub.env.hovered = true;
            if ctx.input(|i| i.pointer.any_pressed()) {
                app.hub.env.front = w;
            }
        }
    }
    if !open {
        app.hub.env.wins[w].open = false;
    }
}

fn left_panel(app: &mut App, ui: &mut egui::Ui, w: usize) {
    // Layer pop-up: the layers, then Rename / Delete / Create Layer; Option-choosing a layer
    // moves the selected objects there
    ui.horizontal(|ui| {
        ui.label("Layer:");
        let s = &app.hub.env;
        let cur = s.env.layers.get(s.wins[w].layer).cloned().unwrap_or_default();
        let layers = s.env.layers.clone();
        let mut pick = None;
        let mut action = None;
        egui::ComboBox::from_id_salt(("env-layer", w)).selected_text(cur).width(150.0).show_ui(ui, |ui| {
            for (i, l) in layers.iter().enumerate() {
                if ui.selectable_label(i == app.hub.env.wins[w].layer, l).clicked() {
                    pick = Some(i);
                }
            }
            ui.separator();
            for a in ["Rename Layer…", "Delete Layer", "Create Layer"] {
                if ui.button(a).clicked() {
                    action = Some(a);
                }
            }
        });
        if let Some(i) = pick {
            if ui.input(|x| x.modifiers.alt) && i >= 1 {
                checkpoint(app);
                let sel = app.hub.env.wins[w].sel.clone();
                for id in sel {
                    if let Some(o) = app.hub.env.env.obj_mut(id) {
                        o.layer = i;
                    }
                }
            } else {
                let win = &mut app.hub.env.wins[w];
                win.prev_layer = win.layer;
                win.layer = i;
            }
        }
        match action {
            Some("Rename Layer…") => {
                let l = app.hub.env.wins[w].layer;
                if l >= 2 {
                    app.hub.env.dlg = Some(Dlg::RenameLayer(l, app.hub.env.env.layers[l].clone()));
                }
            }
            Some("Delete Layer") => {
                let l = app.hub.env.wins[w].layer;
                if l >= 2 {
                    app.hub.env.dlg = Some(Dlg::DeleteLayer(l));
                }
            }
            Some("Create Layer") => {
                let ctx = ui.ctx().clone();
                env_command(app, &ctx, w, "Create Layer");
            }
            _ => {}
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().id_salt(("env-insp", w)).show(ui, |ui| inspector(app, ui, w));
}

/// The Object inspector: name, icon, Assignable, Port, and the kind's parameters.
fn inspector(app: &mut App, ui: &mut egui::Ui, w: usize) {
    let Some(&id) = app.hub.env.wins[w].sel.first() else {
        ui.label(egui::RichText::new("No object selected").color(look::text_dim()));
        return;
    };
    let Some(mut o) = app.hub.env.env.obj(id).cloned() else { return };
    let before = o.clone();
    let ports = app.hub.outs.ports();
    let tracks: Vec<String> = app.project.as_ref().map(|p| p.tracks.iter().map(|t| t.name.clone()).collect()).unwrap_or_default();
    egui::Grid::new(("env-grid", w)).num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        ui.label(format!("{}:", o.kind.name()));
        ui.text_edit_singleline(&mut o.name);
        ui.end_row();
        ui.label("Icon:");
        ui.add(egui::DragValue::new(&mut o.icon).range(0..=99));
        ui.end_row();
        // (View ▸ Show Colors: the object's colour, which its cables take)
        ui.label("Color:");
        let mut rgb = [(o.color >> 16) as u8, (o.color >> 8) as u8, o.color as u8];
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            o.color = (rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32;
        }
        ui.end_row();
        if o.kind.assignable_default() || o.kind == Kind::Instrument {
            ui.label("Assignable:");
            ui.checkbox(&mut o.assignable, "");
            ui.end_row();
        }
        if o.kind.has_port() {
            ui.label("Port:");
            let shown = match o.port.as_str() {
                "" => "Off".to_string(),
                "*" => "All".to_string(),
                p => p.to_string(),
            };
            egui::ComboBox::from_id_salt(("env-port", w)).selected_text(shown).width(104.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut o.port, String::new(), "Off");
                ui.selectable_value(&mut o.port, "*".to_string(), "All");
                for p in &ports {
                    ui.selectable_value(&mut o.port, p.clone(), p);
                }
            });
            ui.end_row();
        }
        // a multi-instrument's selected subchannel shows the full instrument set
        let sub = app.hub.env.subch.filter(|s| s.0 == id).map(|s| s.1);
        let specs: Vec<envm::Spec> = if o.kind == Kind::MultiInstrument && sub.is_some() { envm::specs(Kind::Instrument).iter().filter(|s| s.0 != "ch").copied().collect() } else { envm::specs(o.kind).to_vec() };
        if let Some(n) = sub {
            ui.label("Subchannel:");
            ui.label(format!("{n}"));
            ui.end_row();
        }
        for s in specs {
            let key = match sub {
                Some(n) => format!("s{n}.{}", s.0),
                None => s.0.to_string(),
            };
            // (faders: the definitions only where they mean something)
            if o.kind == Kind::Fader && matches!(s.0, "val2") && o.p("style") != 11 {
                continue;
            }
            if o.kind == Kind::ChannelStrip && s.0 == "trk" {
                ui.label("Track:");
                let mut t = o.p("trk").max(0) as usize;
                egui::ComboBox::from_id_salt(("env-strip-trk", w)).selected_text(tracks.get(t).cloned().unwrap_or_else(|| format!("{}", t + 1))).show_ui(ui, |ui| {
                    for (k, n) in tracks.iter().enumerate() {
                        ui.selectable_value(&mut t, k, n);
                    }
                });
                o.set("trk", t as i32);
                ui.end_row();
                continue;
            }
            ui.label(format!("{}:", s.1));
            let mut v = o.p(&key);
            match s.2 {
                Widget::Check => {
                    let mut b = v != 0;
                    ui.checkbox(&mut b, "");
                    v = b as i32;
                }
                Widget::Popup(opts) => {
                    let cur = opts.get(v.max(0) as usize).copied().unwrap_or("");
                    egui::ComboBox::from_id_salt(("env-pop", w, s.0)).selected_text(cur).width(104.0).show_ui(ui, |ui| {
                        for (k, n) in opts.iter().enumerate() {
                            ui.selectable_value(&mut v, k as i32, *n);
                        }
                    });
                }
                Widget::Note => {
                    ui.add(egui::DragValue::new(&mut v).range(0..=127).custom_formatter(|x, _| envm::note_name(x as u8)));
                }
                Widget::Num(a, b) => {
                    // (Channel 0 shows All; the fader's Control -1- names its controller)
                    let all = (s.0 == "ch" || s.0 == "ch") && a == 0;
                    ui.add(egui::DragValue::new(&mut v).range(a..=b).custom_formatter(move |x, _| if all && x == 0.0 { "All".into() } else { format!("{x:.0}") }));
                }
            }
            if v != o.p(&key) {
                o.set(&key, v);
            }
            ui.end_row();
        }
    });
    // Program / Volume / Pan: sent at once when changed and ticked
    let setup_changed = ["prog_on", "prog", "bank", "vol_on", "vol", "pan_on", "pan"].iter().any(|k| before.p(k) != o.p(k));
    if o.kind == Kind::Instrument || o.kind == Kind::MultiInstrument || o.kind == Kind::MappedInstrument {
        ui.add_space(4.0);
        if ui.button("Custom Bank Messages…").clicked() {
            app.hub.env.sub = Some((Sub::Banks, id));
        }
    }
    if o.kind == Kind::GmMixer {
        gm_inspector(app, ui, &mut o);
    }
    if o.kind == Kind::Alias {
        if let Some(r) = app.hub.env.env.obj(o.p("ref") as Id) {
            ui.label(format!("Reference: {}", r.name));
        }
    }
    if o.kind == Kind::Fader && ((o.p("out_t") == 7 && before.p("out_t") != 7) || (o.p("in_t") == 7 && before.p("in_t") != 7)) {
        app.hub.env.sub = Some((Sub::SysEx, id));
    }
    if o != before {
        checkpoint(app);
        if let Some(x) = app.hub.env.env.obj_mut(id) {
            *x = o.clone();
        }
        if setup_changed {
            let pre = app.hub.env.subch.filter(|s| s.0 == id).map(|s| format!("s{}", s.1)).unwrap_or_default();
            for ev in envm::instrument_setup(&o, &pre) {
                if let Some(b) = ev.bytes() {
                    if !o.port.is_empty() {
                        app.hub.outs.send(&o.port, &b);
                    }
                }
            }
        }
    }
}

/// The GM mixer's Reset button and, for GS and XG, the GS/XG Effect Controls (reverb and
/// chorus programs and levels, the reverb time).
fn gm_inspector(app: &mut App, ui: &mut egui::Ui, o: &mut Obj) {
    let std = o.p("std");
    let port = if o.port.is_empty() { "*".to_string() } else { o.port.clone() };
    ui.add_space(4.0);
    if ui.button("Reset").on_hover_text(format!("Sends {} and resets all controllers", ["GM On", "GS Reset", "XG System On"][std.clamp(0, 2) as usize])).clicked() {
        for m in envm::gm_reset(std) {
            app.hub.outs.send(&port, &m);
        }
    }
    if std == 0 {
        return;
    }
    egui::CollapsingHeader::new("GS/XG Effect Controls").id_salt(("gsxg", o.id)).default_open(o.on("fxshow")).show(ui, |ui| {
        let (revs, chos) = if std == 1 { (envm::GS_REVERBS, envm::GS_CHORUSES) } else { (envm::XG_REVERBS, envm::XG_CHORUSES) };
        let mut send: Option<(u8, u8)> = None;
        egui::Grid::new(("gsxg-grid", o.id)).num_columns(2).show(ui, |ui| {
            for (what, key, label, opts) in [(0u8, "fx_rev", "Reverb", revs), (1, "fx_cho", "Chorus", chos)] {
                ui.label(label);
                let mut v = o.p(key).clamp(0, opts.len() as i32 - 1);
                egui::ComboBox::from_id_salt((key, o.id)).selected_text(opts[v as usize]).show_ui(ui, |ui| {
                    for (k, n) in opts.iter().enumerate() {
                        if ui.selectable_value(&mut v, k as i32, *n).clicked() {
                            send = Some((what, k as u8));
                        }
                    }
                });
                o.params.insert(key.into(), v);
                ui.end_row();
            }
            for (what, key, label, def) in [(2u8, "fx_revlvl", "Reverb Level", 64), (3, "fx_cholvl", "Chorus Level", 64), (4, "fx_time", "Time", 64)] {
                ui.label(label);
                let mut v = if o.params.contains_key(key) { o.p(key) } else { def };
                // (double-click the Time field to type a value)
                if ui.add(egui::DragValue::new(&mut v).range(0..=127)).changed() {
                    send = Some((what, v as u8));
                }
                o.params.insert(key.into(), v);
                ui.end_row();
            }
        });
        if let Some((what, v)) = send {
            if let Some(m) = envm::gsxg_effect(std, what, v) {
                app.hub.outs.send(&port, &m);
            }
        }
    });
}

fn top_bar(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, w: usize) {
    let mut fired: Option<String> = None;
    ui.menu_button("New ⌄", |ui| {
        for k in ["Instrument", "Multi-Instrument", "Mapped Instrument", "Touch Tracks"] {
            if ui.button(k).clicked() {
                fired = Some(k.into());
            }
        }
        ui.menu_button("Fader", |ui| {
            for (k, st) in envm::FADER_STYLES.iter().enumerate().skip(1) {
                if ui.button(*st).clicked() {
                    fired = Some(format!("fader:{k}"));
                }
            }
            ui.separator();
            ui.menu_button("Specials", |ui| {
                for (k, n) in [(48, "Cable Switcher"), (46, "Alias Assigner"), (100, "Tempo Control"), (49, "Screenset Meta"), (51, "Marker Meta"), (52, "Stop Meta"), (99, "Bang Meta")] {
                    if ui.button(n).clicked() {
                        fired = Some(format!("special:{k}"));
                    }
                }
            });
        });
        ui.separator();
        for k in ["Keyboard", "Monitor", "Macro", "Arpeggiator", "Transformer", "Delay Line", "Voice Limiter", "Channel Splitter", "Chord Memorizer", "Alias", "Ornament", "GM Mixer", "MMC Record Buttons"] {
            if ui.add_enabled(k != "Macro" || !app.hub.env.wins[w].sel.is_empty(), egui::Button::new(k)).clicked() {
                fired = Some(k.into());
            }
        }
        ui.separator();
        for k in ["Physical Input", "Sequencer Input", "MIDI Metronome Click"] {
            let on = new_kind(k).is_some_and(|kk| app.hub.env.env.find(kk).is_none());
            if ui.add_enabled(on, egui::Button::new(k)).clicked() {
                fired = Some(k.into());
            }
        }
        ui.separator();
        let auto = app.proj.on("Audio / Channel Strips / Automatic management of channel strip objects");
        if ui.add_enabled(!auto, egui::Button::new("Channel Strip")).on_hover_text("Turn off Project Settings ▸ Audio ▸ Automatic management of channel strip objects first").clicked() {
            fired = Some("Channel Strip".into());
        }
    });
    ui.menu_button("Edit ⌄", |ui| {
        for k in ["Undo", "Redo", "-", "Cut", "Copy", "Paste", "Delete", "Clear Cables only", "-", "Select All", "Select Used Instruments", "Select Unused Instruments", "Select Cable Destination", "Select Cable Origin", "Invert Selection"] {
            if k == "-" {
                ui.separator();
            } else if ui.button(k).clicked() {
                fired = Some(format!("edit:{k}"));
            }
        }
    });
    ui.menu_button("Options ⌄", |ui| {
        ui.menu_button("Layer", |ui| {
            for k in ["Create Layer", "Delete Layer"] {
                if ui.button(k).clicked() {
                    fired = Some(format!("edit:{k}"));
                }
            }
        });
        for k in ["Go to Layer of Object", "Go to Previous Layer", "-", "Cable serially", "Send All Fader Values", "Send Selected Fader Values", "Reset Selected Faders", "-", "Define Custom Bank Messages…"] {
            if k == "-" {
                ui.separator();
            } else if ui.button(k).clicked() {
                fired = Some(format!("edit:{k}"));
            }
        }
        ui.menu_button("Clean up", |ui| {
            for (k, c) in [("Align Objects", "Align Objects"), ("Positions by Grid", "Clean Up Positions"), ("Size by Default", "Reset Sizes")] {
                if ui.button(k).clicked() {
                    fired = Some(format!("edit:{c}"));
                }
            }
        });
        ui.menu_button("Apply Buffer Template to", |ui| {
            for k in ["Size", "Position", "Position and Size", "Definition", "Definition, channel increment", "Definition, number increment", "Cables"] {
                if ui.button(k).clicked() {
                    fired = Some(format!("edit:Apply Buffer Template to {k}"));
                }
            }
        });
        ui.menu_button("Import Environment", |ui| {
            for k in ["Layer…", "Merge…", "Update…", "Replace by Port MIDI/Channel…", "Replace by Name…", "Total Replace…", "Custom…"] {
                if ui.button(k).clicked() {
                    fired = Some(format!("import:{k}"));
                }
            }
        });
    });
    ui.menu_button("View ⌄", |ui| {
        let s = &mut app.hub.env;
        let win = &mut s.wins[w];
        ui.checkbox(&mut win.inspector, "Show Inspector");
        ui.checkbox(&mut win.by_text, "By Text");
        ui.checkbox(&mut s.cables_shown, "Cables");
        ui.checkbox(&mut s.protect, "Protect Cabling/Positions");
        ui.checkbox(&mut s.colored, "Colored Cables");
        ui.checkbox(&mut s.snap, "Snap Positions");
        ui.checkbox(&mut s.wins[w].frameless, "Frameless Floating Window");
    });
    ui.separator();
    // the tools: Pointer, Pencil, Eraser, Text, MIDI Thru
    for (t, icon, tip) in [(Tool::Pointer, crate::glyphs::ph::CURSOR, "Pointer Tool"), (Tool::Pencil, crate::glyphs::ph::PENCIL, "Pencil Tool: click the background for a new instrument"), (Tool::Eraser, crate::glyphs::ph::ERASER, "Eraser Tool"), (Tool::Text, crate::glyphs::ph::TEXT_T, "Text Tool: rename"), (Tool::MidiThru, crate::glyphs::ph::LINK, "MIDI Thru Tool: assign the object to the selected track")] {
        let on = app.hub.env.wins[w].tool == t;
        let (r, resp) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::click());
        ui.painter().rect_filled(r, 4.0, if on { look::accent() } else if resp.hovered() { Color32::from_gray(70) } else { Color32::from_gray(52) });
        crate::glyphs::paint(ui.painter(), r, icon, 15.0, Color32::WHITE);
        if resp.on_hover_text(tip).clicked() {
            app.hub.env.wins[w].tool = t;
        }
    }
    if let Some(f) = fired {
        if let Some(k) = f.strip_prefix("fader:") {
            let id = add_object(app, w, Kind::Fader, None);
            let o = app.hub.env.env.obj_mut(id).unwrap();
            o.set("style", k.parse().unwrap_or(1));
            if o.p("style") == 11 {
                (o.w, o.h) = (90.0, 90.0);
            } else if o.p("style") == 6 {
                (o.w, o.h) = (110.0, 22.0);
            } else if matches!(o.p("style"), 7 | 8 | 9) {
                (o.w, o.h) = (36.0, 36.0);
            }
        } else if let Some(k) = f.strip_prefix("special:") {
            let id = add_object(app, w, Kind::Fader, None);
            let n: i32 = k.parse().unwrap_or(48);
            let o = app.hub.env.env.obj_mut(id).unwrap();
            o.set("out_t", 6);
            o.set("out_d1", n);
            o.set("style", if n == 48 { 0 } else { 8 });
            (o.w, o.h) = if n == 48 { (60.0, 30.0) } else { (40.0, 24.0) };
            o.name = match n {
                48 => "Cable Switcher",
                46 => "Alias Assigner",
                100 => "Tempo Control",
                _ => "Meta",
            }.into();
            if n == 100 {
                o.set("as", 6);
                o.set("val", 70);
            }
            if n == 48 {
                o.set("max", 127);
            }
        } else if let Some(op) = f.strip_prefix("edit:") {
            match op {
                "Send All Fader Values" => env_command(app, ctx, w, "Send All Current Fader Values"),
                "Reset Selected Faders" => {
                    checkpoint(app);
                    let sel = app.hub.env.wins[w].sel.clone();
                    let c = clock(app);
                    let outs = app.hub.env.env.reset_faders(&sel, c);
                    dispatch(app, ctx, outs);
                }
                x => edit_op(app, ctx, w, x),
            }
        } else if let Some(k) = f.strip_prefix("import:") {
            import_env(app, w, k);
        } else if f == "Alias" {
            let sel = app.hub.env.wins[w].sel.first().copied();
            match sel {
                Some(orig) => {
                    let (x, y, nm) = app.hub.env.env.obj(orig).map(|o| (o.x + 24.0, o.y + 24.0, o.name.clone())).unwrap_or_default();
                    let id = add_object(app, w, Kind::Alias, Some((x, y)));
                    let o = app.hub.env.env.obj_mut(id).unwrap();
                    o.set("ref", orig as i32);
                    o.name = nm;
                }
                None => app.tell("Select an object first: New ▸ Alias makes an alias of it"),
            }
        } else if f == "Macro" {
            new_macro(app, w);
        } else if f == "Channel Strip" {
            add_object(app, w, Kind::ChannelStrip, None);
        } else if let Some(k) = new_kind(&f) {
            add_object(app, w, k, None);
        }
    }
}

/// New ▸ Macro: the selected objects packed into a macro (cables to objects outside are
/// deleted, so the macro is then made from a copy and Logic's warning is shown).
fn new_macro(app: &mut App, w: usize) {
    let sel = app.hub.env.wins[w].sel.clone();
    checkpoint(app);
    match app.hub.env.env.make_macro(&sel) {
        Ok((m, copied)) => {
            app.hub.env.wins[w].sel = vec![m];
            if copied {
                app.tell("Cables to objects outside the macro were deleted: the macro was made from a copy, the original objects are unchanged");
            }
        }
        Err(e) => {
            app.hub.env.undo.pop();
            app.tell(e);
        }
    }
}

/// Options ▸ Import Environment: from another project's saved Environment.
fn import_env(app: &mut App, w: usize, how: &str) {
    let src = if app.automated {
        None
    } else {
        kn_ui::files::FileDialog::new().set_title("Import Environment from a project").pick_folder().and_then(|p| {
            let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
            let id: String = p.canonicalize().unwrap_or(p).display().to_string().chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
            std::fs::read_to_string(base.join("bramble-studio").join("environments").join(format!("{id}.txt"))).ok().and_then(|t| Env::from_text(&t)).or_else(|| Some(Env::new()))
        })
    };
    let Some(src) = src else { return };
    checkpoint(app);
    let s = &mut app.hub.env;
    match how {
        "Layer…" => {
            let l = s.wins[w].layer.max(2);
            s.env.import_layer(&src, l.min(src.layers.len().saturating_sub(1)));
        }
        "Merge…" => s.env.merge(&src),
        "Update…" => s.env.update_from(&src),
        "Replace by Port MIDI/Channel…" => s.env.replace_matching(&src, true),
        "Replace by Name…" => s.env.replace_matching(&src, false),
        "Total Replace…" => s.env.total_replace(&src),
        _ => s.dlg = Some(Dlg::ImportCustom(Box::new(src))),
    }
}

/// View ▸ By Text, and the All Objects layer: the objects as a list (no cables).
fn list_view(app: &mut App, ui: &mut egui::Ui, w: usize) {
    let s = &app.hub.env;
    let ids = s.env.on_layer(s.wins[w].layer);
    let rows: Vec<(Id, String, String, String)> = ids.iter().filter_map(|id| s.env.obj(*id)).map(|o| (o.id, o.name.clone(), o.kind.name().to_string(), s.env.layers.get(o.layer).cloned().unwrap_or_default())).collect();
    egui::ScrollArea::vertical().id_salt(("env-list", w)).show(ui, |ui| {
        egui::Grid::new(("env-list-grid", w)).striped(true).num_columns(3).show(ui, |ui| {
            ui.strong("Name");
            ui.strong("Type");
            ui.strong("Layer");
            ui.end_row();
            for (id, name, kind, layer) in rows {
                let on = app.hub.env.wins[w].sel.contains(&id);
                if ui.selectable_label(on, name).clicked() {
                    let m = ui.input(|i| i.modifiers);
                    let sel = &mut app.hub.env.wins[w].sel;
                    if m.shift || m.command {
                        if on { sel.retain(|x| *x != id) } else { sel.push(id) }
                    } else {
                        *sel = vec![id];
                    }
                }
                ui.label(kind);
                ui.label(layer);
                ui.end_row();
            }
        });
    });
}

// ---- drawing and editing on the layer ----

/// The output triangle positions (outlet number, point) of an object.
fn outlets(env: &Env, o: &Obj, r: Rect, ports: &[String]) -> Vec<(u16, Pos2)> {
    match o.kind {
        Kind::SequencerInput | Kind::Ornament | Kind::MmcRecord | Kind::ChannelStrip | Kind::TouchTracks => Vec::new(),
        Kind::PhysicalInput => (0..=ports.len().min(64) as u16).map(|k| (k, pos2(r.max.x + 4.0, r.min.y + 8.0 + k as f32 * 9.0))).collect(),
        Kind::ChannelSplitter => (0..17u16).map(|k| (k, pos2(r.max.x + 4.0, r.min.y + 4.0 + k as f32 * (r.height() - 8.0) / 16.0))).collect(),
        _ => {
            let n = env.cables.iter().filter(|c| c.from == o.id).count() as u16;
            (0..=n).map(|k| (k, pos2(r.max.x + 4.0, r.min.y + 5.0 + k as f32 * 7.0))).collect()
        }
    }
}

fn obj_rect(o: &Obj, canvas: Rect, scroll: egui::Vec2) -> Rect {
    Rect::from_min_size(canvas.min + vec2(o.x, o.y) - scroll + vec2(10.0, 10.0), vec2(o.w, o.h))
}

/// The name label under an object.
fn label_rect(r: Rect) -> Rect {
    Rect::from_min_size(pos2(r.min.x - 20.0, r.max.y + 1.0), vec2(r.width() + 40.0, 15.0))
}

fn col(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn cable_path(a: Pos2, b: Pos2) -> Vec<Pos2> {
    let mx = if b.x > a.x + 16.0 { (a.x + b.x) / 2.0 } else { a.x + 10.0 };
    if b.x > a.x + 16.0 {
        vec![a, pos2(mx, a.y), pos2(mx, b.y), b]
    } else {
        let ly = if b.y > a.y { b.y - 12.0 } else { b.y + 12.0 };
        vec![a, pos2(mx, a.y), pos2(mx, ly), pos2(b.x - 10.0, ly), pos2(b.x - 10.0, b.y), b]
    }
}

fn near_path(p: Pos2, path: &[Pos2]) -> bool {
    path.windows(2).any(|s| {
        let (a, b) = (s[0], s[1]);
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_sq().max(1e-3)).clamp(0.0, 1.0);
        (a + ab * t).distance(p) < 4.0
    })
}

fn canvas_ui(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, w: usize, canvas: Rect) {
    let id = ui.scope_id().with(("env-canvas", w));
    let resp = ui.interact(canvas, id, Sense::click_and_drag());
    let painter = ui.painter_at(canvas);
    let protect = app.hub.env.protect;
    let bg = if protect && !app.hub.env.cables_shown { Color32::from_rgb(0x2a, 0x26, 0x30) } else { Color32::from_gray(38) };
    painter.rect_filled(canvas, 0.0, bg);
    // scrolling the layer
    if resp.hovered() {
        let d = ui.input(|i| i.smooth_scroll_delta);
        let win = &mut app.hub.env.wins[w];
        win.scroll = (win.scroll - d).max(egui::Vec2::ZERO);
    }
    let scroll = app.hub.env.wins[w].scroll;
    let layer = app.hub.env.wins[w].layer;
    let ports: Vec<String> = app.live.input_names.clone();
    let ids = app.hub.env.env.on_layer(layer);
    let mut objs: Vec<Obj> = ids.iter().filter(|i| !app.hub.env.env.hidden_in_macro(**i)).filter_map(|i| app.hub.env.env.obj(*i).cloned()).collect();
    // macros behind their members (outer macros first)
    let depth = |id: Id| std::iter::successors(app.hub.env.env.macro_of(id), |m| app.hub.env.env.macro_of(*m)).count();
    objs.sort_by_key(|o| if o.kind == Kind::Macro { depth(o.id) as i32 - 100 } else { 0 });
    let rects: BTreeMap<Id, Rect> = objs.iter().map(|o| (o.id, obj_rect(o, canvas, scroll))).collect();
    // ornaments first (behind)
    for o in objs.iter().filter(|o| o.kind == Kind::Ornament) {
        painter.rect_filled(rects[&o.id], 4.0, col(o.color).gamma_multiply(0.35));
    }
    // cables
    let sel_cables = app.hub.env.wins[w].sel_cables.clone();
    let mut cable_hit: Option<Cable> = None;
    let hover = ctx.input(|i| i.pointer.hover_pos());
    if app.hub.env.cables_shown {
        let cables = app.hub.env.env.cables.clone();
        for c in &cables {
            let Some(src) = objs.iter().find(|o| o.id == c.from) else { continue };
            let a = outlets(&app.hub.env.env, src, rects[&c.from], &ports).into_iter().find(|x| x.0 == c.outlet).map(|x| x.1);
            let Some(a) = a.or_else(|| Some(pos2(rects[&c.from].max.x + 4.0, rects[&c.from].center().y))) else { continue };
            let color = if sel_cables.contains(c) { Color32::WHITE } else if app.hub.env.colored { col(src.color).gamma_multiply(0.9) } else { Color32::from_gray(150) };
            match rects.get(&c.to) {
                Some(tr) => {
                    let b = pos2(tr.min.x - 1.0, tr.min.y + 6.0);
                    let path = cable_path(a + vec2(4.0, 0.0), b);
                    if hover.is_some_and(|h| near_path(h, &path)) {
                        cable_hit = Some(*c);
                    }
                    painter.add(egui::Shape::line(path, Stroke::new(if sel_cables.contains(c) { 2.0 } else { 1.3 }, color)));
                }
                None => {
                    // to another layer: a short cable with the layer's name
                    let tl = app.hub.env.env.obj(c.to).map(|t| format!("→ {} ({})", t.name, app.hub.env.env.layers.get(t.layer).cloned().unwrap_or_default())).unwrap_or_default();
                    painter.line_segment([a + vec2(4.0, 0.0), a + vec2(24.0, 0.0)], Stroke::new(1.3, color));
                    painter.circle_filled(a + vec2(26.0, 0.0), 2.5, color);
                    painter.text(a + vec2(32.0, 0.0), Align2::LEFT_CENTER, tl, look::regular(10.0), look::text_dim());
                    if hover.is_some_and(|h| near_path(h, &[a, a + vec2(24.0, 0.0)])) {
                        cable_hit = Some(*c);
                    }
                }
            }
        }
    }
    // objects
    let sel = app.hub.env.wins[w].sel.clone();
    let held_of = |id: Id| app.hub.env.env.run.get(&id).map(|r| r.held.iter().map(|h| h.0).collect::<Vec<u8>>()).unwrap_or_default();
    for o in &objs {
        let r = rects[&o.id];
        let selected = sel.contains(&o.id);
        draw_object(app, &painter, o, r, selected, &held_of(o.id), &ports);
        let lr = label_rect(r);
        let renaming = app.hub.env.wins[w].rename.as_ref().is_some_and(|x| x.0 == o.id);
        if !renaming && !matches!(o.kind, Kind::Ornament | Kind::GmMixer | Kind::ChannelStrip) {
            painter.text(pos2(lr.center().x, lr.min.y), Align2::CENTER_TOP, &o.name, look::regular(11.5), if selected { Color32::WHITE } else { Color32::from_gray(185) });
        }
        if app.hub.env.cables_shown {
            for (_, p) in outlets(&app.hub.env.env, o, r, &ports) {
                let filled = o.kind.has_port() && !o.port.is_empty();
                let tri = vec![p + vec2(-3.0, -3.0), p + vec2(3.0, 0.0), p + vec2(-3.0, 3.0)];
                painter.add(egui::Shape::convex_polygon(tri.clone(), if filled { Color32::WHITE } else { Color32::TRANSPARENT }, Stroke::new(1.0, Color32::WHITE)));
            }
        }
    }
    // the rename field (Text tool)
    if let Some((rid, mut text)) = app.hub.env.wins[w].rename.clone() {
        if let Some(r) = rects.get(&rid) {
            let lr = label_rect(*r);
            let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(lr.expand(2.0)));
            let te = cui.add(egui::TextEdit::singleline(&mut text).desired_width(lr.width()));
            te.request_focus();
            if te.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                checkpoint(app);
                if let Some(o) = app.hub.env.env.obj_mut(rid) {
                    o.name = text.clone();
                }
                // (a trailing number: the other selected objects numbered on)
                rename_series(app, w, rid, &text);
                app.hub.env.wins[w].rename = None;
            } else {
                app.hub.env.wins[w].rename = Some((rid, text));
            }
        }
    }
    interact(app, ui, ctx, w, canvas, &resp, &objs, &rects, cable_hit, &ports);
}

fn rename_series(app: &mut App, w: usize, rid: Id, text: &str) {
    let digits: String = text.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    if digits.is_empty() {
        return;
    }
    let base = &text[..text.len() - digits.len()];
    let n0: u32 = digits.parse().unwrap_or(1);
    let s = &mut app.hub.env;
    let mut others: Vec<Id> = s.wins[w].sel.iter().copied().filter(|x| *x != rid).collect();
    others.sort_by(|a, b| {
        let p = |id: &Id| s.env.obj(*id).map_or(0.0, |o| o.y * 10000.0 + o.x);
        p(a).partial_cmp(&p(b)).unwrap_or(std::cmp::Ordering::Equal)
    });
    for (k, id) in others.iter().enumerate() {
        if let Some(o) = s.env.obj_mut(*id) {
            o.name = format!("{base}{}", n0 + 1 + k as u32);
        }
    }
}

fn draw_object(app: &App, p: &egui::Painter, o: &Obj, r: Rect, selected: bool, held: &[u8], ports: &[String]) {
    let edge = Stroke::new(1.0, if selected { look::accent() } else { Color32::from_gray(200) });
    let dim = Color32::from_gray(200);
    match o.kind {
        Kind::Macro => {
            // a frame with a grey background around its objects
            p.rect_filled(r, 3.0, Color32::from_gray(72));
            p.rect_stroke(r, 3.0, edge, egui::StrokeKind::Inside);
        }
        Kind::PhysicalInput => {
            p.rect_stroke(r, 0.0, edge, egui::StrokeKind::Inside);
            p.text(pos2(r.max.x - 6.0, r.min.y + 8.0), Align2::RIGHT_CENTER, "Sum", look::regular(10.5), dim);
            for (k, n) in ports.iter().take(64).enumerate() {
                p.text(pos2(r.max.x - 6.0, r.min.y + 17.0 + k as f32 * 9.0), Align2::RIGHT_CENTER, n.chars().take(28).collect::<String>(), look::regular(9.0), dim);
            }
        }
        Kind::Monitor => {
            p.rect_stroke(r, 0.0, edge, egui::StrokeKind::Inside);
            let log = app.hub.env.env.run.get(&o.id).map(|x| x.log.clone()).unwrap_or_default();
            let rows = ((r.height() - 4.0) / 11.0).floor().max(1.0) as usize;
            for (k, e) in log.iter().rev().take(rows).rev().enumerate() {
                p.with_clip_rect(r).text(pos2(r.min.x + 3.0, r.min.y + 2.0 + k as f32 * 11.0), Align2::LEFT_TOP, e.describe(), egui::FontId::monospace(9.5), dim);
            }
        }
        Kind::Keyboard => {
            p.rect_filled(r, 0.0, Color32::WHITE);
            let low = o.p("low").clamp(0, 120) as u8;
            let whites: Vec<u8> = (low..=127).filter(|n| ![1, 3, 6, 8, 10].contains(&(n % 12))).collect();
            let kw = 10.0;
            let n_white = ((r.width() / kw) as usize).min(whites.len());
            for (k, n) in whites.iter().take(n_white).enumerate() {
                let kr = Rect::from_min_size(pos2(r.min.x + k as f32 * kw, r.min.y), vec2(kw, r.height()));
                if held.contains(n) {
                    p.rect_filled(kr.shrink(1.0), 0.0, look::accent());
                }
                p.vline(kr.max.x, kr.y_range(), Stroke::new(1.0, Color32::from_gray(90)));
                if n % 12 == 0 {
                    p.text(pos2(kr.min.x + 2.0, kr.max.y - 2.0), Align2::LEFT_BOTTOM, envm::note_name(*n), look::regular(7.5), Color32::BLACK);
                }
                if ![4, 11].contains(&(n % 12)) && k + 1 < n_white {
                    let b = n + 1;
                    let br = Rect::from_min_size(pos2(kr.max.x - 3.0, r.min.y), vec2(6.0, r.height() * 0.6));
                    p.rect_filled(br, 0.0, if held.contains(&b) { look::accent() } else { Color32::BLACK });
                }
            }
            p.rect_stroke(r, 0.0, edge, egui::StrokeKind::Inside);
        }
        Kind::Fader => draw_fader(p, o, r, selected),
        Kind::Ornament => {
            if selected {
                p.rect_stroke(r, 4.0, edge, egui::StrokeKind::Inside);
            }
        }
        Kind::ChannelSplitter => {
            p.rect_stroke(r, 0.0, edge, egui::StrokeKind::Inside);
            p.text(r.center(), Align2::CENTER_CENTER, "Ch", look::regular(10.0), dim);
        }
        Kind::GmMixer => draw_gm(p, o, r, selected),
        Kind::ChannelStrip => draw_strip(app, p, o, r, selected),
        Kind::MmcRecord => {
            let n = ((r.width() / 20.0) as usize).max(1);
            let armed = o.p("armed");
            for k in 0..n {
                let br = Rect::from_min_size(pos2(r.min.x + k as f32 * 20.0, r.min.y), vec2(18.0, r.height()));
                p.rect_filled(br, 2.0, if armed >> k & 1 == 1 { Color32::from_rgb(0xd8, 0x3a, 0x3a) } else { Color32::from_gray(70) });
                let lab = if o.on("extra") && k < 4 { ["V", "TC", "A1", "A2"][k].to_string() } else { format!("{}", k + 1 - if o.on("extra") { 4 } else { 0 }) };
                p.text(br.center(), Align2::CENTER_CENTER, lab, look::regular(9.0), Color32::WHITE);
            }
        }
        Kind::MultiInstrument => {
            p.rect_stroke(r, 0.0, edge, egui::StrokeKind::Inside);
            let cw = r.width() / 8.0;
            let ch = (r.height() - 20.0) / 2.0;
            for k in 0..16 {
                let sr = Rect::from_min_size(pos2(r.min.x + (k % 8) as f32 * cw, r.min.y + 20.0 + (k / 8) as f32 * ch), vec2(cw, ch));
                let subsel = app.hub.env.subch == Some((o.id, k as u8 + 1));
                p.rect_stroke(sr.shrink(1.0), 0.0, Stroke::new(1.0, if subsel { look::accent() } else { Color32::from_gray(120) }), egui::StrokeKind::Inside);
                p.text(sr.center(), Align2::CENTER_CENTER, format!("{}", k + 1), look::regular(9.5), dim);
                if o.params.get(&format!("s{}.assign", k + 1)) == Some(&0) {
                    p.line_segment([sr.left_bottom(), sr.right_top()], Stroke::new(1.0, dim));
                }
            }
            icon(p, Rect::from_min_size(r.min, vec2(r.width(), 20.0)), crate::glyphs::ph::PIANO_KEYS, col(o.color));
        }
        _ => {
            let ic = match o.kind {
                Kind::SequencerInput => crate::glyphs::ph::ARROW_SQUARE_IN,
                Kind::Instrument | Kind::MappedInstrument => crate::glyphs::ph::PIANO_KEYS,
                Kind::Transformer => crate::glyphs::ph::SHUFFLE,
                Kind::Arpeggiator => crate::glyphs::ph::STAIRS,
                Kind::ChordMemorizer => crate::glyphs::ph::MUSIC_NOTES,
                Kind::DelayLine => crate::glyphs::ph::REPEAT,
                Kind::VoiceLimiter => crate::glyphs::ph::FUNNEL,
                Kind::MidiClick => crate::glyphs::ph::METRONOME,
                Kind::Alias => crate::glyphs::ph::COPY,
                Kind::TouchTracks => crate::glyphs::ph::HAND_TAP,
                _ => crate::glyphs::ph::CIRCLE,
            };
            if selected {
                p.rect_filled(r, 3.0, look::accent().gamma_multiply(0.35));
            }
            icon(p, r, ic, if o.kind == Kind::Alias { Color32::from_gray(160) } else { col(o.color) });
        }
    }
}

fn icon(p: &egui::Painter, r: Rect, ic: &str, c: Color32) {
    crate::glyphs::paint(p, r, ic, r.height().min(r.width()).min(26.0) * 0.8, c);
}

fn fader_value01(o: &Obj, v: i32) -> f32 {
    let (lo, hi) = (o.p("min"), o.p("max"));
    ((v - lo) as f32 / (hi - lo).max(1) as f32).clamp(0.0, 1.0)
}

fn draw_fader(p: &egui::Painter, o: &Obj, r: Rect, selected: bool) {
    let edge = Stroke::new(1.0, if selected { look::accent() } else { Color32::from_gray(200) });
    let v = o.p("val");
    let f = fader_value01(o, v);
    let style = match o.p("style") {
        0 if o.is_cable_switcher() => 99,
        0 if r.width() > r.height() * 2.0 => 6,
        0 if (r.width() - r.height()).abs() < 8.0 => 7,
        0 => 1,
        s => s,
    };
    let txt = envm::value_text(o, v);
    match style {
        99 => {
            // a cable switcher (auto style): its position shown
            p.rect_stroke(r, 2.0, edge, egui::StrokeKind::Inside);
            p.text(r.center(), Align2::CENTER_CENTER, format!("→ {}", v + 1), look::regular(11.0), Color32::WHITE);
        }
        6 => {
            p.rect_filled(r, 2.0, Color32::from_gray(55));
            p.rect_filled(Rect::from_min_size(r.min, vec2(r.width() * f, r.height())), 2.0, look::accent());
            p.text(r.center(), Align2::CENTER_CENTER, txt, look::regular(10.0), Color32::WHITE);
            p.rect_stroke(r, 2.0, edge, egui::StrokeKind::Inside);
        }
        7 => {
            let c = r.center();
            let rad = r.width().min(r.height()) / 2.0 - 2.0;
            p.circle_filled(c, rad, Color32::from_gray(70));
            let a = std::f32::consts::PI * (0.75 + 1.5 * f);
            p.line_segment([c, c + vec2(a.cos(), a.sin()) * rad], Stroke::new(2.0, Color32::WHITE));
            p.circle_stroke(c, rad, edge);
        }
        8 => {
            p.rect_stroke(r, 2.0, edge, egui::StrokeKind::Inside);
            p.text(r.center(), Align2::CENTER_CENTER, txt, look::regular(11.0), Color32::WHITE);
        }
        9 => {
            let on = v >= o.p("max") && o.p("max") != o.p("min");
            p.rect_filled(r, 3.0, if on { look::accent() } else { Color32::from_gray(70) });
            p.rect_stroke(r, 3.0, edge, egui::StrokeKind::Inside);
        }
        10 => {
            p.rect_stroke(r, 2.0, edge, egui::StrokeKind::Inside);
            p.text(r.center(), Align2::CENTER_CENTER, txt, look::regular(11.0), Color32::WHITE);
        }
        11 => {
            p.rect_stroke(r, 0.0, edge, egui::StrokeKind::Inside);
            let fx = fader_value01(o, o.p("val2"));
            let pt = pos2(r.min.x + r.width() * fx, r.max.y - r.height() * f);
            p.hline(r.x_range(), pt.y, Stroke::new(1.0, Color32::from_gray(140)));
            p.vline(pt.x, r.y_range(), Stroke::new(1.0, Color32::from_gray(140)));
            p.circle_filled(pt, 3.0, Color32::WHITE);
        }
        12 => {}
        _ => {
            // vertical styles (and Vertical/Mute with its button)
            let mute = style == 5;
            let fr = if mute { Rect::from_min_max(r.min, pos2(r.max.x, r.max.y - 14.0)) } else { r };
            p.rect_filled(fr, 2.0, Color32::from_gray(50));
            let y = fr.max.y - fr.height() * f;
            p.rect_filled(Rect::from_min_max(pos2(fr.min.x + 2.0, y), pos2(fr.max.x - 2.0, fr.max.y)), 1.0, look::accent().gamma_multiply(0.6));
            p.hline(fr.x_range(), y, Stroke::new(2.0, Color32::WHITE));
            p.text(pos2(fr.center().x, fr.min.y + 2.0), Align2::CENTER_TOP, txt, look::regular(9.0), Color32::WHITE);
            p.rect_stroke(fr, 2.0, edge, egui::StrokeKind::Inside);
            if mute {
                let mr = Rect::from_min_max(pos2(r.min.x, r.max.y - 12.0), r.max);
                p.rect_filled(mr, 2.0, if o.on("mute") { Color32::from_rgb(0x3a, 0x9a, 0xd8) } else { Color32::from_gray(80) });
                p.text(mr.center(), Align2::CENTER_CENTER, "M", look::regular(9.0), Color32::WHITE);
            }
        }
    }
}

const GM_ROWS: [&str; 6] = ["Prg", "Vol", "Pan", "Rev", "Cho", "Mute"];

fn draw_gm(p: &egui::Painter, o: &Obj, r: Rect, selected: bool) {
    p.rect_filled(r, 2.0, Color32::from_gray(45));
    p.rect_stroke(r, 2.0, Stroke::new(1.0, if selected { look::accent() } else { Color32::from_gray(150) }), egui::StrokeKind::Inside);
    let legend = if o.on("legend") { 30.0 } else { 0.0 };
    let n = (((r.width() - legend) / 20.0) as i32).clamp(1, 16);
    let mw = (r.width() - legend) / n as f32;
    if legend > 0.0 {
        for (k, l) in GM_ROWS.iter().enumerate() {
            p.text(pos2(r.min.x + 3.0, r.min.y + 8.0 + k as f32 * 14.0), Align2::LEFT_CENTER, *l, look::regular(8.5), Color32::from_gray(170));
        }
    }
    for m in 0..n {
        let ch = o.p("ch") - 1 + m;
        if ch > 15 {
            break;
        }
        let x = r.min.x + legend + m as f32 * mw;
        let c = |k: &str| o.params.get(&format!("{k}{ch}")).copied();
        p.text(pos2(x + mw / 2.0, r.min.y + 8.0), Align2::CENTER_CENTER, format!("{}", c("prg").unwrap_or(0) + 1), look::regular(8.0), Color32::WHITE);
        for (k, key, def) in [(1, "pan", 64), (2, "rev", 40), (3, "cho", 0)] {
            let v = c(key).unwrap_or(def) as f32 / 127.0;
            let ctr = pos2(x + mw / 2.0, r.min.y + 8.0 + (k + 1) as f32 * 14.0);
            p.circle_stroke(ctr, 5.0, Stroke::new(1.0, Color32::from_gray(150)));
            let a = std::f32::consts::PI * (0.75 + 1.5 * v);
            p.line_segment([ctr, ctr + vec2(a.cos(), a.sin()) * 5.0], Stroke::new(1.5, Color32::WHITE));
        }
        let muted = c("mute").unwrap_or(0) != 0;
        let mr = Rect::from_center_size(pos2(x + mw / 2.0, r.min.y + 78.0), vec2(mw - 4.0, 10.0));
        p.rect_filled(mr, 2.0, if muted { Color32::from_rgb(0x3a, 0x9a, 0xd8) } else { Color32::from_gray(80) });
        let fr = Rect::from_min_max(pos2(x + mw / 2.0 - 3.0, r.min.y + 90.0), pos2(x + mw / 2.0 + 3.0, r.max.y - 18.0));
        p.rect_filled(fr, 2.0, Color32::from_gray(30));
        let v = c("vol").unwrap_or(100) as f32 / 127.0;
        let y = fr.max.y - fr.height() * v;
        p.rect_filled(Rect::from_center_size(pos2(fr.center().x, y), vec2(mw - 6.0, 5.0)), 1.0, Color32::from_gray(220));
        p.text(pos2(x + mw / 2.0, r.max.y - 8.0), Align2::CENTER_CENTER, format!("{}", ch + 1), look::regular(8.5), Color32::from_gray(200));
        // the Bank field at the bottom (inspector: Bank)
        if o.on("bankrow") {
            p.text(pos2(x + mw / 2.0, r.max.y - 19.0), Align2::CENTER_CENTER, format!("B{}", c("bnk").unwrap_or(0)), look::regular(8.0), Color32::from_gray(170));
        }
    }
}

fn draw_strip(app: &App, p: &egui::Painter, o: &Obj, r: Rect, selected: bool) {
    let t = o.p("trk").max(0) as usize;
    let tr = app.project.as_ref().and_then(|pr| pr.tracks.get(t));
    p.rect_filled(r, 2.0, if selected { Color32::from_gray(200) } else { Color32::from_gray(95) });
    let text = if selected { Color32::BLACK } else { Color32::WHITE };
    let mut y = r.min.y + 4.0;
    for (show, l) in [(true, "Setting"), (o.on("eqs"), "EQ"), (o.on("ins"), "Inserts"), (o.on("sends"), "Sends"), (o.on("io"), "I/O")] {
        if show {
            p.rect_filled(Rect::from_min_size(pos2(r.min.x + 3.0, y), vec2(r.width() - 6.0, 13.0)), 2.0, Color32::from_gray(if selected { 225 } else { 120 }));
            p.text(pos2(r.center().x, y + 6.5), Align2::CENTER_CENTER, l, look::regular(8.5), text);
            y += 16.0;
        }
    }
    let db = tr.map_or(0.0, |t| t.volume_db);
    let fr = Rect::from_min_max(pos2(r.center().x - 4.0, y + 4.0), pos2(r.center().x + 4.0, r.max.y - 34.0));
    p.rect_filled(fr, 2.0, Color32::from_gray(35));
    let f = crate::automation::db_to_logic(db) / 127.0;
    let fy = fr.max.y - fr.height() * f;
    p.rect_filled(Rect::from_center_size(pos2(fr.center().x, fy), vec2(r.width() - 16.0, 8.0)), 2.0, Color32::from_gray(225));
    p.text(pos2(r.center().x, r.max.y - 28.0), Align2::CENTER_CENTER, format!("{db:+.1}"), look::regular(8.5), text);
    let (m, s) = tr.map_or((false, false), |t| (t.mute, t.solo));
    for (k, (l, on, c)) in [("M", m, Color32::from_rgb(0x3a, 0x9a, 0xd8)), ("S", s, Color32::from_rgb(0xe0, 0xc0, 0x30))].iter().enumerate() {
        let br = Rect::from_min_size(pos2(r.min.x + 6.0 + k as f32 * 24.0, r.max.y - 20.0), vec2(20.0, 14.0));
        p.rect_filled(br, 2.0, if *on { *c } else { Color32::from_gray(70) });
        p.text(br.center(), Align2::CENTER_CENTER, *l, look::regular(9.0), Color32::WHITE);
    }
    let name = tr.map(|t| t.name.clone()).unwrap_or_else(|| o.name.clone());
    p.text(pos2(r.center().x, r.max.y + 2.0), Align2::CENTER_TOP, name, look::regular(10.5), Color32::from_gray(210));
}

/// Is the object operated by the pointer (played, moved) rather than dragged?
fn playable(o: &Obj) -> bool {
    matches!(o.kind, Kind::Keyboard | Kind::Fader | Kind::Monitor | Kind::GmMixer | Kind::ChannelStrip | Kind::MmcRecord | Kind::MultiInstrument)
}

#[allow(clippy::too_many_arguments)]
fn interact(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, w: usize, canvas: Rect, resp: &egui::Response, objs: &[Obj], rects: &BTreeMap<Id, Rect>, cable_hit: Option<Cable>, ports: &[String]) {
    let m = ui.input(|i| i.modifiers);
    let tool = app.hub.env.wins[w].tool;
    let protect = app.hub.env.protect;
    let pos = resp.interact_pointer_pos().or(ctx.input(|i| i.pointer.hover_pos()));
    // topmost object under the pointer (its body or name label)
    let hit = |p: Pos2| -> Option<(Id, bool)> {
        objs.iter().rev().filter(|o| o.kind != Kind::Ornament).find_map(|o| {
            let r = rects[&o.id];
            if r.contains(p) {
                Some((o.id, true))
            } else if label_rect(r).contains(p) {
                Some((o.id, false))
            } else {
                None
            }
        }).or_else(|| objs.iter().rev().find(|o| o.kind == Kind::Ornament && rects[&o.id].contains(p)).map(|o| (o.id, false)))
    };
    let outlet_hit = |p: Pos2| -> Option<(Id, u16)> {
        objs.iter().find_map(|o| outlets(&app.hub.env.env, o, rects[&o.id], ports).into_iter().find(|(_, q)| q.distance(p) < 5.0).map(|(k, _)| (o.id, k)))
    };
    let to_layer = |p: Pos2| -> (f32, f32) {
        let q = p - canvas.min + app.hub.env.wins[w].scroll - vec2(10.0, 10.0);
        (q.x, q.y)
    };
    // double-click: object windows, or unpacking (macros: none in Studio)
    if resp.double_clicked() {
        if let Some((id, _)) = pos.and_then(hit) {
            let o = objs.iter().find(|o| o.id == id).cloned().unwrap();
            let sub = match o.kind {
                Kind::ChordMemorizer => Some(Sub::Chord),
                Kind::MappedInstrument => Some(Sub::Mapped),
                Kind::MultiInstrument => Some(Sub::Multi),
                Kind::TouchTracks => Some(Sub::Touch),
                Kind::Transformer => Some(Sub::Transformer),
                Kind::Fader if o.p("out_t") == 7 || o.p("in_t") == 7 => Some(Sub::SysEx),
                Kind::Fader if o.p("style") == 10 || o.p("as") == 7 => Some(Sub::TextFader),
                _ => None,
            };
            // a macro's empty area: unpacked into its objects and cables
            if o.kind == Kind::Macro && !protect {
                checkpoint(app);
                if app.hub.env.env.unpack_macro(id) {
                    app.hub.env.wins[w].sel.clear();
                } else {
                    app.hub.env.undo.pop();
                    app.tell("This macro is protected: turn off Protected in its inspector to unpack it");
                }
            }
            if let Some(s) = sub {
                app.hub.env.sub = Some((s, id));
            }
            if o.kind == Kind::ChannelStrip {
                app.selected_track = Some(o.p("trk").max(0) as usize);
            }
        }
        return;
    }
    if resp.drag_started() {
        let Some(p) = resp.interact_pointer_pos() else { return };
        let start = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
        // Option-click on an output triangle: a menu of destinations (see below)
        if let Some((from, outlet)) = outlet_hit(start).filter(|_| !protect && tool == Tool::Pointer && !m.alt) {
            app.hub.env.wins[w].drag = Drag::Cable { from, outlet, at: start };
            return;
        }
        if let Some((id, body)) = hit(start) {
            let o = objs.iter().find(|o| o.id == id).cloned().unwrap();
            let r = rects[&id];
            // the lower-right corner resizes
            if tool == Tool::Pointer && !protect && (r.max - start).length() < 7.0 && body {
                app.hub.env.wins[w].drag = Drag::Resize { id, start, w: o.w, h: o.h };
                return;
            }
            if tool == Tool::Pointer && body && playable(&o) && !m.shift {
                let v0 = o.p("val");
                let v1 = o.p("val2");
                operate_press(app, ctx, w, &o, r, start);
                app.hub.env.wins[w].drag = Drag::Operate { id, start, v0, v1 };
                return;
            }
            if tool == Tool::Pointer && !protect {
                let sel = &mut app.hub.env.wins[w].sel;
                if !sel.contains(&id) {
                    *sel = vec![id];
                }
                let mut ids = sel.clone();
                // (macros move with their members)
                for i in sel.clone() {
                    for m in app.hub.env.env.all_members(i) {
                        if !ids.contains(&m) {
                            ids.push(m);
                        }
                    }
                }
                let orig: Vec<(Id, f32, f32)> = ids.iter().filter_map(|i| app.hub.env.env.obj(*i).map(|o| (o.id, o.x, o.y))).collect();
                checkpoint(app);
                app.hub.env.wins[w].drag = Drag::Move { start, orig, copy: m.alt };
                return;
            }
        }
        if let Some(c) = cable_hit.filter(|_| tool == Tool::Pointer && !protect) {
            let mut cs = app.hub.env.wins[w].sel_cables.clone();
            if !cs.contains(&c) {
                cs = vec![c];
            }
            app.hub.env.wins[w].sel_cables = cs.clone();
            app.hub.env.wins[w].drag = Drag::Recable { cables: cs, at: start };
            return;
        }
        app.hub.env.wins[w].drag = Drag::Band { start, at: start };
        return;
    }
    if resp.dragged() {
        let Some(p) = resp.interact_pointer_pos() else { return };
        let snap = app.hub.env.snap;
        match app.hub.env.wins[w].drag.clone() {
            Drag::Move { start, orig, .. } => {
                let d = p - start;
                for (id, x, y) in orig {
                    if let Some(o) = app.hub.env.env.obj_mut(id) {
                        let (mut nx, mut ny) = (x + d.x, y + d.y);
                        if snap {
                            nx = (nx / GRID).round() * GRID;
                            ny = (ny / GRID).round() * GRID;
                        }
                        o.x = nx.max(0.0);
                        o.y = ny.max(0.0);
                    }
                }
            }
            Drag::Resize { id, start, w: w0, h: h0 } => {
                if let Some(o) = app.hub.env.env.obj_mut(id) {
                    let d = p - start;
                    o.w = (w0 + d.x).max(12.0);
                    o.h = (h0 + d.y).max(10.0);
                }
            }
            Drag::Cable { from, outlet, .. } => app.hub.env.wins[w].drag = Drag::Cable { from, outlet, at: p },
            Drag::Recable { cables, .. } => app.hub.env.wins[w].drag = Drag::Recable { cables, at: p },
            Drag::Band { start, .. } => app.hub.env.wins[w].drag = Drag::Band { start, at: p },
            Drag::Operate { id, start, v0, v1 } => {
                let Some(o) = app.hub.env.env.obj(id).cloned() else { return };
                operate_drag(app, ctx, &o, rects[&id], start, p, v0, v1, m.ctrl);
            }
            Drag::None => {}
        }
        // the drag in progress drawn on top
        let painter = ui.painter_at(canvas);
        match &app.hub.env.wins[w].drag {
            Drag::Cable { from, outlet, at } => {
                if let Some(o) = objs.iter().find(|o| o.id == *from) {
                    if let Some((_, a)) = outlets(&app.hub.env.env, o, rects[from], ports).into_iter().find(|x| x.0 == *outlet) {
                        painter.add(egui::Shape::line(cable_path(a, *at), Stroke::new(1.5, Color32::WHITE)));
                        if let Some((t, _)) = hit(*at) {
                            painter.rect_stroke(rects[&t].expand(2.0), 2.0, Stroke::new(2.0, look::accent()), egui::StrokeKind::Outside);
                        }
                    }
                }
            }
            Drag::Recable { at, .. } => {
                painter.circle_filled(*at, 4.0, Color32::WHITE);
            }
            Drag::Band { start, at } => {
                painter.rect_stroke(Rect::from_two_pos(*start, *at), 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
            }
            _ => {}
        }
        return;
    }
    if resp.drag_stopped() {
        let drag = std::mem::replace(&mut app.hub.env.wins[w].drag, Drag::None);
        let end = resp.interact_pointer_pos().or(ctx.input(|i| i.pointer.latest_pos()));
        match drag {
            Drag::Move { start, orig, copy } => {
                let d = end.map_or(egui::Vec2::ZERO, |e| e - start);
                if copy && d.length() > 3.0 {
                    // Option-drag: copies (the originals back in place)
                    let ids: Vec<Id> = orig.iter().map(|x| x.0).collect();
                    for (id, x, y) in &orig {
                        if let Some(o) = app.hub.env.env.obj_mut(*id) {
                            o.x = *x;
                            o.y = *y;
                        }
                    }
                    let new = app.hub.env.env.duplicate(&ids, d.x, d.y, None);
                    app.hub.env.wins[w].sel = new;
                } else if d.length() < 1.0 {
                    app.hub.env.undo.pop();
                }
                // dragged onto the other Environment window: to its layer
                if let Some(e) = end {
                    let other = 1 - w;
                    if app.hub.env.wins[other].open && app.hub.env.wins[other].canvas.contains(e) && !canvas.contains(e) {
                        let l = app.hub.env.wins[other].layer.max(2);
                        let ids: Vec<Id> = orig.iter().map(|x| x.0).collect();
                        if copy {
                            app.hub.env.env.duplicate(&ids, 0.0, 0.0, Some(l));
                        } else {
                            for id in ids {
                                if let Some(o) = app.hub.env.env.obj_mut(id) {
                                    o.layer = l;
                                }
                            }
                        }
                    }
                }
            }
            Drag::Cable { from, outlet, .. } => {
                let Some(e) = end else { return };
                // over another window's object: cable across layers
                let target = hit(e).map(|x| x.0).or_else(|| {
                    let other = 1 - w;
                    if !app.hub.env.wins[other].open || !app.hub.env.wins[other].canvas.contains(e) {
                        return None;
                    }
                    let oc = app.hub.env.wins[other].canvas;
                    let os = app.hub.env.wins[other].scroll;
                    let env = &app.hub.env.env;
                    env.on_layer(app.hub.env.wins[other].layer).into_iter().find(|id| env.obj(*id).is_some_and(|o| obj_rect(o, oc, os).contains(e)))
                });
                // dropped back on its own triangle: nothing
                if let Some(to) = target.filter(|t| *t != from) {
                    let has_port = app.hub.env.env.obj(from).is_some_and(|o| o.kind.has_port() && !o.port.is_empty() && o.port != "*");
                    if has_port {
                        app.hub.env.dlg = Some(Dlg::ReplacePort { from, outlet, to });
                    } else {
                        checkpoint(app);
                        let env = &mut app.hub.env.env;
                        if env.fixed_outlets(from) { env.connect(from, outlet, to) } else { env.connect_next(from, to) };
                    }
                }
            }
            Drag::Recable { cables, at } => {
                let target = hit(at).map(|x| x.0);
                match target {
                    // dragged back over its source: removed
                    Some(t) if cables.iter().all(|c| c.from == t) || cables.is_empty() => {
                        checkpoint(app);
                        for c in cables {
                            app.hub.env.env.disconnect(c);
                        }
                    }
                    Some(t) if cables.len() > 1 => app.hub.env.dlg = Some(Dlg::ConnectAll { cables, to: t }),
                    Some(t) => {
                        checkpoint(app);
                        let env = &mut app.hub.env.env;
                        for c in env.cables.iter_mut() {
                            if *c == cables[0] {
                                c.to = t;
                            }
                        }
                        app.hub.env.wins[w].sel_cables.clear();
                    }
                    None => {
                        if cables.first().is_some_and(|c| rects.get(&c.from).is_some_and(|r| r.expand(8.0).contains(at))) {
                            checkpoint(app);
                            for c in cables {
                                app.hub.env.env.disconnect(c);
                            }
                        }
                    }
                }
            }
            Drag::Band { start, at } => {
                // (ornaments aren't picked up by drag-selecting)
                let band = Rect::from_two_pos(start, at);
                let picked: Vec<Id> = objs.iter().filter(|o| o.kind != Kind::Ornament && band.intersects(rects[&o.id])).map(|o| o.id).collect();
                let win = &mut app.hub.env.wins[w];
                if m.shift {
                    for id in picked {
                        if !win.sel.contains(&id) {
                            win.sel.push(id);
                        }
                    }
                } else {
                    win.sel = picked;
                }
                // (selecting objects selects their cables)
                let sel = win.sel.clone();
                win.sel_cables = app.hub.env.env.cables.iter().filter(|c| sel.contains(&c.from) && sel.contains(&c.to)).copied().collect();
            }
            Drag::Operate { id, .. } => operate_release(app, ctx, id),
            _ => {}
        }
        return;
    }
    if resp.clicked() || resp.secondary_clicked() {
        let Some(p) = resp.interact_pointer_pos() else { return };
        // Option-click an output triangle: choose a destination by layer
        if let Some((from, outlet)) = outlet_hit(p).filter(|_| m.alt || resp.secondary_clicked()) {
            ui.memory_mut(|mem| mem.data.insert_temp(egui::Id::unique(("env-dest", w)), (from, outlet)));
            return;
        }
        match (tool, hit(p)) {
            (Tool::Eraser, Some((id, _))) => {
                checkpoint(app);
                app.hub.env.env.delete(&[id]);
                app.hub.env.wins[w].sel.retain(|x| *x != id);
            }
            (Tool::Eraser, None) => {
                if let Some(c) = cable_hit {
                    checkpoint(app);
                    app.hub.env.env.disconnect(c);
                }
            }
            (Tool::Text, Some((id, _))) => {
                let name = app.hub.env.env.obj(id).map(|o| o.name.clone()).unwrap_or_default();
                app.hub.env.wins[w].rename = Some((id, name));
            }
            (Tool::MidiThru, Some((id, _))) => {
                if let Some(t) = app.selected_track {
                    app.hub.env.track_obj.insert(t, id);
                    let n = app.hub.env.env.obj(id).map(|o| o.name.clone()).unwrap_or_default();
                    app.tell(format!("Track {} plays through “{n}”", t + 1));
                } else {
                    app.tell("Select a track in the main window first");
                }
            }
            (Tool::Pencil, None) => {
                let (x, y) = to_layer(p);
                add_object(app, w, Kind::Instrument, Some((x, y)));
            }
            (_, Some((id, body))) => {
                let o = objs.iter().find(|o| o.id == id).cloned().unwrap();
                // a click on a playable object's body acted already (operate)
                if body && playable(&o) && !m.shift && tool == Tool::Pointer {
                    return;
                }
                if body && o.is_cable_switcher() {
                    checkpoint(app);
                    app.hub.env.env.click_switcher(id);
                }
                let win = &mut app.hub.env.wins[w];
                if m.shift || m.command {
                    if win.sel.contains(&id) { win.sel.retain(|x| *x != id) } else { win.sel.push(id) }
                } else {
                    win.sel = vec![id];
                }
                win.sel_cables = app.hub.env.env.cables.iter().filter(|c| c.from == id).copied().collect();
                app.hub.env.subch = None;
            }
            (_, None) => {
                let win = &mut app.hub.env.wins[w];
                if let Some(c) = cable_hit {
                    if m.shift { win.sel_cables.push(c) } else { win.sel_cables = vec![c] }
                    win.sel.clear();
                } else {
                    win.sel.clear();
                    win.sel_cables.clear();
                }
            }
        }
    }
    // the Option-click destination menu
    let key = egui::Id::unique(("env-dest", w));
    let pending: Option<(Id, u16)> = ui.memory(|mem| mem.data.get_temp(key));
    if let Some((from, outlet)) = pending {
        let mut close = false;
        let at = ctx.input(|i| i.pointer.latest_pos()).unwrap_or(canvas.center());
        egui::Area::new(key.with("menu")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let env = app.hub.env.env.clone();
                for (li, l) in env.layers.iter().enumerate().skip(1) {
                    ui.menu_button(l, |ui| {
                        for o in env.objects.iter().filter(|o| o.layer == li && o.id != from) {
                            if o.kind == Kind::MultiInstrument {
                                ui.menu_button(&o.name, |ui| {
                                    if ui.button("(whole)").clicked() {
                                        app.hub.env.env.connect_next(from, o.id);
                                        close = true;
                                    }
                                });
                            } else if ui.button(&o.name).clicked() {
                                checkpoint(app);
                                let e = &mut app.hub.env.env;
                                if e.fixed_outlets(from) { e.connect(from, outlet, o.id) } else { e.connect_next(from, o.id) };
                                close = true;
                            }
                        }
                    });
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ui.memory_mut(|mem| mem.data.remove::<(Id, u16)>(key));
        }
    }
}

/// The pointer pressed on a playable object: a key plays, a button toggles, a monitor clears…
fn operate_press(app: &mut App, ctx: &egui::Context, w: usize, o: &Obj, r: Rect, p: Pos2) {
    let c = clock(app);
    match o.kind {
        Kind::Keyboard => {
            if let Some(n) = key_at(o, r, p) {
                let ch = (o.p("ch") - 1).clamp(0, 15) as u8;
                let outs = app.hub.env.env.inject(o.id, Ev::note(ch, n, o.p("vel") as u8), c);
                dispatch(app, ctx, outs);
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("env-key-down", o.id)), n));
            }
        }
        Kind::Monitor => {
            if let Some(r) = app.hub.env.env.run.get_mut(&o.id) {
                r.log.clear();
            }
        }
        Kind::Fader => {
            let style = o.p("style");
            if o.p("style") == 5 && p.y > r.max.y - 14.0 {
                // Vertical/Mute: the Mute button (sends 0, then holds; off resends the value)
                let mute = !o.on("mute");
                app.hub.env.env.obj_mut(o.id).unwrap().set("mute", mute as i32);
                let mut f = o.clone();
                f.set("mute", 0);
                let v = if mute { 0 } else { o.p("val") };
                let evs = envm::fader_output(&f, v, false);
                let mut outs = Vec::new();
                for e in evs {
                    outs.extend(app.hub.env.env.inject(o.id, e, c));
                }
                dispatch(app, ctx, outs);
            } else if style == 9 {
                // a button: off ↔ on (equal minimum and maximum: that value each click)
                let (lo, hi) = (o.p("min"), o.p("max"));
                let v = if lo == hi || o.p("val") != hi { hi } else { lo };
                let outs = app.hub.env.env.move_fader(o.id, v, c);
                dispatch(app, ctx, outs);
            } else if o.is_cable_switcher() {
                checkpoint(app);
                app.hub.env.env.click_switcher(o.id);
            } else if style == 10 && o.on("menu") {
                app.hub.env.sub = Some((Sub::TextFader, o.id));
            }
        }
        Kind::MultiInstrument => {
            // a subchannel's square: selected (its parameters in the inspector); the top: the
            // whole instrument
            if p.y < r.min.y + 20.0 {
                app.hub.env.subch = None;
            } else {
                let cw = r.width() / 8.0;
                let ch = (r.height() - 20.0) / 2.0;
                let k = ((p.x - r.min.x) / cw).floor().clamp(0.0, 7.0) as u8 + 8 * ((p.y - r.min.y - 20.0) / ch).floor().clamp(0.0, 1.0) as u8;
                app.hub.env.subch = Some((o.id, k + 1));
            }
            app.hub.env.wins[w].sel = vec![o.id];
        }
        Kind::MmcRecord => {
            let k = ((p.x - r.min.x) / 20.0).floor().max(0.0) as i32;
            let armed = o.p("armed") ^ (1 << k);
            app.hub.env.env.obj_mut(o.id).unwrap().set("armed", armed);
            let extra = o.on("extra");
            let mask = if extra { (armed >> 4) as u32 } else { armed as u32 };
            dispatch(app, ctx, vec![Out::MmcArm { mask }]);
        }
        Kind::GmMixer => gm_press(app, ctx, o, r, p),
        Kind::ChannelStrip => {
            let t = o.p("trk").max(0) as usize;
            if p.y > r.max.y - 22.0 {
                let mute = p.x < r.min.x + 28.0;
                app.checkpoint(if mute { "Mute" } else { "Solo" });
                if let Some(tr) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)) {
                    if mute { tr.mute = !tr.mute } else { tr.solo = !tr.solo }
                }
                app.sync_mix();
            }
            app.hub.env.wins[w].sel = vec![o.id];
        }
        _ => {}
    }
}

fn key_at(o: &Obj, r: Rect, p: Pos2) -> Option<u8> {
    let low = o.p("low").clamp(0, 120) as u8;
    let whites: Vec<u8> = (low..=127).filter(|n| ![1, 3, 6, 8, 10].contains(&(n % 12))).collect();
    let kw = 10.0;
    let k = ((p.x - r.min.x) / kw).floor() as usize;
    let n = *whites.get(k)?;
    // (the black keys: the upper part between whites)
    if p.y < r.min.y + r.height() * 0.6 {
        let fx = (p.x - r.min.x) / kw - k as f32;
        if fx > 0.7 && ![4, 11].contains(&(n % 12)) {
            return Some(n + 1);
        }
        if fx < 0.3 && k > 0 && ![0, 5].contains(&(n % 12)) {
            return Some(n - 1);
        }
    }
    Some(n)
}

#[allow(clippy::too_many_arguments)]
fn operate_drag(app: &mut App, ctx: &egui::Context, o: &Obj, r: Rect, start: Pos2, p: Pos2, v0: i32, v1: i32, fine: bool) {
    let c = clock(app);
    match o.kind {
        Kind::Fader if !o.is_cable_switcher() && !matches!(o.p("style"), 9 | 10) => {
            let (lo, hi) = (o.p("min"), o.p("max"));
            let span = (hi - lo).max(1) as f32;
            if o.p("style") == 11 {
                // vector: the crosshair follows the pointer
                let fx = ((p.x - r.min.x) / r.width()).clamp(0.0, 1.0);
                let fy = ((r.max.y - p.y) / r.height()).clamp(0.0, 1.0);
                let outs = app.hub.env.env.move_vector(o.id, lo + (fy * span).round() as i32, lo + (fx * span).round() as i32, c);
                dispatch(app, ctx, outs);
                return;
            }
            let horiz = o.p("style") == 6 || (o.p("style") == 0 && r.width() > r.height() * 2.0);
            let d = if horiz { p.x - start.x } else { start.y - p.y };
            let len = if horiz { r.width() } else { r.height() };
            // (Control-drag: single steps; numeric fields: by pixels)
            let per = if fine || o.p("style") == 8 { 0.25 } else { span / len.max(10.0) };
            let v = (v0 as f32 + d * per).round() as i32;
            let v = v.clamp(lo.min(hi), hi.max(lo));
            if v != o.p("val") {
                let outs = app.hub.env.env.move_fader(o.id, v, c);
                dispatch(app, ctx, outs);
            }
            let _ = v1;
        }
        Kind::Keyboard => {
            // gliding over the keys plays them in turn
            let key = egui::Id::unique(("env-key-down", o.id));
            let cur: Option<u8> = ctx.data(|d| d.get_temp(key));
            let n = key_at(o, r, p);
            if n != cur {
                let ch = (o.p("ch") - 1).clamp(0, 15) as u8;
                let mut outs = Vec::new();
                if let Some(old) = cur {
                    outs.extend(app.hub.env.env.inject(o.id, Ev::new(0x80, ch, old, 0), c));
                }
                if let Some(nn) = n {
                    outs.extend(app.hub.env.env.inject(o.id, Ev::note(ch, nn, o.p("vel") as u8), c));
                    ctx.data_mut(|d| d.insert_temp(key, nn));
                }
                dispatch(app, ctx, outs);
            }
        }
        Kind::GmMixer => gm_drag(app, ctx, o, r, start, p),
        Kind::ChannelStrip => {
            let t = o.p("trk").max(0) as usize;
            if let Some(tr) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)) {
                let l = (crate::automation::db_to_logic(tr.volume_db) + (start.y - p.y) * 0.02).clamp(0.0, 127.0);
                tr.volume_db = crate::automation::logic_to_db(l);
                app.sync_mix();
            }
        }
        _ => {}
    }
}

fn operate_release(app: &mut App, ctx: &egui::Context, id: Id) {
    let key = egui::Id::unique(("env-key-down", id));
    let cur: Option<u8> = ctx.data(|d| d.get_temp(key));
    if let (Some(n), Some(o)) = (cur, app.hub.env.env.obj(id).cloned()) {
        let c = clock(app);
        let ch = (o.p("ch") - 1).clamp(0, 15) as u8;
        let outs = app.hub.env.env.inject(id, Ev::new(0x80, ch, n, 0), c);
        dispatch(app, ctx, outs);
        ctx.data_mut(|d| d.remove::<u8>(key));
    }
}

/// The GM mixer module and row under the pointer.
fn gm_at(o: &Obj, r: Rect, p: Pos2) -> Option<(i32, usize)> {
    let legend = if o.on("legend") { 30.0 } else { 0.0 };
    let n = (((r.width() - legend) / 20.0) as i32).clamp(1, 16);
    let mw = (r.width() - legend) / n as f32;
    let m = ((p.x - r.min.x - legend) / mw).floor() as i32;
    if m < 0 || m >= n {
        return None;
    }
    let ch = o.p("ch") - 1 + m;
    let y = p.y - r.min.y;
    let row = if o.on("bankrow") && p.y > r.max.y - 25.0 && p.y < r.max.y - 13.0 { 6 } else if y < 15.0 { 0 } else if y < 29.0 { 2 } else if y < 43.0 { 3 } else if y < 57.0 { 4 } else if y < 84.0 { 5 } else { 1 };
    (ch <= 15).then_some((ch, row))
}

fn gm_send(app: &mut App, ctx: &egui::Context, o: &Obj, ev: Ev) {
    let c = clock(app);
    let outs = app.hub.env.env.inject(o.id, ev, c);
    dispatch(app, ctx, outs);
}

fn gm_press(app: &mut App, ctx: &egui::Context, o: &Obj, r: Rect, p: Pos2) {
    let Some((ch, row)) = gm_at(o, r, p) else { return };
    match row {
        // the Program button: the GM sound names (one step per click; the menu in the inspector)
        0 => {
            let v = (o.params.get(&format!("prg{ch}")).copied().unwrap_or(0) + 1) % 128;
            app.hub.env.env.obj_mut(o.id).unwrap().params.insert(format!("prg{ch}"), v);
            gm_send(app, ctx, o, Ev::new(0xC0, ch as u8, v as u8, 0));
            app.tell(format!("Channel {}: {}", ch + 1, envm::GM_PROGRAMS[v as usize]));
        }
        // the Bank field: steps the bank (Bank Select MSB)
        6 => {
            let v = (o.params.get(&format!("bnk{ch}")).copied().unwrap_or(0) + 1) % 128;
            app.hub.env.env.obj_mut(o.id).unwrap().params.insert(format!("bnk{ch}"), v);
            gm_send(app, ctx, o, Ev::new(0xB0, ch as u8, 0, v as u8));
        }
        // Mute: the channel volume to 0 and back to the fader's value
        5 => {
            let key = format!("mute{ch}");
            let m = o.params.get(&key).copied().unwrap_or(0) == 0;
            app.hub.env.env.obj_mut(o.id).unwrap().params.insert(key, m as i32);
            let vol = if m { 0 } else { o.params.get(&format!("vol{ch}")).copied().unwrap_or(100) };
            gm_send(app, ctx, o, Ev::new(0xB0, ch as u8, 7, vol as u8));
        }
        _ => {}
    }
}

fn gm_drag(app: &mut App, ctx: &egui::Context, o: &Obj, r: Rect, start: Pos2, p: Pos2) {
    let Some((ch, row)) = gm_at(o, r, start) else { return };
    let (key, cc, def) = match row {
        1 => ("vol", 7, 100),
        // (each knob's controller is chosen in the inspector: Pan, Reverb, Chorus by default)
        2 => ("pan", o.p("k1").clamp(0, 127) as u8, 64),
        3 => ("rev", o.p("k2").clamp(0, 127) as u8, 40),
        4 => ("cho", o.p("k3").clamp(0, 127) as u8, 0),
        _ => return,
    };
    let k = format!("{key}{ch}");
    let base_key = egui::Id::unique(("gm-base", o.id, ch, row));
    let base: i32 = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(base_key, || o.params.get(&k).copied().unwrap_or(def)));
    let v = (base as f32 + (start.y - p.y) * 0.6).round().clamp(0.0, 127.0) as i32;
    if o.params.get(&k).copied().unwrap_or(def) != v {
        app.hub.env.env.obj_mut(o.id).unwrap().params.insert(k, v);
        gm_send(app, ctx, o, Ev::new(0xB0, ch as u8, cc, v as u8));
    }
    if ctx.input(|i| i.pointer.any_released()) {
        ctx.data_mut(|d| d.remove::<i32>(base_key));
    }
    let _ = r;
}

// ---- confirmations ----

fn dialogs(app: &mut App, ctx: &egui::Context) {
    let Some(d) = app.hub.env.dlg.clone() else { return };
    let mut close = false;
    egui::Window::new("Environment").id(egui::Id::unique("env-dlg")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -60.0)).show(ctx, |ui| {
        match d {
            Dlg::DeleteLayer(l) => {
                ui.label(format!("Delete the layer “{}”? Its objects will be deleted.", app.hub.env.env.layers.get(l).cloned().unwrap_or_default()));
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("Delete").clicked() {
                        checkpoint(app);
                        let s = &mut app.hub.env;
                        s.env.delete_layer(l);
                        for win in s.wins.iter_mut() {
                            if win.layer >= s.env.layers.len() || win.layer == l {
                                win.layer = 2.min(s.env.layers.len() - 1);
                            }
                        }
                        close = true;
                    }
                });
            }
            Dlg::RenameLayer(l, mut name) => {
                ui.label("Layer name:");
                let r = ui.text_edit_singleline(&mut name);
                r.request_focus();
                app.hub.env.dlg = Some(Dlg::RenameLayer(l, name.clone()));
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        checkpoint(app);
                        if let Some(x) = app.hub.env.env.layers.get_mut(l) {
                            *x = name.clone();
                        }
                        close = true;
                    }
                });
            }
            Dlg::ReplacePort { from, outlet, to } => {
                ui.label("This object has a direct output to a MIDI port. Replace it with the cable?");
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    let no = ui.button("No").on_hover_text("Keep the port too (two destinations)").clicked();
                    let remove = ui.button("Remove").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if no || remove {
                        checkpoint(app);
                        let env = &mut app.hub.env.env;
                        if env.fixed_outlets(from) { env.connect(from, outlet, to) } else { env.connect_next(from, to) };
                        if remove {
                            if let Some(o) = env.obj_mut(from) {
                                o.port.clear();
                            }
                        }
                        close = true;
                    }
                });
            }
            Dlg::ConnectAll { cables, to } => {
                ui.label(format!("Connect all {} selected cables to this object?", cables.len()));
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("Connect").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        checkpoint(app);
                        for c in app.hub.env.env.cables.iter_mut() {
                            if cables.contains(c) {
                                c.to = to;
                            }
                        }
                        close = true;
                    }
                });
            }
            Dlg::PasteReplace => {
                ui.label("Replace the selected objects with the ones on the clipboard? Their cabling stays.");
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("Paste").clicked() {
                        let w = app.hub.env.front;
                        app.hub.env.wins[w].sel.clear();
                        paste(app, w, false);
                        close = true;
                    }
                    if ui.button("Replace").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let w = app.hub.env.front;
                        paste(app, w, true);
                        close = true;
                    }
                });
            }
            Dlg::ImportCustom(src) => {
                ui.label("Import Environment ▸ Custom: what happens to each of this project's objects");
                let key = egui::Id::unique("env-custom");
                let mut choice: BTreeMap<Id, i64> = ui.data(|d| d.get_temp(key)).unwrap_or_default();
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    egui::Grid::new("env-custom-grid").num_columns(2).show(ui, |ui| {
                        for o in app.hub.env.env.objects.iter() {
                            ui.label(&o.name);
                            let c = choice.entry(o.id).or_insert(-1);
                            let label = match *c {
                                -1 => "Keep".to_string(),
                                -2 => "Delete".to_string(),
                                x => src.obj(x as Id).map_or("?".into(), |s| format!("→ {}", s.name)),
                            };
                            egui::ComboBox::from_id_salt(("env-custom", o.id)).selected_text(label).show_ui(ui, |ui| {
                                ui.selectable_value(c, -1, "Keep");
                                ui.selectable_value(c, -2, "Delete");
                                for s in src.objects.iter().filter(|s| s.kind == o.kind) {
                                    ui.selectable_value(c, s.id as i64, &s.name);
                                }
                            });
                            ui.end_row();
                        }
                    });
                });
                ui.horizontal(|ui| {
                    // Assign by Identical / Unique ID / Name: fills the choices
                    if ui.button("Assign by Name").clicked() {
                        for o in &app.hub.env.env.objects {
                            if let Some(s) = src.objects.iter().find(|s| s.kind == o.kind && similar(&s.name, &o.name) >= 0.8) {
                                choice.insert(o.id, s.id as i64);
                            }
                        }
                    }
                    if ui.button("Assign by Port/MIDI Channel").clicked() {
                        for o in &app.hub.env.env.objects {
                            if let Some(s) = src.objects.iter().find(|s| s.kind == o.kind && s.port == o.port && s.p("ch") == o.p("ch")) {
                                choice.insert(o.id, s.id as i64);
                            }
                        }
                    }
                    if ui.button("Assign by Unique ID").clicked() {
                        for o in &app.hub.env.env.objects {
                            if src.obj(o.id).is_some_and(|s| s.kind == o.kind) {
                                choice.insert(o.id, o.id as i64);
                            }
                        }
                    }
                    if ui.button("Assign as Keep").clicked() {
                        choice.clear();
                    }
                });
                ui.data_mut(|d| d.insert_temp(key, choice.clone()));
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("Import").clicked() {
                        checkpoint(app);
                        let env = &mut app.hub.env.env;
                        let mut del = Vec::new();
                        for (id, c) in &choice {
                            match *c {
                                -2 => del.push(*id),
                                x if x >= 0 => {
                                    if let (Some(s), Some(o)) = (src.obj(x as Id).cloned(), env.obj_mut(*id)) {
                                        let (oid, l, ox, oy) = (o.id, o.layer, o.x, o.y);
                                        *o = s;
                                        o.id = oid;
                                        o.layer = l;
                                        o.x = ox;
                                        o.y = oy;
                                    }
                                }
                                _ => {}
                            }
                        }
                        env.delete(&del);
                        close = true;
                    }
                });
            }
        }
    });
    if close {
        app.hub.env.dlg = None;
    }
}

/// Name similarity (0–1), for Assign by Name's 80 percent.
fn similar(a: &str, b: &str) -> f32 {
    let (a, b): (Vec<char>, Vec<char>) = (a.to_lowercase().chars().collect(), b.to_lowercase().chars().collect());
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + (a[i - 1] != b[j - 1]) as usize);
        }
    }
    1.0 - d[a.len()][b.len()] as f32 / a.len().max(b.len()) as f32
}

// ---- object windows ----

fn sub_windows(app: &mut App, ctx: &egui::Context) {
    let Some((kind, id)) = app.hub.env.sub else { return };
    let Some(o) = app.hub.env.env.obj(id).cloned() else {
        app.hub.env.sub = None;
        return;
    };
    let mut open = true;
    let title = match kind {
        Sub::Chord => format!("Chord Memorizer: {}", o.name),
        Sub::Mapped => format!("Mapped Instrument: {}", o.name),
        Sub::Multi => format!("Multi Instrument: {}", o.name),
        Sub::Touch => format!("Touch Tracks: {}", o.name),
        Sub::Transformer => format!("Transformer: {}", o.name),
        Sub::TextFader => format!("Text Fader: {}", o.name),
        Sub::Banks => format!("Custom Bank Messages: {}", o.name),
        Sub::SysEx => format!("SysEx Fader: {}", o.name),
    };
    egui::Window::new(title).id(egui::Id::unique(("env-sub", id))).open(&mut open).resizable(true).default_size(vec2(560.0, 380.0)).show(ctx, |ui| match kind {
        Sub::Chord => chord_window(app, ui, id),
        Sub::Mapped => mapped_window(app, ui, ctx, id),
        Sub::Multi => multi_window(app, ui, ctx, id),
        Sub::Touch => touch_window(app, ui, id),
        Sub::Transformer => transformer_window(app, ui, id),
        Sub::TextFader => text_fader_window(app, ui, ctx, id),
        Sub::Banks => banks_window(app, ui, id),
        Sub::SysEx => crate::envsysex::window(app, ui, id),
    });
    if !open {
        app.hub.env.sub = None;
        app.hub.env.listen = false;
    }
}

/// The Chord Memorizer window: the input note on the top keyboard, its chord on the lower one
/// (in any octave); Listen records chords played on a MIDI keyboard.
fn chord_window(app: &mut App, ui: &mut egui::Ui, id: Id) {
    let o = app.hub.env.env.obj(id).cloned().unwrap();
    let key = egui::Id::unique(("cm-input", id));
    let input: usize = ui.data(|d| d.get_temp(key)).unwrap_or(0);
    ui.horizontal(|ui| {
        let mut l = app.hub.env.listen;
        if ui.checkbox(&mut l, "Listen").changed() {
            app.hub.env.listen = l;
            app.hub.env.sub_sel.clear();
        }
        ui.label(format!("Input: {}", ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"][input]));
    });
    let draw_keys = |ui: &mut egui::Ui, lit: &dyn Fn(i32) -> bool, octaves: i32, base: i32, salt: &str| -> Option<i32> {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
        let p = ui.painter_at(r);
        let whites: Vec<i32> = (0..12 * octaves).filter(|n| ![1, 3, 6, 8, 10].contains(&(n % 12))).collect();
        let kw = r.width() / whites.len() as f32;
        let mut clicked = None;
        for (k, n) in whites.iter().enumerate() {
            let kr = Rect::from_min_size(pos2(r.min.x + k as f32 * kw, r.min.y), vec2(kw, r.height()));
            p.rect_filled(kr.shrink(0.5), 1.0, if lit(*n + base) { look::accent() } else { Color32::WHITE });
            if resp.clicked() && resp.interact_pointer_pos().is_some_and(|q| kr.contains(q)) {
                clicked = Some(*n + base);
            }
        }
        for (k, n) in whites.iter().enumerate() {
            if [4, 11].contains(&(n % 12)) || k + 1 >= whites.len() {
                continue;
            }
            let br = Rect::from_min_size(pos2(r.min.x + (k as f32 + 0.65) * kw, r.min.y), vec2(kw * 0.7, r.height() * 0.6));
            p.rect_filled(br, 1.0, if lit(n + 1 + base) { look::accent() } else { Color32::BLACK });
            if resp.clicked() && resp.interact_pointer_pos().is_some_and(|q| br.contains(q)) {
                clicked = Some(n + 1 + base);
            }
        }
        let _ = salt;
        clicked
    };
    ui.label("Input note");
    let chord = o.chord(input);
    if let Some(n) = draw_keys(ui, &|n| n == input as i32, 1, 0, "top") {
        ui.data_mut(|d| d.insert_temp(key, n.rem_euclid(12) as usize));
    }
    ui.label("Chord");
    // (the chord's notes relative to the input note, shown from two octaves below)
    let lit = |n: i32| chord.contains(&(n - input as i32));
    if let Some(n) = draw_keys(ui, &lit, 4, -24, "bottom") {
        let off = n - input as i32;
        let mut c = chord.clone();
        if c.contains(&off) { c.retain(|x| *x != off) } else { c.push(off) }
        checkpoint(app);
        app.hub.env.env.obj_mut(id).unwrap().set_chord(input, &c);
    }
    if app.hub.env.listen {
        ui.label(egui::RichText::new("Listen: play the chord on your MIDI keyboard (one note at a time or together)").small().color(look::text_dim()));
        // (the notes held at the Physical Input's keyboard object, or recorded as they come)
        let held: Vec<u8> = app.live.held.keys().copied().collect();
        for n in held {
            if !app.hub.env.sub_sel.contains(&n) {
                app.hub.env.sub_sel.push(n);
                // the chord relative to the input key's C in the lowest note's octave
                let lowest = *app.hub.env.sub_sel.iter().min().unwrap() as i32;
                let base = lowest - (lowest - input as i32).rem_euclid(12);
                let rel: Vec<i32> = app.hub.env.sub_sel.iter().map(|x| *x as i32 - base).collect();
                app.hub.env.env.obj_mut(id).unwrap().set_chord(input, &rel);
            }
        }
        ui.ctx().request_repaint();
    }
}

/// The Mapped Instrument window: one row per input note — name, output note, velocity offset,
/// channel, cable, head, staff position, group; Initialize choices.
fn mapped_window(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, id: Id) {
    let mut o = app.hub.env.env.obj(id).cloned().unwrap();
    let before = o.clone();
    let sel_key = egui::Id::unique(("map-sel", id));
    let mut sel: Vec<u8> = ui.data(|d| d.get_temp(sel_key)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.menu_button("Initialize ⌄", |ui| {
            let targets: Vec<u8> = if sel.is_empty() { (0..128).collect() } else { sel.clone() };
            if ui.button("Names as Notes").clicked() {
                for n in &targets {
                    let mut r = o.map_row(*n);
                    r.name = envm::note_name(*n);
                    o.set_map_row(*n, &r);
                }
            }
            if ui.button("Names as General MIDI").clicked() {
                for n in &targets {
                    let mut r = o.map_row(*n);
                    if (35..=81).contains(n) {
                        r.name = envm::GM_DRUMS[(*n - 35) as usize].into();
                    }
                    o.set_map_row(*n, &r);
                }
            }
            if ui.button("Output Notes").clicked() {
                for n in &targets {
                    let mut r = o.map_row(*n);
                    r.out = *n;
                    o.set_map_row(*n, &r);
                }
            }
            if ui.button("Output Velocities").clicked() {
                for n in 0..128 {
                    let mut r = o.map_row(n);
                    r.vel = 0;
                    o.set_map_row(n, &r);
                }
            }
            if ui.button("Output Channels").clicked() {
                for n in &targets {
                    let mut r = o.map_row(*n);
                    r.ch = 0;
                    o.set_map_row(*n, &r);
                }
            }
            if ui.button("Output Cables").clicked() {
                for n in 0..128 {
                    let mut r = o.map_row(n);
                    r.cable = 1;
                    o.set_map_row(n, &r);
                }
            }
            if ui.button("Score Parameters").clicked() {
                for n in &targets {
                    let mut r = o.map_row(*n);
                    r.head = 0;
                    r.relpos = 0;
                    r.group.clear();
                    o.set_map_row(*n, &r);
                }
            }
        });
        ui.label(egui::RichText::new("Click a key to play it; Shift-click to add notes to the selection").small().color(look::text_dim()));
    });
    egui::ScrollArea::vertical().max_height(ui.available_height() - 4.0).show(ui, |ui| {
        egui::Grid::new(("map-grid", id)).striped(true).num_columns(9).show(ui, |ui| {
            for h in ["Input", "Name", "Output Note", "Velocity", "Channel", "Cable", "Head", "Rel. Pos.", "Group"] {
                ui.strong(h);
            }
            ui.end_row();
            for n in (0..128u8).rev() {
                let mut r = o.map_row(n);
                let on = sel.contains(&n);
                let b = ui.selectable_label(on, envm::note_name(n));
                if b.clicked() {
                    if ui.input(|i| i.modifiers.shift) {
                        if on { sel.retain(|x| *x != n) } else { sel.push(n) }
                    } else {
                        sel = vec![n];
                    }
                    // (the input note plays)
                    let c = clock(app);
                    let outs = app.hub.env.env.deliver(id, Ev::note(0, n, 100), c);
                    let offs = app.hub.env.env.deliver(id, Ev::new(0x80, 0, n, 0), Clock { ms: c.ms + 1.0, ..c });
                    dispatch(app, ctx, outs);
                    dispatch(app, ctx, offs);
                }
                let mut name = r.name.clone();
                if ui.add(egui::TextEdit::singleline(&mut name).desired_width(110.0).char_limit(12)).changed() {
                    r.name = name;
                }
                let mut out = r.out as i32;
                ui.add(egui::DragValue::new(&mut out).range(0..=127).custom_formatter(|x, _| envm::note_name(x as u8)));
                r.out = out as u8;
                ui.add(egui::DragValue::new(&mut r.vel).range(-99..=99));
                let mut ch = r.ch as i32;
                ui.add(egui::DragValue::new(&mut ch).range(0..=16).custom_formatter(|x, _| if x == 0.0 { "Base".into() } else { format!("{x:.0}") }));
                r.ch = ch as u8;
                let mut cable = r.cable as i32;
                ui.add(egui::DragValue::new(&mut cable).range(1..=16));
                r.cable = cable as u8;
                let mut head = r.head as i32;
                ui.add(egui::DragValue::new(&mut head).range(0..=12));
                r.head = head as u8;
                let mut rp = r.relpos as i32;
                ui.add(egui::DragValue::new(&mut rp).range(-24..=24));
                r.relpos = rp as i8;
                egui::ComboBox::from_id_salt(("map-grp", id, n)).selected_text(if r.group.is_empty() { "—".to_string() } else { r.group.clone() }).width(70.0).show_ui(ui, |ui| {
                    for g in ["", "Kick", "Snare", "Toms", "Hi-Hat", "Cymbals", "Percussion"] {
                        ui.selectable_value(&mut r.group, g.to_string(), if g.is_empty() { "—" } else { g });
                    }
                });
                if r != o.map_row(n) {
                    // (edits apply to every selected note)
                    let targets: Vec<u8> = if sel.contains(&n) { sel.clone() } else { vec![n] };
                    let old = o.map_row(n);
                    for t in targets {
                        let mut x = o.map_row(t);
                        if r.name != old.name && t == n {
                            x.name = r.name.clone();
                        }
                        if r.out != old.out {
                            x.out = if t == n { r.out } else { (x.out as i32 + r.out as i32 - old.out as i32).clamp(0, 127) as u8 };
                        }
                        if r.vel != old.vel {
                            x.vel = r.vel;
                        }
                        if r.ch != old.ch {
                            x.ch = r.ch;
                        }
                        if r.cable != old.cable {
                            x.cable = r.cable;
                        }
                        if r.head != old.head {
                            x.head = r.head;
                        }
                        if r.relpos != old.relpos {
                            x.relpos = r.relpos;
                        }
                        if r.group != old.group {
                            x.group = r.group.clone();
                        }
                        o.set_map_row(t, &x);
                    }
                }
                ui.end_row();
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(sel_key, sel));
    if o != before {
        checkpoint(app);
        *app.hub.env.env.obj_mut(id).unwrap() = o;
    }
}

/// The Multi-Instrument window: device names, the bank's 128 program names (double-click to
/// edit), Options (Init Names as Numbers / General MIDI Names, copy / paste the bank), the 16
/// subchannels' Program boxes; clicking a name sends the program change.
fn multi_window(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, id: Id) {
    let mut o = app.hub.env.env.obj(id).cloned().unwrap();
    let before = o.clone();
    let bank_key = egui::Id::unique(("multi-bank", id));
    let mut bank: usize = ui.data(|d| d.get_temp(bank_key)).unwrap_or(0);
    ui.horizontal(|ui| {
        ui.label("Device Name:");
        let mut n = o.texts.get("device").cloned().unwrap_or_else(|| o.name.clone());
        if ui.add(egui::TextEdit::singleline(&mut n).desired_width(140.0)).changed() {
            o.texts.insert("device".into(), n);
        }
        ui.label("Short Device Name:");
        let mut s = o.texts.get("short").cloned().unwrap_or_default();
        if ui.add(egui::TextEdit::singleline(&mut s).desired_width(60.0)).changed() {
            o.texts.insert("short".into(), s);
        }
    });
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(("multi-bank-sel", id)).selected_text(if bank == 15 { "No Bank specified".to_string() } else { format!("Bank {bank}") }).show_ui(ui, |ui| {
            for b in 0..15 {
                ui.selectable_value(&mut bank, b, format!("Bank {b}"));
            }
            ui.selectable_value(&mut bank, 15, "No Bank specified");
        });
        let b = if bank == 15 { 0 } else { bank };
        if b > 0 && !o.texts.contains_key(&format!("progs{b}")) {
            if ui.button("Initialize this bank").on_hover_text("Uninitialized banks use bank 0's names").clicked() {
                let names: Vec<String> = (0..128).map(|k| o.program_name(0, k)).collect();
                o.texts.insert(format!("progs{b}"), names.join("\n"));
            }
        }
        ui.menu_button("Options ⌄", |ui| {
            if ui.button("Init Names as Numbers").clicked() {
                o.texts.insert(format!("progs{b}"), (1..=128).map(|k| k.to_string()).collect::<Vec<_>>().join("\n"));
            }
            if ui.button("Init General MIDI Names").clicked() {
                o.texts.insert(format!("progs{b}"), envm::GM_PROGRAMS.join("\n"));
            }
            if ui.button("Copy Bank Names").clicked() {
                let t: Vec<String> = (0..128).map(|k| o.program_name(b, k)).collect();
                ui.ctx().copy_text(t.join("\n"));
            }
            if ui.button("Paste Bank Names").clicked() {
                let key = egui::Id::unique("multi-paste");
                ui.ctx().data_mut(|d| d.insert_temp(key, (id, b)));
            }
        });
        let mut gm = o.on("gmdrums");
        if ui.checkbox(&mut gm, "Use GM Drum Program Names for Channel 10").changed() {
            o.set("gmdrums", gm as i32);
        }
    });
    let b = if bank == 15 { 0 } else { bank };
    // subchannel Program boxes: the track list shows the program; clicking a name sends it
    ui.horizontal_wrapped(|ui| {
        ui.label("Program:");
        for k in 1..=16 {
            let key = format!("s{k}.prog_on");
            let mut on = o.p(&key) != 0;
            if ui.checkbox(&mut on, format!("{k}")).changed() {
                o.set(&key, on as i32);
            }
        }
    });
    let edit_key = egui::Id::unique(("multi-edit", id));
    let editing: Option<(usize, String)> = ui.data(|d| d.get_temp(edit_key));
    let sub = app.hub.env.subch.filter(|s| s.0 == id).map_or(1, |s| s.1);
    egui::ScrollArea::vertical().max_height(ui.available_height() - 4.0).show(ui, |ui| {
        egui::Grid::new(("multi-grid", id)).num_columns(4).striped(true).show(ui, |ui| {
            for k in 0..128 {
                let name = o.program_name(b, k);
                if editing.as_ref().is_some_and(|e| e.0 == k) {
                    let mut t = editing.as_ref().unwrap().1.clone();
                    let r = ui.text_edit_singleline(&mut t);
                    r.request_focus();
                    if r.lost_focus() {
                        let mut names: Vec<String> = (0..128).map(|x| o.program_name(b, x)).collect();
                        names[k] = t.clone();
                        o.texts.insert(format!("progs{b}"), names.join("\n"));
                        ui.data_mut(|d| d.remove::<(usize, String)>(edit_key));
                    } else {
                        ui.data_mut(|d| d.insert_temp(edit_key, (k, t)));
                    }
                } else {
                    let r = ui.selectable_label(o.p(&format!("s{sub}.prog")) == k as i32, format!("{:>3} {name}", k + 1));
                    if r.double_clicked() {
                        ui.data_mut(|d| d.insert_temp(edit_key, (k, name.clone())));
                    } else if r.clicked() && o.p(&format!("s{sub}.prog_on")) != 0 {
                        o.set(&format!("s{sub}.prog"), k as i32);
                        o.set(&format!("s{sub}.bank"), b as i32);
                        let pre = format!("s{sub}");
                        for ev in envm::instrument_setup(&o, &pre) {
                            if let Some(bytes) = ev.bytes() {
                                app.hub.outs.send(&o.port, &bytes);
                            }
                        }
                    }
                }
                if k % 4 == 3 {
                    ui.end_row();
                }
            }
        });
    });
    // (Paste Bank Names: from the clipboard text, empty lines skipped)
    let pk = egui::Id::unique("multi-paste");
    if let Some((pid, pb)) = ctx.data(|d| d.get_temp::<(Id, usize)>(pk)) {
        if pid == id {
            let pasted: Option<String> = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Paste(t) = e { Some(t.clone()) } else { None }));
            if let Some(t) = pasted {
                let lines: Vec<&str> = t.lines().filter(|l| !l.trim().is_empty()).take(128).collect();
                let mut names: Vec<String> = (0..128).map(|x| o.program_name(pb, x)).collect();
                for (k, l) in lines.iter().enumerate() {
                    names[k] = l.to_string();
                }
                o.texts.insert(format!("progs{pb}"), names.join("\n"));
                ctx.data_mut(|d| d.remove::<(Id, usize)>(pk));
            }
        }
    }
    ui.data_mut(|d| d.insert_temp(bank_key, bank));
    if o != before {
        checkpoint(app);
        *app.hub.env.env.obj_mut(id).unwrap() = o;
    }
}

/// The Touch Tracks window: a row per input note with its region, group, transposition,
/// velocity, trigger mode, start and delay. Regions are assigned to the selected keys from the
/// selected region in the Tracks area.
fn touch_window(app: &mut App, ui: &mut egui::Ui, id: Id) {
    let mut o = app.hub.env.env.obj(id).cloned().unwrap();
    let before = o.clone();
    let sel_key = egui::Id::unique(("tt-sel", id));
    let mut sel: Vec<u8> = ui.data(|d| d.get_temp(sel_key)).unwrap_or_default();
    ui.horizontal(|ui| {
        let region = app.selected_region;
        if ui.add_enabled(region.is_some(), egui::Button::new("Assign Selected Region")).on_hover_text("To the selected keys; with none selected: every key, C3 at the original pitch").clicked() {
            if let Some((t, r)) = region {
                let name = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.get(r)).map(|x| x.name.clone()).unwrap_or_default();
                if sel.is_empty() {
                    // every note triggers it: C3 original, the others transposed by distance
                    o.set_touch_row(60, Some(&envm::TouchRow { region: Some((t, r)), name, group: 0, transpose: 0, velocity: 0, trigger: 0, start: 0, delay: 0 }));
                    for n in 0..60 {
                        o.set_touch_row(n, None);
                    }
                    let row = o.touch_row(60).unwrap();
                    for n in 0..128u8 {
                        if n != 60 {
                            let mut x = row.clone();
                            x.transpose = n as i32 - 60;
                            o.set_touch_row(n, Some(&x));
                        }
                    }
                } else {
                    // a range: incremental transpositions from its lowest key
                    let lo = *sel.iter().min().unwrap();
                    for n in &sel {
                        o.set_touch_row(*n, Some(&envm::TouchRow { region: Some((t, r)), name: name.clone(), group: 0, transpose: if sel.len() > 1 { (*n - lo) as i32 } else { 0 }, velocity: 0, trigger: 0, start: 0, delay: 0 }));
                    }
                }
            }
        }
        if ui.button("Clear Selected").clicked() {
            for n in &sel {
                o.set_touch_row(*n, None);
            }
        }
    });
    egui::ScrollArea::vertical().max_height(ui.available_height() - 4.0).show(ui, |ui| {
        egui::Grid::new(("tt-grid", id)).striped(true).num_columns(8).show(ui, |ui| {
            for h in ["Key", "Region/Folder", "Group", "Transpos", "Velocity", "Trigger", "Start", "Delay"] {
                ui.strong(h);
            }
            ui.end_row();
            for n in (0..128u8).rev() {
                let on = sel.contains(&n);
                if ui.selectable_label(on, envm::note_name(n)).clicked() {
                    if ui.input(|i| i.modifiers.shift) {
                        if on { sel.retain(|x| *x != n) } else { sel.push(n) }
                    } else {
                        sel = vec![n];
                    }
                }
                match o.touch_row(n) {
                    None => {
                        // (a grey line: same as the row above)
                        ui.label(egui::RichText::new("│").color(Color32::from_gray(110)));
                        for _ in 0..6 {
                            ui.label("");
                        }
                    }
                    Some(mut r) => {
                        ui.label(&r.name);
                        let mut g = r.group as i32;
                        ui.add(egui::DragValue::new(&mut g).range(0..=16).custom_formatter(|x, _| if x == 0.0 { "Off".into() } else { format!("{x:.0}") }));
                        r.group = g as u8;
                        ui.add(egui::DragValue::new(&mut r.transpose).range(-96..=96));
                        egui::ComboBox::from_id_salt(("tt-vel", id, n)).selected_text(["100%", "50%", "Off"][r.velocity.min(2) as usize]).width(56.0).show_ui(ui, |ui| {
                            for (k, l) in ["100%", "50%", "Off"].iter().enumerate() {
                                ui.selectable_value(&mut r.velocity, k as u8, *l);
                            }
                        });
                        egui::ComboBox::from_id_salt(("tt-trig", id, n)).selected_text(envm::TOUCH_TRIGGER[r.trigger.min(5) as usize]).width(84.0).show_ui(ui, |ui| {
                            for (k, l) in envm::TOUCH_TRIGGER.iter().enumerate() {
                                ui.selectable_value(&mut r.trigger, k as u8, *l);
                            }
                        });
                        egui::ComboBox::from_id_salt(("tt-start", id, n)).selected_text(envm::TOUCH_START[r.start.min(3) as usize]).width(76.0).show_ui(ui, |ui| {
                            for (k, l) in envm::TOUCH_START.iter().enumerate() {
                                ui.selectable_value(&mut r.start, k as u8, *l);
                            }
                        });
                        ui.add(egui::DragValue::new(&mut r.delay).range(-3840..=3840).suffix(" ticks"));
                        if Some(&r) != o.touch_row(n).as_ref() {
                            o.set_touch_row(n, Some(&r));
                        }
                    }
                }
                ui.end_row();
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(sel_key, sel));
    if o != before {
        checkpoint(app);
        *app.hub.env.env.obj_mut(id).unwrap() = o;
    }
}

/// SysEx mapper mode: the message's length, checksum and fixed byte values in the operations'
/// place (matching events: data byte 1 the position, data byte 2 the value; channel 1 sends
/// the changed message, 2 doesn't send it, 3 sends it unchanged; 4–16 mean nothing).
fn sysex_mapper_fields(ui: &mut egui::Ui, o: &mut Obj) {
    let mut bytes = envm::sysex_map(o);
    ui.label("SysEx Message");
    ui.horizontal(|ui| {
        ui.label("Length:");
        let mut n = bytes.len() as i32;
        if ui.add(egui::DragValue::new(&mut n).range(1..=64)).changed() {
            bytes.resize(n as usize, 0);
        }
        ui.label("Checksum:");
        let mut c = o.p("sx_sum").max(0) as usize;
        egui::ComboBox::from_id_salt(("sxm-sum", o.id)).selected_text(lg_core::sysexfader::CHECKSUMS[c.min(5)]).show_ui(ui, |ui| {
            for (k, n) in lg_core::sysexfader::CHECKSUMS.iter().enumerate() {
                ui.selectable_value(&mut c, k, *n);
            }
        });
        o.params.insert("sx_sum".into(), c as i32);
        let mut f = o.on("sx_filter");
        ui.checkbox(&mut f, "Filter non-matching");
        o.params.insert("sx_filter".into(), f as i32);
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("F0").monospace().color(look::text_dim()));
        for (k, b) in bytes.iter_mut().enumerate() {
            ui.add(egui::DragValue::new(b).range(0..=127).custom_formatter(|x, _| format!("{:02X}", x as u8))).on_hover_text(format!("Byte {k}"));
        }
        if o.p("sx_sum") > 0 {
            ui.label(egui::RichText::new("SUM").monospace().color(look::text_dim()));
        }
        ui.label(egui::RichText::new("F7").monospace().color(look::text_dim()));
    });
    o.params.insert("sx_len".into(), bytes.len() as i32);
    o.texts.insert("sxmap".into(), bytes.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(" "));
    ui.label(egui::RichText::new("Channel 1: send the changed message · 2: change only · 3: send unchanged").small().color(look::text_dim()));
}

/// The Transformer window: Mode, conditions and operations (the Object inspector's rows laid
/// out as Logic's window), and the map.
fn transformer_window(app: &mut App, ui: &mut egui::Ui, id: Id) {
    let mut o = app.hub.env.env.obj(id).cloned().unwrap();
    let before = o.clone();
    let mut mode = o.p("mode");
    egui::ComboBox::from_id_salt(("tr-mode", id)).selected_text(envm::TR_MODES[mode as usize]).width(420.0).show_ui(ui, |ui| {
        for (k, m) in envm::TR_MODES.iter().enumerate() {
            ui.selectable_value(&mut mode, k as i32, *m);
        }
    });
    o.set("mode", mode);
    ui.separator();
    let row = |ui: &mut egui::Ui, o: &mut Obj, title: &str, keys: [&str; 4], cond: bool| {
        ui.label(title);
        egui::Grid::new(("tr", id, title)).num_columns(4).show(ui, |ui| {
            for (k, h) in ["Status", "Channel", "Data Byte 1", "Data Byte 2"].iter().enumerate() {
                ui.vertical(|ui| {
                    ui.strong(*h);
                    let key = keys[k];
                    let opts: &[&str] = if k == 0 { if cond { &envm::TR_COND[..3] } else { &["Thru", "Fix"] } } else if cond { envm::TR_COND } else { envm::TR_OPS };
                    let mut v = o.p(key);
                    egui::ComboBox::from_id_salt(("tr-op", id, key)).selected_text(opts.get(v as usize).copied().unwrap_or("")).width(80.0).show_ui(ui, |ui| {
                        for (j, n) in opts.iter().enumerate() {
                            ui.selectable_value(&mut v, j as i32, *n);
                        }
                    });
                    o.set(key, v);
                    if k == 0 {
                        if v != 0 {
                            let sk = format!("{key}v");
                            let mut s = o.p(&sk);
                            egui::ComboBox::from_id_salt(("tr-st", id, key)).selected_text(envm::TR_STATUS[s as usize]).width(80.0).show_ui(ui, |ui| {
                                for (j, n) in envm::TR_STATUS.iter().enumerate().skip(1) {
                                    ui.selectable_value(&mut s, j as i32, *n);
                                }
                            });
                            o.set(&sk, s);
                        }
                    } else if v != 0 {
                        for suf in ["a", "b"] {
                            let vk = format!("{key}{suf}");
                            let mut x = o.p(&vk);
                            ui.add(egui::DragValue::new(&mut x).range(-127..=127));
                            o.set(&vk, x);
                        }
                        if !cond && matches!(v, 7 | 8 | 9 | 15) {
                            let fk = format!("{key}f");
                            let mut x = o.p(&fk);
                            ui.add(egui::DragValue::new(&mut x).range(0..=1000).suffix(" %"));
                            o.set(&fk, x);
                        }
                    }
                });
            }
        });
    };
    if mode != 6 {
        row(ui, &mut o, "Conditions", ["c_st", "c_ch", "c_d1", "c_d2"], true);
        if mode == 7 {
            sysex_mapper_fields(ui, &mut o);
        } else if mode != 2 {
            row(ui, &mut o, "Operations", ["o_st", "o_ch", "o_d1", "o_d2"], false);
        }
    }
    // the map (Use Map on data byte 2; meta 122 / 123)
    let map = envm::transformer_map(&o);
    ui.label("Map");
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 100.0), Sense::click_and_drag());
    let p = ui.painter_at(r);
    p.rect_filled(r, 0.0, Color32::from_gray(30));
    let bw = r.width() / 128.0;
    for (k, v) in map.iter().enumerate() {
        let h = r.height() * *v as f32 / 127.0;
        p.rect_filled(Rect::from_min_max(pos2(r.min.x + k as f32 * bw, r.max.y - h), pos2(r.min.x + (k + 1) as f32 * bw, r.max.y)), 0.0, look::accent().gamma_multiply(0.7));
    }
    if let Some(q) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        let k = (((q.x - r.min.x) / bw) as usize).min(127);
        let v = ((r.max.y - q.y) / r.height() * 127.0).round().clamp(0.0, 127.0) as u8;
        let mut m = map;
        m[k] = v;
        o.texts.insert("map".into(), m.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","));
    }
    if ui.button("Reset Map").clicked() {
        o.texts.remove("map");
    }
    if o != before {
        checkpoint(app);
        *app.hub.env.env.obj_mut(id).unwrap() = o;
    }
}

/// The Text Fader window: a name per value; clicking one sends it, double-clicking edits it;
/// Options cut / copy / paste the list; Behave as Menu.
fn text_fader_window(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, id: Id) {
    let mut o = app.hub.env.env.obj(id).cloned().unwrap();
    let before = o.clone();
    let (lo, hi) = (o.p("min"), o.p("max"));
    let mut names = o.names();
    let n = (hi - lo + 1).clamp(1, 128) as usize;
    names.resize(n, String::new());
    for (k, nm) in names.iter_mut().enumerate() {
        if nm.is_empty() {
            *nm = format!("{}", lo + k as i32);
        }
    }
    ui.horizontal(|ui| {
        let mut menu = o.on("menu");
        if ui.checkbox(&mut menu, "Behave as Menu").changed() {
            o.set("menu", menu as i32);
        }
        ui.menu_button("Options ⌄", |ui| {
            if ui.button("Cut").clicked() {
                ui.ctx().copy_text(names.join("\n"));
                names = (0..n).map(|k| format!("{}", lo + k as i32)).collect();
            }
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(names.join("\n"));
            }
            if ui.button("Paste").clicked() {
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique("tf-paste"), id));
            }
        });
    });
    if ctx.data(|d| d.get_temp::<Id>(egui::Id::unique("tf-paste"))) == Some(id) {
        let pasted: Option<String> = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Paste(t) = e { Some(t.clone()) } else { None }));
        if let Some(t) = pasted {
            // (empty or space-only lines are ignored)
            let lines: Vec<String> = t.lines().filter(|l| !l.trim().is_empty()).map(|l| l.to_string()).collect();
            for (k, l) in lines.into_iter().take(n).enumerate() {
                names[k] = l;
            }
            ctx.data_mut(|d| d.remove::<Id>(egui::Id::unique("tf-paste")));
        }
    }
    let edit_key = egui::Id::unique(("tf-edit", id));
    let editing: Option<(usize, String)> = ui.data(|d| d.get_temp(edit_key));
    let mut send = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for k in 0..n {
            if editing.as_ref().is_some_and(|e| e.0 == k) {
                let mut t = editing.as_ref().unwrap().1.clone();
                let r = ui.text_edit_singleline(&mut t);
                r.request_focus();
                if r.lost_focus() {
                    names[k] = t;
                    ui.data_mut(|d| d.remove::<(usize, String)>(edit_key));
                } else {
                    ui.data_mut(|d| d.insert_temp(edit_key, (k, t)));
                }
                continue;
            }
            let r = ui.selectable_label(o.p("val") == lo + k as i32, format!("{:>3}  {}", lo + k as i32, names[k]));
            if r.double_clicked() {
                ui.data_mut(|d| d.insert_temp(edit_key, (k, names[k].clone())));
            } else if r.clicked() {
                send = Some(lo + k as i32);
            }
        }
    });
    o.texts.insert("names".into(), names.join("\n"));
    if o != before {
        checkpoint(app);
        *app.hub.env.env.obj_mut(id).unwrap() = o;
    }
    if let Some(v) = send {
        let c = clock(app);
        let outs = app.hub.env.env.move_fader(id, v, c);
        dispatch(app, ctx, outs);
    }
}

/// Options ▸ Define Custom Bank Messages: per bank (0–15), a list of events sent in its place.
fn banks_window(app: &mut App, ui: &mut egui::Ui, id: Id) {
    let mut o = app.hub.env.env.obj(id).cloned().unwrap();
    let before = o.clone();
    let mut rows: Vec<(i32, i32, i32, i32)> = o.texts.get("banks").map(|t| t.lines().filter_map(|l| {
        let f: Vec<&str> = l.split('\t').collect();
        Some((f.first()?.parse().ok()?, i32::from_str_radix(f.get(1)?.trim_start_matches("0x"), 16).ok()?, f.get(2)?.parse().ok()?, f.get(3)?.parse().ok()?))
    }).collect()).unwrap_or_default();
    ui.label(egui::RichText::new("By default each bank sends Control 0 with the bank number. Events listed here replace it; they are sent in order.").small().color(look::text_dim()));
    let mut del = None;
    egui::Grid::new(("banks", id)).num_columns(5).striped(true).show(ui, |ui| {
        for h in ["Bank", "Event", "Num", "Val", ""] {
            ui.strong(h);
        }
        ui.end_row();
        for (k, r) in rows.iter_mut().enumerate() {
            ui.add(egui::DragValue::new(&mut r.0).range(0..=15));
            egui::ComboBox::from_id_salt(("bank-ev", id, k)).selected_text(match r.1 {
                0xB0 => "Control",
                0xC0 => "Program",
                0xF0 => "SysEx",
                _ => "Note",
            }).width(80.0).show_ui(ui, |ui| {
                for (st, n) in [(0xB0, "Control"), (0xC0, "Program"), (0x90, "Note")] {
                    ui.selectable_value(&mut r.1, st, n);
                }
            });
            ui.add(egui::DragValue::new(&mut r.2).range(0..=127));
            ui.add(egui::DragValue::new(&mut r.3).range(0..=127));
            if ui.small_button("−").clicked() {
                del = Some(k);
            }
            ui.end_row();
        }
    });
    if let Some(k) = del {
        rows.remove(k);
    }
    if ui.button("Add Event").clicked() {
        rows.push((0, 0xB0, 0, 0));
    }
    rows.sort_by_key(|r| r.0);
    let t = rows.iter().map(|r| format!("{}\t{:02X}\t{}\t{}", r.0, r.1, r.2, r.3)).collect::<Vec<_>>().join("\n");
    if t.is_empty() {
        o.texts.remove("banks");
    } else {
        o.texts.insert("banks".into(), t);
    }
    if o != before {
        checkpoint(app);
        *app.hub.env.env.obj_mut(id).unwrap() = o;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_similar() {
        assert!(similar("Piano 1", "Piano 2") >= 0.8);
        assert!(similar("Bass", "Strings") < 0.8);
    }

    #[test]
    fn cable_paths() {
        let p = cable_path(pos2(0.0, 0.0), pos2(100.0, 50.0));
        assert_eq!(p.len(), 4);
        assert!(near_path(pos2(50.0, 25.0), &p));
        let back = cable_path(pos2(100.0, 0.0), pos2(0.0, 50.0));
        assert_eq!(back.len(), 6);
    }
}
