//! Third-party plug-ins in Studio, as Logic has Audio Units: Studio hosts CLAP, VST3 and LV2
//! (lg-host).
//!
//! - The Plug-in Manager (Settings ▸ Plug-in Manager…): every plug-in the scan found, by
//!   manufacturer, with its format, version, latency and compatibility (validated, not
//!   compatible, crashed N times while scanning, ignored); search; Show All reveals the Hide
//!   (not in the menus, still loaded by projects) and Ignore (not scanned, not loaded) columns;
//!   Reset & Rescan Selection, Full Reset & Rescan, Restore Default Settings, Done. The scan
//!   runs in the background and each plug-in file is looked at in its own process.
//! - The channel strip slot menus: the Audio FX slots' and the instrument slot's "Audio Units"
//!   submenu lists the hosted plug-ins by manufacturer (Logic's place for third-party ones).
//! - The plug-in windows: the plug-in's own text for values, and View: Editor (its own editor
//!   window: embedded in an X11 window Studio makes, through XWayland on Wayland) beside
//!   Studio's Controls view. Parameters reach automation lanes and Smart Controls like
//!   Studio's own; latency goes into the plug-in delay compensation.
//! - State: a hosted plug-in's settings (parameter values and its own state blob, lg_host::
//!   state) ride in the slot's floats, so Copy / Paste, Compare, undo and Plug-in Presets carry
//!   them; on Save they go into Studio's own settings beside the project
//!   (~/.config/bramble-studio/hosted-plug-ins/), since the Logic file can't hold them, and come
//!   back when the project opens.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lg_host::scan::{Entry, Progress, Status};
use lg_host::{Kind, PluginInfo};

use crate::app::App;
use crate::audio::FxStrip;
use crate::mixer::AUX;
use crate::mixstate::MASTER;

// (build.rs and tool/checklist.py read it)
#[allow(dead_code)]
pub const COMMANDS: &[&str] = &["Plug-in Manager…", "Audio Units"];

/// Where a hosted plug-in sits: a track's instrument slot, or a strip's Audio FX slot
/// (Mixer strip id, insert index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Slot {
    Inst(usize),
    Fx(usize, usize),
}

/// The Mixer's strip id for an engine strip.
pub fn strip_id(s: FxStrip) -> usize {
    match s {
        FxStrip::Track(t) => t,
        FxStrip::Aux(a) => AUX + a,
        FxStrip::Master => MASTER,
    }
}

#[derive(Default)]
struct Ui {
    manager_open: bool,
    entries: Vec<Entry>,
    search: String,
    /// The left column's pick: "" all, "\u{1}Instruments", "\u{1}Effects", or a manufacturer.
    filter: String,
    selected: BTreeSet<String>,
    show_all: bool,
    scan: Option<Arc<Mutex<Progress>>>,
    started: bool,
    /// Slots whose plug-in changed its settings (its editor), and when they were last copied
    /// into the slot's floats.
    stale: BTreeSet<Slot>,
    last_sync: Option<Instant>,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

fn with<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
    UI.with(|u| f(&mut u.borrow_mut()))
}

/// Starts a scan in the background (unchanged files keep their result unless in `force`).
fn start_scan(force: HashSet<PathBuf>) {
    with(|u| {
        if u.scan.as_ref().is_some_and(|p| !p.lock().unwrap_or_else(|e| e.into_inner()).finished) {
            return;
        }
        let p = Arc::new(Mutex::new(Progress::default()));
        u.scan = Some(p.clone());
        std::thread::spawn(move || {
            lg_host::scan::scan_all(&force, Some(&p));
        });
    });
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Plug-in Manager…" => {
            with(|u| {
                u.manager_open = true;
                u.entries = lg_host::scan::load_cache();
            });
            let _ = app;
            true
        }
        _ => false,
    }
}

/// The key a plug-in has in the manager's lists.
fn key_of(e: &Entry) -> String {
    match &e.info {
        Some(i) => lg_host::scan::key(i),
        None => e.path.display().to_string(),
    }
}

/// The hosted plug-ins of a kind for the slot menus, by manufacturer (hidden ones left out).
fn by_maker(kind: Kind) -> BTreeMap<String, Vec<PluginInfo>> {
    let hidden = lg_host::scan::choices().hidden;
    let mut m: BTreeMap<String, Vec<PluginInfo>> = BTreeMap::new();
    for p in lg_host::plugins() {
        if p.kind == kind && !hidden.contains(&lg_host::scan::key(&p)) {
            m.entry(p.manufacturer().to_string()).or_default().push(p);
        }
    }
    for v in m.values_mut() {
        v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.format.cmp(&b.format)));
    }
    m
}

/// The "Audio Units" submenu's contents: manufacturer ▸ plug-in ▸ Stereo. Returns the pick
/// (its Studio name).
pub fn menu_items(ui: &mut egui::Ui, kind: Kind, current: Option<&str>) -> Option<String> {
    let mut picked = None;
    let list = by_maker(kind);
    if list.is_empty() {
        ui.add_enabled(false, egui::Button::new("No CLAP, VST3 or LV2 plug-ins found"));
        if ui.button("Plug-in Manager…").clicked() {
            with(|u| {
                u.manager_open = true;
                u.entries = lg_host::scan::load_cache();
            });
            ui.close();
        }
        return None;
    }
    for (maker, plugs) in list {
        ui.menu_button(maker, |ui| {
            for p in plugs {
                let n = p.studio_name();
                let label = if current == Some(n.as_str()) { format!("✓ {n}") } else { n.clone() };
                ui.menu_button(label, |ui| {
                    let ch = if p.inputs == 1 && p.outputs == 2 { "Mono → Stereo" } else if p.outputs == 1 { "Mono" } else { "Stereo" };
                    if ui.button(ch).clicked() {
                        picked = Some(n.clone());
                        ui.close();
                    }
                });
            }
        });
    }
    picked
}

/// The hosted instruments by manufacturer, for another slot menu (instwin.rs); the one picked.
pub fn instrument_items(ui: &mut egui::Ui) -> Option<String> {
    menu_items(ui, Kind::Instrument, None)
}

/// Puts a hosted instrument in a track's instrument slot (None: takes it out, the track plays
/// its patch again).
pub fn set_instrument(app: &mut App, t: usize, name: Option<String>) {
    crate::mixstate::checkpoint(app, if name.is_some() { "Insert Plug-in" } else { "Remove Plug-in" });
    let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return };
    match &name {
        Some(n) => {
            let mut u = crate::mixstate::new_plugin(n);
            u.slot = 2;
            match tr.plugins.iter_mut().find(|u| u.slot == 2) {
                Some(x) => *x = u,
                None => {
                    tr.plugins.push(u);
                    tr.plugins.sort_by_key(|u| u.slot);
                }
            }
        }
        None => tr.plugins.retain(|u| !(u.slot == 2 && lg_host::parse_studio_name(&u.name).is_some())),
    }
    app.instrument_choice.remove(&t);
    app.dirty = true;
    app.load_engine();
    if name.is_some() {
        // (Logic opens the plug-in window on insertion)
        app.plugin_window = Some(t);
    }
}

/// The hosted plug-in in a slot, from the engine.
fn instance<'a>(e: &'a mut crate::audio::Engine, slot: Slot) -> Option<&'a mut lg_host::Instance> {
    let (strip, k) = match slot {
        Slot::Inst(t) => (FxStrip::Track(t), None),
        Slot::Fx(s, k) => (crate::mixer::fx_strip(s), Some(k)),
    };
    let (_, u) = e.all_units_mut().into_iter().find(|(s, _)| *s == strip)?;
    let unit: &mut dyn lg_dsp::Plugin = match k {
        None => u.instrument.as_deref_mut()?,
        Some(k) => {
            let at = u.insert_index.iter().position(|x| *x == k)?;
            u.inserts.get_mut(at)?.as_mut()
        }
    };
    unit.hosted()?.downcast_mut::<lg_host::Instance>()
}

/// Whether the plug-in in a slot is a hosted one (and has its own editor).
pub fn hosted_info(app: &App, slot: Slot) -> Option<(bool, bool)> {
    let mut e = app.engine.lock().ok()?;
    let i = instance(&mut e, slot)?;
    Some((i.has_editor(), i.editor_open()))
}

/// The plug-in's own text for each value (None: Studio formats it).
pub fn texts(app: &App, slot: Slot, params: &[lg_dsp::Param], values: &[f32]) -> Vec<Option<String>> {
    let Ok(mut e) = app.engine.lock() else { return vec![None; params.len()] };
    match instance(&mut e, slot) {
        Some(i) => params.iter().zip(values).map(|(p, v)| lg_dsp::Plugin::value_text(i, p.id, *v)).collect(),
        None => vec![None; params.len()],
    }
}

/// The plug-in window's view row for a hosted plug-in: "View: Controls | Editor" (the Editor
/// button opens or closes the plug-in's own window).
pub fn view_row(app: &mut App, ui: &mut egui::Ui, slot: Slot, title: &str) {
    let Some((has, open)) = hosted_info(app, slot) else { return };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("View:").color(crate::look::text_dim()));
        let _ = ui.add(egui::Button::selectable(true, "Controls"));
        if ui.add_enabled(has, egui::Button::selectable(open, "Editor")).on_hover_text(if has { "The plug-in's own window" } else { "This plug-in has no editor of its own" }).clicked() {
            let res = app.engine.lock().ok().and_then(|mut e| {
                let i = instance(&mut e, slot)?;
                Some(if i.editor_open() {
                    i.close_editor();
                    Ok(())
                } else {
                    i.open_editor(title)
                })
            });
            if let Some(Err(msg)) = res {
                app.tell(msg);
            }
        }
    });
}

/// After an edit in Studio's Controls view of a hosted instrument: its slot's saved state
/// needs refreshing.
pub fn mark_changed(slot: Slot) {
    with(|u| {
        u.stale.insert(slot);
    });
}

/// Copies a hosted plug-in's settings (values and state) into its slot's floats.
pub fn sync_slot(app: &mut App, slot: Slot) {
    let floats = app.engine.lock().ok().and_then(|mut e| instance(&mut e, slot).map(|i| i.floats()));
    let Some(f) = floats else { return };
    match slot {
        Slot::Inst(t) => {
            if let Some(u) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|tr| tr.plugins.iter_mut().find(|u| u.slot == 2)) {
                if u.params != f {
                    u.params = f;
                    app.dirty = true;
                }
            }
        }
        Slot::Fx(s, k) => {
            if crate::mixstate::inserts(app, s).get(k).is_some_and(|u| u.params != f) {
                crate::mixstate::edit_inserts(app, s, |v| {
                    if let Some(u) = v.get_mut(k) {
                        u.params = f;
                    }
                });
            }
        }
    }
}

/// Every hosted plug-in's settings copied into its slot (before Save, Copy, a preset).
pub fn sync_all(app: &mut App) {
    let slots: Vec<Slot> = app.engine.lock().map(|mut e| hosted_slots(&mut e)).unwrap_or_default();
    for s in slots {
        sync_slot(app, s);
    }
}

fn hosted_slots(e: &mut crate::audio::Engine) -> Vec<Slot> {
    let mut v = Vec::new();
    for (strip, u) in e.all_units_mut() {
        if let (FxStrip::Track(t), Some(ins)) = (strip, u.instrument.as_deref_mut()) {
            if ins.hosted().is_some() {
                v.push(Slot::Inst(t));
            }
        }
        for (fx, k) in u.inserts.iter_mut().zip(u.insert_index.iter()) {
            if fx.hosted().is_some() {
                v.push(Slot::Fx(strip_id(strip), *k));
            }
        }
    }
    v
}

/// Each frame: the hosted plug-ins' main-thread work (callbacks, timers, file descriptors,
/// their editor windows), settings changed in their editors copied into the slots (at most a
/// few times a second), the first scan of the session.
pub fn idle(app: &mut App, ctx: &egui::Context) {
    if !with(|u| std::mem::replace(&mut u.started, true)) && std::env::var_os("LG_NO_PLUGIN_SCAN").is_none() {
        // (new and changed plug-in files since the last run; unchanged ones keep their result)
        start_scan(HashSet::new());
    }
    if let Some(p) = with(|u| u.scan.clone()) {
        let done = p.lock().unwrap_or_else(|e| e.into_inner()).finished;
        if done {
            with(|u| {
                u.scan = None;
                u.entries = lg_host::scan::load_cache();
            });
            // plug-ins that failed to load before may load now
            app.load_engine();
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    let mut any = false;
    let mut editors = false;
    let mut changed = Vec::new();
    if let Ok(mut e) = app.engine.lock() {
        for (strip, u) in e.all_units_mut() {
            let mut visit = |slot: Slot, p: &mut dyn lg_dsp::Plugin| {
                if let Some(i) = p.hosted().and_then(|h| h.downcast_mut::<lg_host::Instance>()) {
                    any = true;
                    i.idle();
                    editors |= i.editor_open();
                    if i.take_changed() {
                        changed.push(slot);
                    }
                }
            };
            if let (FxStrip::Track(t), Some(ins)) = (strip, u.instrument.as_deref_mut()) {
                visit(Slot::Inst(t), ins);
            }
            for (fx, k) in u.inserts.iter_mut().zip(u.insert_index.clone()) {
                visit(Slot::Fx(strip_id(strip), k), fx.as_mut());
            }
        }
    }
    with(|u| u.stale.extend(changed));
    let due = with(|u| !u.stale.is_empty() && u.last_sync.is_none_or(|t| t.elapsed() > Duration::from_millis(400)));
    if due {
        let list: Vec<Slot> = with(|u| {
            u.last_sync = Some(Instant::now());
            std::mem::take(&mut u.stale).into_iter().collect()
        });
        for s in list {
            sync_slot(app, s);
        }
    }
    if editors {
        ctx.request_repaint_after(Duration::from_millis(16));
    } else if any {
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

/// A hosted plug-in's parameter list (for the automation lanes' menus and values), made once
/// per plug-in; None for Studio's own.
pub fn params_of(name: &str) -> Option<Vec<lg_dsp::Param>> {
    thread_local! {
        static P: RefCell<std::collections::HashMap<String, Option<Vec<lg_dsp::Param>>>> = RefCell::new(Default::default());
    }
    if !is_hosted(name) {
        return None;
    }
    P.with(|m| m.borrow_mut().entry(name.to_string()).or_insert_with(|| lg_host::open_by_name(name, crate::audio::RATE as f32).map(|i| lg_dsp::Plugin::params(&i).to_vec())).clone())
}

/// A plug-in slot's help tag: its name and latency in samples and milliseconds (Logic's).
pub fn slot_tip(app: &App, s: usize, k: usize, name: &str) -> String {
    let lat = app.engine.lock().ok().and_then(|mut e| {
        let strip = crate::mixer::fx_strip(s);
        let (_, u) = e.all_units_mut().into_iter().find(|(x, _)| *x == strip)?;
        let at = u.insert_index.iter().position(|x| *x == k)?;
        Some(u.inserts[at].latency())
    });
    match lat {
        Some(n) => format!("{name}\nLatency: {n} samples ({:.1} ms)", n as f64 * 1000.0 / crate::audio::RATE as f64),
        None => name.to_string(),
    }
}

// --- the Plug-in Manager ---------------------------------------------------------------------

pub fn manager(app: &mut App, ctx: &egui::Context) {
    if !with(|u| u.manager_open) {
        return;
    }
    let mut open = true;
    let mut done = false;
    let mut rescan: Option<HashSet<PathBuf>> = None;
    let mut choices = lg_host::scan::choices();
    let mut choices_changed = false;
    let progress = with(|u| u.scan.as_ref().map(|p| p.lock().unwrap_or_else(|e| e.into_inner()).clone()));
    egui::Window::new("Plug-in Manager").id(egui::Id::unique("plug-in-manager")).open(&mut open).collapsible(false).resizable(true).default_size(egui::vec2(980.0, 560.0)).show(ctx, |ui| {
        UI.with(|cell| {
            let u = &mut *cell.borrow_mut();
            ui.horizontal(|ui| {
                ui.checkbox(&mut u.show_all, "Show All");
                ui.separator();
                ui.add(egui::TextEdit::singleline(&mut u.search).hint_text("Search").desired_width(200.0));
                if let Some(p) = &progress {
                    ui.separator();
                    ui.spinner();
                    ui.label(format!("Scanning {} of {}: {}", (p.done + 1).min(p.total.max(1)), p.total, p.current));
                }
            });
            ui.separator();
            let entries = u.entries.clone();
            let makers: BTreeSet<String> = entries.iter().filter_map(|e| e.info.as_ref()).map(|i| i.manufacturer().to_string()).collect();
            let avail = ui.available_size();
            ui.horizontal_top(|ui| {
                // the left column: All, the kinds, the manufacturers (Logic lists Audio Units by
                // manufacturer)
                ui.allocate_ui(egui::vec2(190.0, avail.y - 44.0), |ui| {
                    egui::ScrollArea::vertical().id_salt("pm-left").show(ui, |ui| {
                        ui.set_min_width(180.0);
                        for (label, key) in [("All", String::new()), ("Instruments", "\u{1}Instruments".to_string()), ("Effects", "\u{1}Effects".to_string())] {
                            if ui.add(egui::Button::selectable(u.filter == key, label)).clicked() {
                                u.filter = key;
                            }
                        }
                        ui.separator();
                        ui.label(egui::RichText::new("Manufacturers").color(crate::look::text_dim()).small());
                        for m in &makers {
                            if ui.add(egui::Button::selectable(u.filter == *m, m)).clicked() {
                                u.filter = m.clone();
                            }
                        }
                    });
                });
                ui.separator();
                let q = u.search.to_lowercase();
                let shown: Vec<&Entry> = entries.iter().filter(|e| {
                    let name = e.info.as_ref().map_or_else(|| e.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), |i| i.name.clone());
                    let ok_q = q.is_empty() || name.to_lowercase().contains(&q) || e.info.as_ref().is_some_and(|i| i.vendor.to_lowercase().contains(&q));
                    let ok_f = match u.filter.as_str() {
                        "" => true,
                        "\u{1}Instruments" => e.info.as_ref().is_some_and(|i| i.kind == Kind::Instrument),
                        "\u{1}Effects" => e.info.as_ref().is_some_and(|i| i.kind == Kind::Effect),
                        m => e.info.as_ref().is_some_and(|i| i.manufacturer() == m),
                    };
                    ok_q && ok_f
                }).collect();
                ui.vertical(|ui| {
                    egui::ScrollArea::both().id_salt("pm-table").max_height(avail.y - 44.0).show(ui, |ui| {
                        egui::Grid::new("pm-grid").striped(true).num_columns(10).spacing(egui::vec2(14.0, 4.0)).show(ui, |ui| {
                            let head = |ui: &mut egui::Ui, t: &str| {
                                ui.label(egui::RichText::new(t).strong());
                            };
                            if u.show_all {
                                head(ui, "Hide");
                                head(ui, "Ignore");
                            }
                            for h in ["Name", "Manufacturer", "Type", "Format", "Version", "Latency", "Compatibility"] {
                                head(ui, h);
                            }
                            ui.end_row();
                            if shown.is_empty() {
                                ui.label(egui::RichText::new(if entries.is_empty() { "No plug-ins found in the CLAP, VST3 and LV2 folders (~/.clap, /usr/lib/clap, ~/.vst3, /usr/lib/vst3, ~/.lv2, /usr/lib/lv2 …)" } else { "Nothing matches" }).color(crate::look::text_dim()));
                                ui.end_row();
                            }
                            for e in shown {
                                let key = key_of(e);
                                if u.show_all {
                                    let mut hide = choices.hidden.contains(&key);
                                    if ui.add_enabled(e.info.is_some(), egui::Checkbox::without_text(&mut hide)).on_hover_text("Hide: not in the plug-in menus (projects still load it)").changed() {
                                        if hide {
                                            choices.hidden.insert(key.clone());
                                        } else {
                                            choices.hidden.remove(&key);
                                        }
                                        choices_changed = true;
                                    }
                                    let pkey = e.path.display().to_string();
                                    let mut ign = choices.ignored.contains(&pkey) || choices.ignored.contains(&key);
                                    if ui.add(egui::Checkbox::without_text(&mut ign)).on_hover_text("Ignore: not scanned and not loaded").changed() {
                                        if ign {
                                            choices.ignored.insert(pkey);
                                        } else {
                                            choices.ignored.remove(&pkey);
                                            choices.ignored.remove(&key);
                                        }
                                        choices_changed = true;
                                    }
                                }
                                let name = e.info.as_ref().map_or_else(|| e.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), |i| i.name.clone());
                                let sel = u.selected.contains(&key);
                                let r = ui.add(egui::Button::selectable(sel, name)).on_hover_text(e.path.display().to_string());
                                if r.clicked() {
                                    let multi = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                                    if !multi {
                                        u.selected.clear();
                                    }
                                    if sel && multi {
                                        u.selected.remove(&key);
                                    } else {
                                        u.selected.insert(key.clone());
                                    }
                                }
                                let i = e.info.as_ref();
                                ui.label(i.map_or("", |i| i.manufacturer()));
                                ui.label(i.map_or("", |i| if i.kind == Kind::Instrument { "Instrument" } else { "Effect" }));
                                ui.label(e.format.label());
                                ui.label(i.map_or("", |i| i.version.as_str()));
                                ui.label(i.map_or(String::new(), |i| if i.latency == 0 { "0".into() } else { format!("{} smp", i.latency) }));
                                let st = e.status.label();
                                let col = match e.status {
                                    Status::Ok => crate::look::text(),
                                    Status::Ignored => crate::look::text_dim(),
                                    _ => egui::Color32::from_rgb(0xe0, 0x70, 0x50),
                                };
                                ui.label(egui::RichText::new(st).color(col));
                                ui.end_row();
                            }
                        });
                    });
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                let busy = progress.is_some();
                if ui.add_enabled(!busy && !u.selected.is_empty(), egui::Button::new("Reset & Rescan Selection")).clicked() {
                    let sel = &u.selected;
                    rescan = Some(entries.iter().filter(|e| sel.contains(&key_of(e))).map(|e| e.path.clone()).collect());
                }
                if ui.add_enabled(!busy, egui::Button::new("Full Reset & Rescan")).on_hover_text("Scans every plug-in file again").clicked() {
                    rescan = Some(entries.iter().map(|e| e.path.clone()).collect());
                }
                if ui.button("Restore Default Settings").on_hover_text("Shows hidden plug-ins and scans ignored ones again").clicked() {
                    choices = Default::default();
                    choices_changed = true;
                    rescan = Some(HashSet::new());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Done").clicked() {
                        done = true;
                    }
                    let ok = entries.iter().filter(|e| e.status.ok()).count();
                    ui.label(egui::RichText::new(format!("{ok} plug-ins available · {} files scanned", entries.iter().map(|e| &e.path).collect::<BTreeSet<_>>().len())).color(crate::look::text_dim()));
                });
            });
        });
    });
    if choices_changed {
        lg_host::scan::save_choices(&choices);
    }
    if let Some(force) = rescan {
        start_scan(force);
    }
    if !open || done {
        with(|u| u.manager_open = false);
    }
    let _ = app;
}

// --- Studio's own settings for a project's hosted plug-ins -----------------------------------

fn sidecar_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("hosted-plug-ins"))
}

/// The settings file for a project (by its alternative's folder or GarageBand package).
pub fn sidecar_path(project: &Path) -> Option<PathBuf> {
    let p = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    let s = p.display().to_string();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let stem: String = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().chars().filter(|c| c.is_alphanumeric()).take(24).collect();
    let parent: String = p.parent().and_then(|q| q.parent()).and_then(|q| q.file_stem()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().chars().filter(|c| c.is_alphanumeric()).take(24).collect();
    Some(sidecar_dir()?.join(format!("{parent}-{stem}-{h:016x}.txt")))
}

fn hex_floats(f: &[f32]) -> String {
    f.iter().map(|x| format!("{:08x}", x.to_bits())).collect()
}

fn unhex_floats(s: &str) -> Vec<f32> {
    (0..s.len() / 8).filter_map(|k| u32::from_str_radix(&s[8 * k..8 * k + 8], 16).ok()).map(f32::from_bits).collect()
}

fn is_hosted(name: &str) -> bool {
    lg_host::parse_studio_name(name).is_some()
}

/// The strips Studio has, as (Mixer strip id, its name for the file).
fn strip_ids(app: &App) -> Vec<(usize, String)> {
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let mut v: Vec<(usize, String)> = (0..n).map(|t| (t, format!("T{t}"))).collect();
    v.extend((0..app.auxes.len()).map(|a| (AUX + a, format!("A{a}"))));
    v.push((MASTER, "M".into()));
    v
}

fn parse_strip(s: &str) -> Option<usize> {
    match s.split_at(1) {
        ("T", n) => n.parse().ok(),
        ("A", n) => n.parse::<usize>().ok().map(|a| AUX + a),
        ("M", "") => Some(MASTER),
        _ => None,
    }
}

/// The project's hosted plug-ins as Studio keeps them (one line per instrument; for a strip
/// with hosted effects, its whole chain in order, hosted ones with their settings).
pub fn sidecar_text(app: &App) -> String {
    let mut s = String::from("# Bramble Studio: the hosted CLAP / VST3 / LV2 plug-ins of this project (the Logic file can't hold them)\n");
    if let Some(p) = &app.project {
        for (t, tr) in p.tracks.iter().enumerate() {
            if let Some(u) = tr.plugins.iter().find(|u| u.slot == 2 && is_hosted(&u.name)) {
                s += &format!("inst\tT{t}\t{}\t{}\n", u.name, hex_floats(&u.params));
            }
        }
    }
    for (id, tag) in strip_ids(app) {
        let chain = crate::mixstate::inserts(app, id);
        if !chain.iter().any(|u| is_hosted(&u.name)) {
            continue;
        }
        s += &format!("chain\t{tag}\t{}\n", chain.len());
        for u in chain {
            if is_hosted(&u.name) {
                s += &format!("fx\t{}\t{}\t{}\t{}\n", u.name, u8::from(u.bypass), u.preset, hex_floats(&u.params));
            } else {
                s += &format!("own\t{}\n", u.name);
            }
        }
    }
    s
}

/// Applies a saved sidecar to the open project; true when anything changed.
pub fn apply_sidecar(app: &mut App, text: &str) -> bool {
    let mut changed = false;
    let mut lines = text.lines().filter(|l| !l.starts_with('#')).peekable();
    while let Some(l) = lines.next() {
        let w: Vec<&str> = l.split('\t').collect();
        match w.as_slice() {
            ["inst", strip, name, floats] => {
                let Some(t) = parse_strip(strip).filter(|t| *t < AUX) else { continue };
                let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { continue };
                let mut u = crate::mixstate::new_plugin(name);
                u.slot = 2;
                u.params = unhex_floats(floats);
                match tr.plugins.iter_mut().find(|x| x.slot == 2) {
                    Some(x) => *x = u,
                    None => {
                        tr.plugins.push(u);
                        tr.plugins.sort_by_key(|x| x.slot);
                    }
                }
                app.instrument_choice.remove(&t);
                changed = true;
            }
            ["chain", strip, n] => {
                let Some(id) = parse_strip(strip) else { continue };
                let n: usize = n.parse().unwrap_or(0);
                let mut saved = Vec::new();
                for _ in 0..n {
                    let Some(x) = lines.next() else { break };
                    saved.push(x.to_string());
                }
                if id < AUX && app.project.as_ref().is_none_or(|p| id >= p.tracks.len()) {
                    continue;
                }
                if (AUX..MASTER).contains(&id) && id - AUX >= app.auxes.len() {
                    continue;
                }
                // Studio's own plug-ins come from the project (by name, in order); the hosted
                // ones from the file, where they were
                let mut current = crate::mixstate::inserts(app, id);
                let mut chain = Vec::new();
                for x in saved {
                    let w: Vec<&str> = x.split('\t').collect();
                    match w.as_slice() {
                        ["fx", name, bypass, preset, floats] => {
                            let mut u = crate::mixstate::new_plugin(name);
                            u.bypass = *bypass == "1";
                            u.preset = preset.to_string();
                            u.params = unhex_floats(floats);
                            chain.push(u);
                        }
                        ["own", name] => {
                            if let Some(k) = current.iter().position(|u| u.name == *name) {
                                chain.push(current.remove(k));
                            }
                        }
                        _ => {}
                    }
                }
                crate::mixstate::edit_inserts(app, id, |v| *v = chain);
                changed = true;
            }
            _ => {}
        }
    }
    changed
}

/// After Save: the project's hosted plug-ins into Studio's settings (or the file removed when
/// there are none).
pub fn save_sidecar(app: &mut App) {
    sync_all(app);
    let Some(path) = app.file.as_ref().and_then(|f| sidecar_path(&f.0)) else { return };
    let text = sidecar_text(app);
    if text.lines().all(|l| l.starts_with('#')) {
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(path, text);
}

/// After a project opens: its hosted plug-ins back from Studio's settings.
pub fn after_open(app: &mut App) {
    let Some(path) = app.file.as_ref().and_then(|f| sidecar_path(&f.0)) else { return };
    let Ok(text) = std::fs::read_to_string(path) else { return };
    if apply_sidecar(app, &text) {
        app.load_engine();
        app.dirty = false;
    }
}

/// What Save reports left out: the hosted plug-ins are kept by Studio, not missing.
pub fn saved_elsewhere(left: &mut Vec<String>) {
    left.retain(|l| !(l.contains(" (CLAP)") || l.contains(" (VST3)") || l.contains(" (LV2)")));
}

// --- Plug-in Presets ----------------------------------------------------------------------------

/// A hosted plug-in's preset file gets its whole state too (a `state` line after the values).
pub fn preset_extra(name: &str, floats: &[f32]) -> Option<String> {
    is_hosted(name).then(|| format!("state {}\n", hex_floats(floats)))
}

/// The full settings a preset file carries for a hosted plug-in.
pub fn preset_floats(path: &Path) -> Option<Vec<f32>> {
    let t = std::fs::read_to_string(path).ok()?;
    t.lines().find_map(|l| l.strip_prefix("state ")).map(unhex_floats)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// lg-host's test CLAP plug-in (built by `cargo test -p lg-host`), copied into a folder of
    /// its own and made known to Studio (in this process: no scan).
    fn test_plugins() -> Option<()> {
        let exe = std::env::current_exe().ok()?;
        let so = exe.parent()?.parent()?.join("examples").join("libtest_clap.so");
        if !so.exists() {
            eprintln!("{} missing: run `cargo test -p lg-host` first (it builds the test plug-ins); skipped", so.display());
            return None;
        }
        let d = exe.parent()?.parent()?.join("lg-app-hosting");
        let _ = std::fs::create_dir_all(&d);
        let p = d.join("Test.clap");
        // (replaced as installers do, beside then renamed: copied over in place, the library an
        // earlier test in this process loaded would be rewritten under its feet and crash the
        // process at exit; lg-host loads the changed file again)
        let tmp = d.join("Test.clap.part");
        std::fs::copy(&so, &tmp).ok()?;
        std::fs::rename(&tmp, &p).ok()?;
        lg_host::set_catalog(lg_host::clap::describe(&p).ok()?);
        Some(())
    }

    fn app() -> App {
        let ctx = egui::Context::default();
        App::new(&eframe::CreationContext::_new_kittest(ctx), None)
    }

    #[test]
    fn a_hosted_effect_plays_in_the_mix_with_its_latency_compensated() {
        if test_plugins().is_none() {
            return;
        }
        use crate::audio::{Engine, PlayTrack};
        let track = |inserts: &[&str]| {
            let mut a = vec![0f32; 2 * 4096];
            a[2 * 200] = 1.0;
            a[2 * 200 + 1] = 1.0;
            PlayTrack { channel: 0, program: 0, notes: Vec::new(), audio: vec![(0, Arc::new(a))], volume: 1.0, pan: 0.0, mute: false, solo: false, eq: None, instrument: None, instrument_floats: Vec::new(), inserts: inserts.iter().map(|n| (n.to_string(), Vec::new())).collect(), sends: Vec::new(), auto_volume: Vec::new(), auto_pan: Vec::new() }
        };
        let mut e = Engine::new();
        e.tracks = vec![track(&["Test Gain (CLAP)"]), track(&[])];
        e.program_changes();
        e.playing = true;
        assert_eq!(e.latency(), 0, "planned on the first block");
        let mut out = Vec::new();
        let mut buf = vec![0f32; 128];
        for _ in 0..40 {
            e.render(&mut buf);
            out.extend_from_slice(&buf);
        }
        assert_eq!(e.latency(), 32, "the plug-in's reported 32 samples");
        // both clicks land together, 32 frames late (the audio regions' tick rounding spreads a
        // click over a frame or two, as in audio.rs's own check)
        let peaks: Vec<usize> = (0..out.len() / 2).filter(|&k| out[2 * k].abs() > 0.3).collect();
        assert!(!peaks.is_empty() && peaks.iter().all(|f| f.abs_diff(232) <= 2), "{peaks:?}");
        let sum: f32 = peaks.iter().map(|&f| out[2 * f]).sum();
        assert!(sum > 1.5, "both tracks' clicks: {sum}");
        // its parameters and the plug-in's own text, as the plug-in window shows them
        let (params, values) = e.effect_params(FxStrip::Track(0), 0).unwrap();
        assert_eq!(params.iter().map(|p| p.name).collect::<Vec<_>>(), ["Gain", "Mode", "Ticks"]);
        assert_eq!(values[0], 0.0);
        e.set_effect_param(FxStrip::Track(0), 0, 0, -6.0);
        let i = instance(&mut e, Slot::Fx(0, 0)).unwrap();
        assert_eq!(lg_dsp::Plugin::value_text(i, 0, -6.0).as_deref(), Some("-6.0 dB"));
    }

    #[test]
    fn hosted_plugins_and_their_state_come_back_with_the_project() {
        if test_plugins().is_none() {
            return;
        }
        let mut a = app();
        a.set_project(crate::demo::song(), true);
        crate::mixstate::edit_inserts(&mut a, 0, |v| v.extend([crate::mixstate::new_plugin("Compressor"), crate::mixstate::new_plugin("Test Gain (CLAP)")]));
        set_instrument(&mut a, 1, Some("Test Synth (CLAP)".into()));
        a.load_engine();
        {
            let mut e = a.engine.lock().unwrap();
            e.set_effect_param(FxStrip::Track(0), 1, 1, 2.0);
            e.set_instrument_param(1, 0, 0.3);
        }
        sync_all(&mut a);
        let text = sidecar_text(&a);
        assert!(text.contains("inst\tT1\tTest Synth (CLAP)"), "{text}");
        assert!(text.contains("own\tCompressor") && text.contains("fx\tTest Gain (CLAP)"), "{text}");
        // a fresh open of the same project: Studio's settings put them back, state and all
        // (the Compressor is Studio's own: it comes from the project file, the hosted one from
        // Studio's settings)
        let mut b = app();
        b.set_project(crate::demo::song(), true);
        crate::mixstate::edit_inserts(&mut b, 0, |v| v.push(crate::mixstate::new_plugin("Compressor")));
        assert!(apply_sidecar(&mut b, &text));
        b.load_engine();
        let names: Vec<String> = crate::mixstate::inserts(&b, 0).iter().map(|u| u.name.clone()).collect();
        assert_eq!(names, ["Compressor", "Test Gain (CLAP)"]);
        assert_eq!(b.instrument_for(1).as_deref(), Some("Test Synth (CLAP)"));
        let e = b.engine.lock().unwrap();
        assert_eq!(e.effect_params(FxStrip::Track(0), 1).unwrap().1[1], 2.0, "Mode: Mute");
        assert!((e.instrument_params(1).unwrap().1[0] - 0.3).abs() < 1e-6, "Level");
        drop(e);
        // the automation lanes see its parameters without loading it each time
        assert_eq!(params_of("Test Gain (CLAP)").unwrap().len(), 3);
        let slot = b.project.as_ref().unwrap().tracks[0].plugins.iter().find(|u| u.name == "Test Gain (CLAP)").unwrap().slot;
        let (_, pname, lo, hi, v) = crate::automation::plugin_param(&b, 0, slot, 1).unwrap();
        assert_eq!((pname.as_str(), lo, hi, v), ("Mode", 0.0, 2.0, 2.0));
    }

    #[test]
    fn floats_survive_the_text() {
        let f = [0.5f32, -3.25, f32::from_bits(lg_host::state::MARK), 1e-9, 16777215.0];
        assert_eq!(unhex_floats(&hex_floats(&f)).iter().map(|x| x.to_bits()).collect::<Vec<_>>(), f.iter().map(|x| x.to_bits()).collect::<Vec<_>>());
        assert_eq!(parse_strip("T3"), Some(3));
        assert_eq!(parse_strip("A2"), Some(AUX + 2));
        assert_eq!(parse_strip("M"), Some(MASTER));
        let mut left = vec!["plug-in Test Gain (CLAP) on “Bass” (added or moved)".to_string(), "sends".to_string()];
        saved_elsewhere(&mut left);
        assert_eq!(left, ["sends"]);
    }
}
