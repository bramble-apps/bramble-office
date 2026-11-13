//! Instrument editors (Logic's instrument plug-in windows): the Sampler (samplerwin), Quick
//! Sampler (quickwin), Drum Machine Designer (dmdwin), Drum Kit Designer (dkdwin) and Alchemy's
//! arpeggiator / effects (alchemywin) open in their own windows instead of the plain Controls view.
//!
//! What the editors change beyond parameters (a Sampler's groups and zones, the pads' samples)
//! is kept here by track id and handed to the running instrument whenever it was rebuilt, so
//! nothing is lost when the engine reloads. Each window has its own undo history (Logic's
//! plug-in undo: ⌘Z / ⇧⌘Z with the pointer over the window), its key commands (the Sampler's 36,
//! the Drum Machine Designer's 16) and takes files dropped on it.

use std::collections::HashMap;
use std::path::PathBuf;

use lg_synth::sampler::Sampler;
use lg_synth::sampler_doc::SamplerDoc;

use crate::app::App;

/// The Logic key commands the instrument editors run (with the pointer over their window, or
/// by name).
pub const COMMANDS: &[&str] = &[
    // Sampler
    "Load or Replace Audio File…",
    "New Zone",
    "New Group",
    "Toggle Keyboard/Group/Zone View",
    "Switch to Keyboard View",
    "Switch to Group View",
    "Switch to Zone View",
    "Show all List Columns",
    "Restore Default List Columns",
    "Shift selected Zones/Groups Left",
    "Shift selected Zones/Groups Right",
    "Shift selected Zones/Groups Left (Zones incl. Root Key)",
    "Shift selected Zones/Groups Right (Zones incl. Root Key)",
    "Load Multiple Audio Files…",
    "Open in Audio File Editor",
    "Select Zone of Last Played Key",
    "New Mapping",
    "Duplicate Group",
    "Crop Sample",
    "Crop Loop",
    "Toggle Snap mode",
    "Vertical Autozoom",
    "Horizontal Autozoom",
    "Automap using Current Root Note",
    "Automap using Root Note from Audio File Names",
    "Remap All Notes",
    "Remap Evenly",
    "Remap To Round Robin",
    "Remap Notes…",
    "Remap To Root Notes",
    "Fill Gaps",
    "Fill Gaps (Notes Only)",
    "Fill Gaps (Vel Only)",
    "Swap Zones",
    "Always Move Root Key with Zone",
    "Preview Selected Zone",
    // Drum Machine Designer
    "Select Next Pad",
    "Select Previous Pad",
    "Preview Pad",
    "Mute Pad",
    "Solo Pad",
    "Resample Pad",
    "Assign To Exclusive Group: Off",
    "Assign To Exclusive Group: 1",
    "Assign To Exclusive Group: 2",
    "Assign To Exclusive Group: 3",
    "Assign To Exclusive Group: 4",
    "Assign To Exclusive Group: 5",
    "Assign To Exclusive Group: 6",
    "Assign To Exclusive Group: 7",
    "Assign To Exclusive Group: 8",
    "Clear Pad",
    // Sampler instruments, plug-in settings
    "Next Sampler Instrument",
    "Previous Sampler Instrument",
    "Next Plug-in Setting or Sampler Instrument",
    "Previous Plug-in Setting or Sampler Instrument",
];

/// Which editor an instrument gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sampler,
    Quick,
    Dmd,
    Dkd,
    Alchemy,
    /// A synthesizer with its own panel (synthwin).
    Synth(crate::synthwin::Synth),
}

/// An editor's state as it was (for its undo).
#[derive(Clone)]
pub struct Snap {
    pub doc: Option<SamplerDoc>,
    pub params: Vec<(u32, f32)>,
    pub pads: Option<crate::dmdwin::Pads>,
    pub quick: Option<crate::quickwin::QuickDoc>,
    pub alch: Option<lg_synth::alchemy_zones::ZoneDoc>,
}

#[derive(Default)]
pub struct State {
    /// Sampler instruments by track id.
    pub docs: HashMap<u64, SamplerDoc>,
    /// Each track's editor revision (bumped on every change) and what the engine has.
    pub rev: HashMap<u64, u64>,
    /// The parameters set in the editors, by track id (re-applied after a rebuild).
    pub params: HashMap<u64, Vec<(u32, f32)>>,
    /// Drum Machine Designer pads (samples, names, mute / solo) by track id.
    pub pads: HashMap<u64, crate::dmdwin::Pads>,
    /// Quick Sampler samples by track id.
    pub quick: HashMap<u64, crate::quickwin::QuickDoc>,
    pub undo: Vec<(String, u64, Snap)>,
    pub redo: Vec<(String, u64, Snap)>,
    /// The editor window's rectangle (keys and drops over it go to the editor).
    pub rect: Option<egui::Rect>,
    /// Files dropped on the editor window this frame, with the pointer.
    pub dropped: Vec<PathBuf>,
    pub drop_pos: Option<egui::Pos2>,
    pub sampler: crate::samplerwin::Ui,
    pub dmd: crate::dmdwin::Ui,
    pub quick_ui: crate::quickwin::Ui,
    pub synth_clip: Option<Vec<(u32, f32)>>,
    pub last_played: Option<u8>,
    pub next_rev: u64,
    /// The Settings pop-up's Search Filter (term, on).
    pub search: (String, bool),
    /// A drag is going on (one undo step for the whole gesture).
    pub gesture: bool,
    /// Alchemy's imported audio (granular / additive / spectral sources) by track id.
    pub alch_audio: HashMap<u64, (PathBuf, std::sync::Arc<lg_synth::wav::Audio>)>,
    /// Alchemy's sample zones and groups (the source edit window) by track id.
    pub alch_zones: HashMap<u64, lg_synth::alchemy_zones::ZoneDoc>,
    /// What each track's running instrument last got: (the instance, the revision).
    pub applied: HashMap<u64, (usize, u64)>,
    /// The synthesizers' current setting name by track id (synthui's Setting pop-up).
    pub synth_setting: HashMap<u64, String>,
}

/// The track's id (stable through reorders).
pub fn track_id(app: &App, t: usize) -> Option<u64> {
    app.project.as_ref()?.tracks.get(t).map(|tr| tr.extra.id)
}

/// Runs `f` on the track's running instrument as its concrete type.
pub fn with_inst<T: 'static, R>(app: &App, t: usize, f: impl FnOnce(&mut T) -> R) -> Option<R> {
    let mut e = app.engine.lock().ok()?;
    let ins = e.units.get_mut(t)?.instrument.as_mut()?;
    ins.as_any_mut()?.downcast_mut::<T>().map(f)
}

/// Which editor the track's instrument has (None: the Controls view).
pub fn kind_of(app: &App, t: usize) -> Option<Kind> {
    let e = app.engine.lock().ok()?;
    let ins = e.units.get(t)?.instrument.as_ref()?;
    let any = ins.as_any()?;
    if any.is::<Sampler>() {
        Some(Kind::Sampler)
    } else if any.is::<lg_synth::quick::QuickSampler>() {
        Some(Kind::Quick)
    } else if any.is::<lg_synth::dmd::DrumMachineDesigner>() {
        Some(Kind::Dmd)
    } else if any.is::<lg_synth::drums::StudioDrumKit>() {
        Some(Kind::Dkd)
    } else if any.is::<lg_synth::alchemy::Alchemy>() {
        Some(Kind::Alchemy)
    } else {
        crate::synthwin::kind_of(any).map(Kind::Synth)
    }
}

/// The parameter values of the track's instrument now.
pub fn values(app: &App, t: usize) -> Option<(Vec<lg_dsp::Param>, Vec<f32>)> {
    app.engine.lock().ok().and_then(|e| e.instrument_params(t))
}

/// Sets instrument parameters (and remembers them for rebuilds).
pub fn set_params(app: &mut App, t: usize, set: &[(u32, f32)]) {
    if set.is_empty() {
        return;
    }
    if let Ok(mut e) = app.engine.lock() {
        for &(id, v) in set {
            e.set_instrument_param(t, id, v);
        }
    }
    if let Some(id) = track_id(app, t) {
        let list = app.inst.params.entry(id).or_default();
        for &(pid, v) in set {
            if let Some(x) = list.iter_mut().find(|x| x.0 == pid) {
                x.1 = v;
            } else {
                list.push((pid, v));
            }
        }
    }
    app.dirty = true;
}

/// A new revision for the track's editor state: the engine picks it up next frame.
pub fn bump(app: &mut App, t: usize) {
    if let Some(id) = track_id(app, t) {
        app.inst.next_rev += 1;
        let r = app.inst.next_rev;
        app.inst.rev.insert(id, r);
        app.dirty = true;
    }
    sync(app);
}

fn snap(app: &App, id: u64, t: usize) -> Snap {
    let params = values(app, t).map(|(p, v)| p.iter().zip(v).map(|(p, v)| (p.id, v)).collect()).unwrap_or_default();
    Snap { doc: app.inst.docs.get(&id).cloned(), params, pads: app.inst.pads.get(&id).cloned(), quick: app.inst.quick.get(&id).cloned(), alch: app.inst.alch_zones.get(&id).cloned() }
}

/// Before an editor change: the state as it is goes on the plug-in undo list.
pub fn checkpoint(app: &mut App, t: usize, what: &str) {
    let Some(id) = track_id(app, t) else { return };
    let s = snap(app, id, t);
    app.inst.undo.push((what.to_string(), id, s));
    if app.inst.undo.len() > 100 {
        app.inst.undo.remove(0);
    }
    app.inst.redo.clear();
}

fn restore(app: &mut App, id: u64, s: Snap) {
    let Some(t) = app.project.as_ref().and_then(|p| p.tracks.iter().position(|tr| tr.extra.id == id)) else { return };
    match s.doc {
        Some(d) => {
            app.inst.docs.insert(id, d);
        }
        None => {
            app.inst.docs.remove(&id);
        }
    }
    match s.pads {
        Some(d) => {
            app.inst.pads.insert(id, d);
        }
        None => {
            app.inst.pads.remove(&id);
        }
    }
    match s.quick {
        Some(d) => {
            app.inst.quick.insert(id, d);
        }
        None => {
            app.inst.quick.remove(&id);
        }
    }
    match s.alch {
        Some(d) => {
            app.inst.alch_zones.insert(id, d);
        }
        None => {
            app.inst.alch_zones.remove(&id);
        }
    }
    let p = s.params.clone();
    app.inst.params.insert(id, s.params);
    if let Ok(mut e) = app.engine.lock() {
        for (pid, v) in p {
            e.set_instrument_param(t, pid, v);
        }
    }
    bump(app, t);
}

/// A checkpoint once per gesture (a drag of a field changes it every frame).
pub fn checkpoint_once(app: &mut App, ctx: &egui::Context, t: usize, what: &str) {
    if app.inst.gesture {
        return;
    }
    checkpoint(app, t, what);
    if ctx.input(|i| i.pointer.any_down()) {
        app.inst.gesture = true;
    }
}

/// Plug-in undo (the window under the pointer).
pub fn undo(app: &mut App) -> bool {
    let Some((what, id, s)) = app.inst.undo.pop() else { return false };
    if let Some(t) = app.project.as_ref().and_then(|p| p.tracks.iter().position(|tr| tr.extra.id == id)) {
        let cur = snap(app, id, t);
        app.inst.redo.push((what.clone(), id, cur));
    }
    restore(app, id, s);
    app.tell(format!("Undo {what}"));
    true
}

pub fn redo(app: &mut App) -> bool {
    let Some((what, id, s)) = app.inst.redo.pop() else { return false };
    if let Some(t) = app.project.as_ref().and_then(|p| p.tracks.iter().position(|tr| tr.extra.id == id)) {
        let cur = snap(app, id, t);
        app.inst.undo.push((what.clone(), id, cur));
    }
    restore(app, id, s);
    app.tell(format!("Redo {what}"));
    true
}

/// Each frame: instruments rebuilt by the engine (a new instance) or edited (a new revision)
/// get their editor state and parameters back.
pub fn sync(app: &mut App) {
    crate::sidechain::sync(app);
    let Some(p) = app.project.as_ref() else { return };
    let ids: Vec<(usize, u64)> = p.tracks.iter().enumerate().map(|(i, t)| (i, t.extra.id)).filter(|(_, id)| app.inst.rev.contains_key(id) || app.inst.params.contains_key(id)).collect();
    if ids.is_empty() {
        return;
    }
    let Ok(mut e) = app.engine.lock() else { return };
    let tempo = e.tempo;
    for (t, id) in ids {
        let rev = app.inst.rev.get(&id).copied().unwrap_or(0);
        let Some(ins) = e.units.get_mut(t).and_then(|u| u.instrument.as_mut()) else { continue };
        let ptr = (&**ins) as *const dyn lg_dsp::Instrument as *const u8 as usize;
        if app.inst.applied.get(&id) == Some(&(ptr, rev)) {
            continue;
        }
        app.inst.applied.insert(id, (ptr, rev));
        if let Some(any) = ins.as_any_mut() {
            if let Some(s) = any.downcast_mut::<Sampler>() {
                if let Some(doc) = app.inst.docs.get(&id) {
                    let (zones, rules) = doc.compile();
                    s.set_zones(zones);
                    s.set_groups(rules);
                    s.name = doc.name.clone();
                }
                s.doc_rev = rev;
            } else if let Some(d) = any.downcast_mut::<lg_synth::dmd::DrumMachineDesigner>() {
                if let Some(pads) = app.inst.pads.get(&id) {
                    crate::dmdwin::apply(pads, d);
                }
                d.doc_rev = rev;
            } else if let Some(a) = any.downcast_mut::<lg_synth::alchemy::Alchemy>() {
                if let Some((_, au)) = app.inst.alch_audio.get(&id) {
                    crate::alchemywin::apply(au, a);
                }
                a.set_zones(std::sync::Arc::new(app.inst.alch_zones.get(&id).cloned().unwrap_or_default()));
            } else if let Some(q) = any.downcast_mut::<lg_synth::quick::QuickSampler>() {
                if let Some(qd) = app.inst.quick.get(&id) {
                    crate::quickwin::apply(qd, q);
                }
                q.doc_rev = rev;
            }
        }
        if let Some(list) = app.inst.params.get(&id) {
            for &(pid, v) in list {
                ins.set(pid, v);
            }
        }
        ins.set_tempo(tempo);
    }
}

/// Files dropped this frame: the editor window takes them when the pointer is over it.
pub fn takes_drop(app: &mut App, ctx: &egui::Context, files: &[PathBuf]) -> bool {
    let Some(r) = app.inst.rect else { return false };
    let pos = ctx.input(|i| i.pointer.hover_pos().or(i.pointer.latest_pos()));
    if app.plugin_window.is_none() || !pos.is_some_and(|p| r.contains(p)) {
        return false;
    }
    app.inst.dropped = files.to_vec();
    app.inst.drop_pos = pos;
    true
}

/// Files being dragged over the window (the drop zones show).
pub fn dragging_files(ctx: &egui::Context) -> bool {
    ctx.input(|i| !i.raw.hovered_files.is_empty())
}

/// The editor window for the track's instrument; false when it has none (the Controls view).
/// Checks (headless): LG_INSTRUMENT="Sampler" puts that instrument on the first track and opens
/// its window; LG_INSTRUMENT_FILES="a.wav|b.wav" adds the files (Sampler: chromatic zones; Quick
/// Sampler, Drum Machine Designer: the sample / the first pads).
fn test_hook(app: &mut App) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let Ok(name) = std::env::var("LG_INSTRUMENT") else { return };
    if app.project.is_none() || DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let t = 0;
    app.instrument_choice.insert(t, name.clone());
    app.load_engine();
    let Some(id) = track_id(app, t) else { return };
    let files: Vec<PathBuf> = std::env::var("LG_INSTRUMENT_FILES").map(|s| s.split('|').filter(|x| !x.is_empty()).map(PathBuf::from).collect()).unwrap_or_default();
    match name.as_str() {
        "Sampler" => {
            let mut d = SamplerDoc::new();
            let zs = d.add_files(&files, lg_synth::sampler_doc::Drop::Chromatic);
            app.inst.sampler.sel_zones = zs.clone();
            app.inst.sampler.focus_zone = zs.first().copied();
            app.inst.sampler.sel_groups = vec![d.groups.len() - 1];
            app.inst.sampler.focus_group = Some(d.groups.len() - 1);
            app.inst.docs.insert(id, d);
        }
        "Quick Sampler" => {
            if let Some(a) = files.first().and_then(|f| lg_synth::wav::read_audio(f).ok()) {
                app.inst.quick.insert(id, crate::quickwin::QuickDoc { file: files.first().cloned(), audio: Some(std::sync::Arc::new(a)), history: files.first().cloned().into_iter().collect(), ..Default::default() });
            }
        }
        "Alchemy" => {
            // the files as one group of source A's zones, played by its Sampler element
            let fs: Vec<_> = files.iter().filter_map(|f| lg_synth::wav::read_audio(f).ok().map(|a| crate::alchemywin::zone_file(f, &a))).collect();
            if !fs.is_empty() {
                let mut d = lg_synth::alchemy_zones::ZoneDoc::default();
                d.src[0].import(fs, lg_synth::alchemy_zones::Mapping::Pitch, false);
                app.inst.alch_zones.insert(id, d);
                set_params(app, t, &[(lg_synth::alchemy_zones::SAMP_ON, 1.0), (297, 0.0)]);
            }
        }
        "Drum Machine Designer" => {
            let mut pads = crate::dmdwin::Pads::default();
            for (k, f) in files.iter().enumerate() {
                if let Ok(a) = lg_synth::wav::read_audio(f) {
                    pads.pads.insert(36 + k as u8, crate::dmdwin::PadDoc { name: f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), file: Some(f.clone()), audio: Some(std::sync::Arc::new(a)), quick: Vec::new() });
                }
            }
            app.inst.pads.insert(id, pads);
        }
        _ => {}
    }
    bump(app, t);
    app.plugin_window = Some(t);
}

pub fn window(app: &mut App, ctx: &egui::Context) -> bool {
    test_hook(app);
    sync(app);
    if !ctx.input(|i| i.pointer.any_down()) {
        app.inst.gesture = false;
    }
    let Some(t) = app.plugin_window else {
        app.inst.rect = None;
        return false;
    };
    // (the keyboard and studio instruments' windows: keyswin)
    if crate::keyswin::window(app, ctx, t) {
        app.inst.dropped.clear();
        return true;
    }
    let Some(k) = kind_of(app, t) else {
        app.inst.rect = None;
        return false;
    };
    match k {
        Kind::Sampler => crate::samplerwin::window(app, ctx, t),
        Kind::Quick => crate::quickwin::window(app, ctx, t),
        Kind::Dmd => crate::dmdwin::window(app, ctx, t),
        Kind::Dkd => crate::dkdwin::window(app, ctx, t),
        Kind::Alchemy => crate::alchemywin::window(app, ctx, t),
        Kind::Synth(s) => crate::synthwin::window(app, ctx, t, s),
    }
    app.inst.dropped.clear();
    true
}

/// Keys with the pointer over an editor window: its undo, then its own key commands.
pub fn key_press(app: &mut App, ctx: &egui::Context, key: egui::Key, m: egui::Modifiers) -> bool {
    let Some(r) = app.inst.rect else { return false };
    if app.plugin_window.is_none() || !ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| r.contains(p)) {
        return false;
    }
    if ctx.egui_wants_keyboard_input() {
        return false;
    }
    if m.command && key == egui::Key::Z {
        return if m.shift { redo(app) } else { undo(app) };
    }
    let Some(t) = app.plugin_window else { return false };
    let area = match kind_of(app, t) {
        Some(Kind::Sampler) => "Sampler",
        Some(Kind::Dmd) => "Drum Machine Designer",
        _ => return false,
    };
    // ⌘ bindings first, then ⌃ (both are Ctrl here)
    for ctrl in [false, true] {
        for b in crate::keymap::bindings().iter().filter(|b| b.area == area && b.ctrl == ctrl) {
            if b.key == key && b.cmd == m.command && b.alt == m.alt && b.shift == m.shift && command(app, ctx, &b.command.clone()) {
                return true;
            }
        }
    }
    // the editors' own keys (Logic's: arrows select, Delete removes)
    match area {
        "Sampler" => crate::samplerwin::plain_key(app, t, key, m),
        _ => crate::dmdwin::plain_key(app, t, key, m),
    }
}

/// Runs an editor key command by name on the open editor.
pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
    }
    let Some(t) = app.plugin_window else { return false };
    match kind_of(app, t) {
        Some(Kind::Sampler) => crate::samplerwin::command(app, ctx, t, name),
        Some(Kind::Dmd) => crate::dmdwin::command(app, t, name),
        Some(Kind::Quick) if name.contains("Plug-in Setting") => false,
        _ => false,
    }
}

/// Logic's instrument slot menu (Mixer, Inspector): the instruments Studio has, by Logic's names.
pub const SLOT_MENU: &[(&str, &[(&str, &str)])] = &[
    ("Drums", &[("Drum Kit Designer", "Drum Kit Designer"), ("Drum Synth", "Drum Synth"), ("Ultrabeat (Drum Machine)", "Ultrabeat")]),
    ("Sampler", &[("Quick Sampler (Single Sample)", "Quick Sampler"), ("Sample Alchemy", "Sample Alchemy"), ("Sampler (Multi-Sample)", "Sampler")]),
    ("Studio Instruments", &[("Studio Bass", "Studio Bass"), ("Studio Horns", "Studio Horns"), ("Studio Piano", "Studio Piano"), ("Studio Strings", "Studio Strings")]),
    ("Synthesizer", &[("Alchemy", "Alchemy"), ("EFM1 (FM Synth)", "EFM1"), ("ES E (Ensemble Synth)", "ES E"), ("ES M (Monophonic Synth)", "ES M"), ("ES P (Polyphonic Synth)", "ES P"), ("ES1 (Synthesizer 1)", "ES1"), ("ES2 (Synthesizer 2)", "ES2"), ("EVOC 20 PS (Vocoder Synth)", "EVOC 20 PolySynth"), ("Retro Synth", "Retro Synth"), ("Sculpture (Modeling Synth)", "Sculpture")]),
    ("Utility", &[("External Instrument", "External Instrument"), ("Klopfgeist", "Klopfgeist"), ("Test Oscillator", "Test Oscillator")]),
    ("Vintage Keys", &[("Vintage B3 Organ", "Vintage B3 Organ"), ("Vintage Clav", "Vintage Clav"), ("Vintage Electric Piano", "Vintage Electric Piano"), ("Vintage Mellotron", "Vintage Mellotron")]),
];

/// The instrument slot's menu; picking an instrument puts it on the track.
pub fn slot_menu(app: &mut App, resp: &egui::Response, t: usize) {
    let mut pick: Option<Option<&str>> = None;
    let mut hosted: Option<String> = None;
    let legacy_id = egui::Id::unique(("slot-legacy", t));
    if resp.secondary_clicked() || resp.clicked() {
        let alt = resp.ctx.input(|i| i.modifiers.alt);
        resp.ctx.data_mut(|d| d.insert_temp(legacy_id, alt));
    }
    let legacy: bool = resp.ctx.data(|d| d.get_temp(legacy_id)).unwrap_or(false);
    resp.context_menu(|ui| {
        ui.set_min_width(200.0);
        if ui.button("No Plug-in").clicked() {
            pick = Some(None);
            ui.close();
        }
        ui.separator();
        for (cat, items) in SLOT_MENU {
            ui.menu_button(*cat, |ui| {
                for (label, name) in *items {
                    ui.menu_button(*label, |ui| {
                        if ui.button("Stereo").clicked() {
                            pick = Some(Some(name));
                            ui.close();
                        }
                    });
                }
            });
        }
        // Option-click: the Legacy submenu (below Vintage Keys, as Logic's under Vintage Mellotron)
        if legacy {
            ui.menu_button("Legacy", |ui| {
                for (name, _) in lg_synth::legacy::VARIANTS {
                    ui.menu_button(*name, |ui| {
                        if ui.button("Stereo").clicked() {
                            pick = Some(Some(name));
                            ui.close();
                        }
                    });
                }
            });
        }
        if ui.button("Drum Machine Designer").clicked() {
            pick = Some(Some("Drum Machine Designer"));
            ui.close();
        }
        // third-party instruments (CLAP / VST3 / LV2, hosting.rs) under Logic's "Audio Units"
        ui.separator();
        ui.menu_button("Audio Units", |ui| {
            if let Some(n) = crate::hosting::instrument_items(ui) {
                hosted = Some(n);
            }
        });
    });
    if let Some(n) = hosted.take() {
        crate::hosting::set_instrument(app, t, Some(n));
        return;
    }
    let Some(choice) = pick else { return };
    match choice {
        Some(n) => {
            app.instrument_choice.insert(t, n.to_string());
        }
        None => {
            app.instrument_choice.insert(t, String::new());
        }
    }
    if let Some(id) = track_id(app, t) {
        app.inst.params.remove(&id);
        app.inst.rev.remove(&id);
    }
    // (External Instrument: the track's MIDI goes out, the module's audio comes back)
    crate::utilwin::picked(app, t, choice);
    app.load_engine();
    if let Some(id) = track_id(app, t) {
        // the new instrument starts from its own state (a Sampler: an empty instrument)
        if choice == Some("Sampler") {
            app.inst.docs.insert(id, SamplerDoc::new());
        }
        bump(app, t);
    }
    if choice.is_some() {
        app.plugin_window = Some(t);
    }
    app.dirty = true;
}

/// Musical Typing and MIDI notes the editors follow (Select Zone of Last Played Key…).
pub fn note_played(app: &mut App, pitch: u8) {
    app.inst.last_played = Some(pitch);
    crate::samplerwin::note_played(app, pitch);
    crate::dmdwin::note_played(app, pitch);
    crate::alchemyedit::note_played(pitch);
}
