//! The channel strip's MIDI FX slots (Logic's MIDI plug-ins, lg-dsp midifx): green labels
//! between the EQ display and the instrument slot, in series before the instrument. Click the
//! empty slot for the plug-in menu (the plug-in's window opens), click a label for its window,
//! its left edge bypasses it, right-click for the menu (another plug-in, No Plug-in), drag a
//! label onto another to swap them. The window: bypass, settings (Save Setting As…, saved
//! settings, Recall Default, Copy / Paste), the plug-in's controls and its editors
//! (Arpeggiator grid, Chord Trigger keyboards and Learn, Transposer User scale).
//!
//! The slots live in the project (Track::extra.midi_fx), so edits undo with it. The engine runs
//! each track's chain on the regions' notes and controllers while playing and on keys played
//! live, on the song's tick clock (a free clock when stopped).

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::regions::MidiFxSlot;
use lg_dsp::midifx::{self, Chain, Ctx, Ev, Msg, Status};

use crate::app::App;
use crate::look;

/// Window state.
#[derive(Default)]
pub struct Ui {
    /// The open window: track, slot.
    pub window: Option<(usize, usize)>,
    /// Chord Trigger Learn: None off, Some(None) waiting for the trigger key, Some(Some(k))
    /// assigning notes to key k.
    learn: Option<Option<u8>>,
    clear_armed: bool,
    clipboard: Option<(String, Vec<f32>)>,
    save_name: Option<String>,
    drag_from: Option<(usize, usize)>,
}

// ---------------------------------------------------------------------------------------------
// the engine side

#[derive(Default)]
pub struct EngineFx {
    pub chains: Vec<Chain>,
    live: Vec<Vec<Msg>>,
    /// Each track's units' status (the windows' lights and dots).
    pub status: Vec<Vec<Status>>,
    /// The regions' controllers, pitch bend, pressure, program changes per track (song tick).
    pub controls: Vec<Vec<(u32, Msg)>>,
    /// Window commands for a unit (track, slot, command).
    pub commands: Vec<(usize, usize, String)>,
    clock: f64,
}

impl EngineFx {
    pub fn active(&self, t: usize) -> bool {
        self.chains.get(t).is_some_and(|c| c.active())
    }

    pub fn set(&mut self, t: usize, slots: &[MidiFxSlot]) -> Vec<Ev> {
        if self.chains.len() <= t {
            self.chains.resize_with(t + 1, Chain::default);
        }
        let s: Vec<(String, Vec<f32>, bool)> = slots.iter().map(|s| (s.name.clone(), s.values.clone(), s.bypass)).collect();
        self.chains[t].set(&s)
    }

    pub fn push_live(&mut self, t: usize, m: Msg) {
        if self.live.len() <= t {
            self.live.resize_with(t + 1, Vec::new);
        }
        self.live[t].push(m);
    }

    pub fn reset_all(&mut self) {
        for c in &mut self.chains {
            c.reset();
        }
        for l in &mut self.live {
            l.clear();
        }
    }
}

/// Sends one event on to a track's instrument.
fn dispatch(e: &mut crate::audio::Engine, t: usize, m: Msg) {
    match m {
        Msg::On { pitch, vel, .. } => e.live_note(t, pitch, vel.max(1)),
        Msg::Off { pitch, .. } => e.live_note(t, pitch, 0),
        Msg::Cc { num, val, .. } => e.control_out(t, num, val as u16),
        Msg::Bend { val, .. } => e.control_out(t, 128, (val as i32 + 8192).clamp(0, 16383) as u16),
        Msg::Pressure { val, .. } => e.live_message(t, 0xD0, val, 0),
        Msg::PolyAt { pitch, val, .. } => e.live_message(t, 0xA0, pitch, val),
        Msg::Program { num, .. } => e.live_message(t, 0xC0, num, 0),
    }
}

/// One block for every track's MIDI FX and region controllers: `span` the song ticks when
/// playing (None: stopped or held by a count-in, the free clock runs).
pub fn render(e: &mut crate::audio::Engine, span: Option<(f64, f64)>, tps_block: f64, bpm: f64, audible: &[bool]) {
    let n = e.tracks.len();
    let (from, to, playing) = match span {
        Some((a, b)) => (a, b, true),
        None => {
            let a = e.fx.clock;
            (a, a + tps_block, false)
        }
    };
    e.fx.clock = if playing { to } else { e.fx.clock + tps_block };
    for t in 0..n {
        let active = e.fx.active(t);
        let mut input: Vec<Ev> = Vec::new();
        if playing && audible.get(t).copied().unwrap_or(true) {
            if active {
                for &(s, end, p, v) in &e.tracks[t].notes {
                    if (s as f64) >= from && (s as f64) < to {
                        input.push(Ev::new(s as f64, Msg::On { ch: 0, pitch: p, vel: v }));
                    }
                    if (end as f64) >= from && (end as f64) < to {
                        input.push(Ev::new(end as f64, Msg::Off { ch: 0, pitch: p }));
                    }
                }
            }
            if let Some(cs) = e.fx.controls.get(t) {
                for &(at, m) in cs {
                    if (at as f64) >= from && (at as f64) < to {
                        input.push(Ev::new(at as f64, m));
                    }
                }
            }
        }
        if !active {
            for ev in input {
                dispatch(e, t, ev.msg);
            }
            if let Some(l) = e.fx.live.get_mut(t) {
                l.clear();
            }
            continue;
        }
        if let Some(l) = e.fx.live.get_mut(t) {
            input.extend(l.drain(..).map(|m| Ev::new(from, m)));
        }
        input.sort_by(|a, b| a.tick.total_cmp(&b.tick));
        // window commands (Clear, Play)
        let cmds: Vec<(usize, String)> = e.fx.commands.iter().filter(|c| c.0 == t).map(|c| (c.1, c.2.clone())).collect();
        e.fx.commands.retain(|c| c.0 != t);
        let ctx = Ctx { from, to, bpm, playing };
        let out = {
            let chain = &mut e.fx.chains[t];
            for (k, c) in cmds {
                if let Some(u) = chain.units.get_mut(k) {
                    u.1.command(&c);
                }
            }
            chain.process(input, &ctx)
        };
        for ev in out {
            dispatch(e, t, ev.msg);
        }
        let st: Vec<Status> = e.fx.chains[t].units.iter().map(|u| u.1.status()).collect();
        if e.fx.status.len() <= t {
            e.fx.status.resize_with(t + 1, Vec::new);
        }
        e.fx.status[t] = st;
    }
}

/// Region events as the engine plays them: song tick, message (looped regions repeat them).
pub fn region_controls(tr: &lg_core::Track, song_end: u32) -> Vec<(u32, Msg)> {
    let mut out = Vec::new();
    for r in tr.regions.iter().filter(|r| !r.muted && r.audio.is_none()) {
        let mut until = if r.props.looped { tr.regions.iter().map(|o| o.start).filter(|&s| s >= r.start + r.length).min().unwrap_or(song_end) } else { r.start + r.length };
        if r.props.looped && r.props.loop_to > r.start + r.length {
            until = until.min(r.props.loop_to);
        }
        let mut off = 0u32;
        loop {
            for c in &r.ext.events {
                if c.pos >= r.length {
                    continue;
                }
                let at = r.start + off + c.pos;
                if at >= until {
                    continue;
                }
                let ch = c.channel;
                let m = match c.kind & 0xF0 {
                    0xB0 => Msg::Cc { ch, num: c.number, val: c.value },
                    0xE0 => Msg::Bend { ch, val: lg_core::regions::bend_value(c) },
                    0xD0 => Msg::Pressure { ch, val: c.number },
                    0xA0 => Msg::PolyAt { ch, pitch: c.number, val: c.value },
                    0xC0 => Msg::Program { ch, num: c.number },
                    _ => continue,
                };
                out.push((at, m));
            }
            off += r.length.max(1);
            if !r.props.looped || r.start + off >= until {
                break;
            }
        }
    }
    out.sort_by_key(|x| x.0);
    out
}

/// The tracks' MIDI FX chains and region controllers into the engine (load_engine).
pub fn to_engine(app: &App, e: &mut crate::audio::Engine) {
    let Some(p) = &app.project else { return };
    let song_end = p.tracks.iter().flat_map(|t| t.regions.iter().map(|r| r.start + r.length)).max().unwrap_or(0).max(64 * app.bar_ticks());
    e.fx.controls = p.tracks.iter().map(|t| region_controls(t, song_end)).collect();
    let n = p.tracks.len();
    let mut offs = Vec::new();
    for (i, t) in p.tracks.iter().enumerate() {
        for ev in e.fx.set(i, &t.extra.midi_fx) {
            offs.push((i, ev.msg));
        }
    }
    e.fx.chains.truncate(n);
    for (i, m) in offs {
        dispatch(e, i, m);
    }
}

/// A slot's values changed in the window: the engine's unit reloads (no rebuild).
fn push_slots(app: &App, t: usize) {
    let Some(slots) = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.extra.midi_fx.clone()) else { return };
    let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
    let offs = e.fx.set(t, &slots);
    for ev in offs {
        dispatch(&mut e, t, ev.msg);
    }
}

fn slots(app: &App, t: usize) -> Vec<MidiFxSlot> {
    app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.extra.midi_fx.clone()).unwrap_or_default()
}

fn edit(app: &mut App, t: usize, what: &str, f: impl FnOnce(&mut Vec<MidiFxSlot>)) {
    if !what.is_empty() {
        app.checkpoint(what);
    }
    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
        f(&mut tr.extra.midi_fx);
    }
    app.dirty = true;
    push_slots(app, t);
}

fn new_slot(name: &str) -> MidiFxSlot {
    MidiFxSlot { name: name.to_string(), values: midifx::defaults(name), bypass: false }
}

// ---------------------------------------------------------------------------------------------
// the channel strip

const GREEN: Color32 = Color32::from_rgb(0x3d, 0x9a, 0x4a);

fn plugin_menu(ui: &mut egui::Ui, current: Option<&str>) -> Option<Option<String>> {
    let mut pick = None;
    ui.set_min_width(180.0);
    if current.is_some() && ui.button("No Plug-in").clicked() {
        pick = Some(None);
        ui.close();
    }
    if current.is_some() {
        ui.separator();
    }
    for n in midifx::NAMES {
        if ui.add(egui::Button::selectable(current == Some(n), n)).clicked() {
            pick = Some(Some(n.to_string()));
            ui.close();
        }
    }
    pick
}

/// Draws the MIDI FX slots from `y` in strip rect `r` for track `t`; returns the y after them.
pub fn strip_slots(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, mut y: f32, t: usize) -> f32 {
    let list = slots(app, t);
    let id = ui.scope_id().with(("midifx", t));
    let rect_at = |y: f32| Rect::from_min_size(pos2(r.min.x + 8.0, y), vec2(r.width() - 16.0, 20.0));
    let mut drop_on: Option<usize> = None;
    for (k, s) in list.iter().enumerate() {
        let sr = rect_at(y);
        let fill = if s.bypass { GREEN.gamma_multiply(0.35) } else { GREEN };
        p.rect_filled(sr, 4.0, fill);
        p.with_clip_rect(sr.shrink(2.0)).text(sr.center() + vec2(4.0, 0.0), Align2::CENTER_CENTER, &s.name, look::regular(11.0), if s.bypass { look::text_dim() } else { Color32::WHITE });
        let resp = ui.interact(sr, id.with(k), Sense::click_and_drag());
        let byp = Rect::from_min_size(sr.min, vec2(16.0, sr.height()));
        if resp.hovered() {
            // the bypass button at the label's left
            p.circle_stroke(byp.center(), 5.0, Stroke::new(1.2, Color32::WHITE));
        }
        if resp.drag_started() {
            app.mfx.drag_from = Some((t, k));
        }
        if app.mfx.drag_from.is_some_and(|d| d.0 == t && d.1 != k) && ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| sr.contains(h)) {
            p.rect_stroke(sr, 4.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
            if ui.input(|i| i.pointer.any_released()) {
                drop_on = Some(k);
            }
        }
        if resp.clicked() {
            if resp.interact_pointer_pos().is_some_and(|pp| byp.contains(pp)) {
                edit(app, t, if s.bypass { "Turn On MIDI Plug-in" } else { "Bypass MIDI Plug-in" }, |v| v[k].bypass = !v[k].bypass);
            } else {
                app.mfx.window = Some((t, k));
            }
        }
        let name = s.name.clone();
        egui::Popup::context_menu(&resp).show(|ui| {
            if let Some(pick) = plugin_menu(ui, Some(&name)) {
                match pick {
                    None => {
                        edit(app, t, "Remove MIDI Plug-in", |v| {
                            v.remove(k);
                        });
                        if app.mfx.window.is_some_and(|w| w.0 == t) {
                            app.mfx.window = None;
                        }
                    }
                    Some(n) => {
                        edit(app, t, "Insert MIDI Plug-in", |v| v[k] = new_slot(&n));
                        app.mfx.window = Some((t, k));
                    }
                }
            }
            ui.separator();
            // a green line inserts another without opening its window
            ui.menu_button("Insert Below", |ui| {
                if let Some(Some(n)) = plugin_menu(ui, None) {
                    edit(app, t, "Insert MIDI Plug-in", |v| v.insert(k + 1, new_slot(&n)));
                }
            });
        });
        resp.on_hover_text(format!("{name} (click: window; left edge: bypass; right-click: plug-ins; drag onto another to swap)"));
        y += 22.0;
    }
    if let (Some(k), Some((_, from))) = (drop_on, app.mfx.drag_from) {
        edit(app, t, "Move MIDI Plug-in", |v| {
            if from < v.len() && k < v.len() {
                // onto the top label: moves down below it; the bottom: moves up; between: swap
                if k == 0 || k + 1 == v.len() {
                    let s = v.remove(from);
                    v.insert(k, s);
                } else {
                    v.swap(from, k);
                }
            }
        });
        app.mfx.drag_from = None;
    }
    if ui.input(|i| i.pointer.any_released()) {
        app.mfx.drag_from = None;
    }
    if list.len() < 8 {
        let er = rect_at(y);
        p.rect_filled(er, 4.0, look::group_bg());
        p.with_clip_rect(er.shrink(2.0)).text(er.center(), Align2::CENTER_CENTER, "MIDI FX", look::regular(11.0), look::text_dim());
        let resp = ui.interact(er, id.with("empty"), Sense::click());
        egui::Popup::menu(&resp).show(|ui| {
            if let Some(Some(n)) = plugin_menu(ui, None) {
                let k = list.len();
                edit(app, t, "Insert MIDI Plug-in", |v| v.push(new_slot(&n)));
                app.mfx.window = Some((t, k));
            }
        });
        y += 22.0;
    }
    y
}

// ---------------------------------------------------------------------------------------------
// settings files

fn settings_files(name: &str) -> Vec<(String, std::path::PathBuf)> {
    crate::pluginwin::saved_settings(&format!("MIDI {name}"))
}

fn save_setting(name: &str, file: &str, values: &[f32]) -> bool {
    let Some(dir) = crate::pluginwin::settings_dir(&format!("MIDI {name}")) else { return false };
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let body: String = values.iter().enumerate().map(|(i, v)| format!("{i} {v}\n")).collect();
    std::fs::write(dir.join(format!("{}.txt", file.replace('/', "-"))), body).is_ok()
}

fn load_setting(name: &str, path: &std::path::Path) -> Option<Vec<f32>> {
    let t = std::fs::read_to_string(path).ok()?;
    let mut v = midifx::defaults(name);
    for l in t.lines() {
        let mut w = l.split_whitespace();
        let (Some(i), Some(x)) = (w.next().and_then(|x| x.parse::<usize>().ok()), w.next().and_then(|x| x.parse::<f32>().ok())) else { continue };
        if i < v.len() {
            v[i] = x;
        }
    }
    Some(v)
}

// ---------------------------------------------------------------------------------------------
// the window

fn is_black(k: u8) -> bool {
    matches!(k % 12, 1 | 3 | 6 | 8 | 10)
}

/// A keyboard from `lo` to `hi`: white keys in a row, black ones on top; `lit` keys filled,
/// `dots` keys marked; returns the key clicked.
fn keyboard(ui: &mut egui::Ui, rect: Rect, lo: u8, hi: u8, lit: &[u8], dots: &[u8], range: Option<(u8, u8)>, salt: &str) -> Option<u8> {
    let p = ui.painter_at(rect);
    let whites: Vec<u8> = (lo..=hi).filter(|k| !is_black(*k)).collect();
    let w = rect.width() / whites.len().max(1) as f32;
    let mut key_rects: Vec<(u8, Rect)> = Vec::new();
    for (i, &k) in whites.iter().enumerate() {
        let kr = Rect::from_min_size(pos2(rect.min.x + i as f32 * w, rect.min.y), vec2(w - 1.0, rect.height()));
        let inr = range.is_some_and(|(a, b)| k >= a.min(b) && k <= a.max(b));
        let fill = if lit.contains(&k) { look::accent() } else if inr { Color32::from_rgb(0xc8, 0xdc, 0xf5) } else { Color32::from_gray(0xee) };
        p.rect_filled(kr, 2.0, fill);
        if dots.contains(&k) {
            p.circle_filled(pos2(kr.center().x, kr.max.y - 8.0), 3.0, Color32::from_rgb(0x2a, 0x6c, 0xd6));
        }
        if k % 12 == 0 {
            p.text(pos2(kr.center().x, kr.max.y - 18.0), Align2::CENTER_CENTER, midifx::note_name(k), look::regular(8.5), Color32::DARK_GRAY);
        }
        key_rects.push((k, kr));
    }
    for (i, &k) in whites.iter().enumerate() {
        let b = k + 1;
        if b <= hi && is_black(b) {
            let x = rect.min.x + (i as f32 + 1.0) * w - w * 0.3;
            let kr = Rect::from_min_size(pos2(x, rect.min.y), vec2(w * 0.6, rect.height() * 0.6));
            let inr = range.is_some_and(|(a, c)| b >= a.min(c) && b <= a.max(c));
            p.rect_filled(kr, 2.0, if lit.contains(&b) { look::accent() } else if inr { Color32::from_rgb(0x3a, 0x5a, 0x8a) } else { Color32::from_gray(0x22) });
            if dots.contains(&b) {
                p.circle_filled(pos2(kr.center().x, kr.max.y - 6.0), 2.5, Color32::from_rgb(0x7a, 0xb4, 0xff));
            }
            key_rects.insert(0, (b, kr));
        }
    }
    let resp = ui.interact(rect, ui.scope_id().with(salt), Sense::click());
    if resp.clicked() {
        let pp = resp.interact_pointer_pos()?;
        return key_rects.iter().find(|(_, r)| r.contains(pp)).map(|(k, _)| *k);
    }
    None
}

/// LG_MIDIFX="Arpeggiator,Transposer" (checks): those plug-ins into track 1's MIDI FX slots
/// once, the first one's window open.
fn env_hook(app: &mut App) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    if app.project.is_none() {
        return;
    }
    ONCE.call_once(|| {
        let Ok(list) = std::env::var("LG_MIDIFX") else { return };
        for n in list.split(',').map(str::trim).filter(|n| midifx::NAMES.contains(n)) {
            edit(app, 0, "", |v| v.push(new_slot(n)));
        }
        if !slots(app, 0).is_empty() {
            app.mfx.window = Some((0, 0));
        }
    });
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    env_hook(app);
    let Some((t, k)) = app.mfx.window else { return };
    let Some(slot) = slots(app, t).get(k).cloned() else {
        app.mfx.window = None;
        return;
    };
    let track = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    let (status, playing) = app.engine.lock().ok().map(|e| (e.fx.status.get(t).and_then(|s| s.get(k).cloned()).unwrap_or_default(), e.playing)).unwrap_or_default();
    let params = midifx::params(&slot.name);
    let mut values = slot.values.clone();
    values.resize(midifx::data_at(&slot.name) + midifx::data_len(&slot.name), 0.0);
    let mut open = true;
    let mut changed: Option<(&'static str, bool)> = None;
    let mut new_values: Option<Vec<f32>> = None;
    let mut bypass_toggle = false;
    let mut cmd: Option<&'static str> = None;
    egui::Window::new(format!("{track} — {}", slot.name)).id(egui::Id::unique(("midifx-window", t, k))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(600.0, 560.0)).show(ctx, |ui| {
        // the header: bypass, the setting pop-up, copy / paste
        ui.horizontal(|ui| {
            if ui.add(egui::Button::selectable(!slot.bypass, if slot.bypass { "Bypassed" } else { "On" })).on_hover_text("Bypass (keeps the settings)").clicked() {
                bypass_toggle = true;
            }
            ui.menu_button("Setting ▾", |ui| {
                ui.set_min_width(200.0);
                if ui.button("Recall Default").clicked() {
                    new_values = Some(midifx::defaults(&slot.name));
                    ui.close();
                }
                if ui.button("Save Setting As…").clicked() {
                    app.mfx.save_name = Some(String::new());
                    ui.close();
                }
                let files = settings_files(&slot.name);
                if !files.is_empty() {
                    ui.separator();
                }
                for (n, path) in files {
                    if ui.button(&n).clicked() {
                        new_values = load_setting(&slot.name, &path);
                        ui.close();
                    }
                }
            });
            if ui.button("Copy").on_hover_text("Copy the settings").clicked() {
                app.mfx.clipboard = Some((slot.name.clone(), values.clone()));
            }
            let can_paste = app.mfx.clipboard.as_ref().is_some_and(|c| c.0 == slot.name);
            if ui.add_enabled(can_paste, egui::Button::new("Paste")).clicked() {
                new_values = app.mfx.clipboard.as_ref().map(|c| c.1.clone());
            }
            if let Some(name) = app.mfx.save_name.as_mut() {
                let te = ui.add(egui::TextEdit::singleline(name).hint_text("Setting name").desired_width(140.0));
                crate::widgets::focus_once(ui, &te);
                if te.lost_focus() {
                    let n = name.trim().to_string();
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !n.is_empty() {
                        save_setting(&slot.name, &n, &values);
                    }
                    app.mfx.save_name = None;
                }
            }
        });
        ui.separator();
        match slot.name.as_str() {
            "Arpeggiator" => arp_editor(ui, &mut values, &status, playing, &mut changed, &mut cmd),
            "Chord Trigger" => chord_editor(app, ui, t, &mut values, &status, &mut changed),
            "Transposer" => {
                ui.label("User scale (Scale: User): keys on are in the scale");
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().min(360.0), 60.0), Sense::hover());
                let dots: Vec<u8> = (0..12u8).filter(|i| values[3 + *i as usize] >= 0.5).map(|i| 60 + i).collect();
                if let Some(key) = keyboard(ui, r, 60, 71, &dots, &[], None, "user-scale") {
                    let i = 3 + (key - 60) as usize;
                    values[i] = if values[i] >= 0.5 { 0.0 } else { 1.0 };
                    changed = Some(("Change User Scale", true));
                }
            }
            "Modulator" => {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
                let p = ui.painter_at(r);
                p.rect_filled(r, 4.0, look::lcd_bg());
                p.text(r.left_center() + vec2(8.0, -10.0), Align2::LEFT_CENTER, format!("LFO {:+.2}", status.lfo), look::regular(11.0), look::lcd_text());
                p.text(r.left_center() + vec2(8.0, 10.0), Align2::LEFT_CENTER, format!("Envelope {:.2}", status.env), look::regular(11.0), look::lcd_text());
                let x = r.center().x + status.lfo * r.width() * 0.2;
                p.circle_filled(pos2(x, r.center().y), 4.0, look::accent());
                ctx.request_repaint();
            }
            "Scripter" => {
                ui.label("Scripter's JavaScript editor isn't available in Studio: the slot passes events through and keeps its four parameters.");
            }
            _ => {}
        }
        ui.separator();
        let shown: Vec<lg_dsp::Param> = params.iter().filter(|p| !(slot.name == "Arpeggiator" && (p.id <= arp_header_max()))).cloned().collect();
        let vals: Vec<f32> = shown.iter().map(|p| values[p.id as usize]).collect();
        let texts = vec![None; shown.len()];
        for (id, v) in crate::pluginwin::controls(ui, &shown, &vals, &texts, &format!("mfx-{t}-{k}")) {
            if let Some(x) = values.get_mut(id as usize) {
                *x = v;
            }
            changed = Some(("Change MIDI Plug-in Parameter", false));
        }
    });
    if !open {
        app.mfx.window = None;
        app.mfx.learn = None;
    }
    if bypass_toggle {
        edit(app, t, "Bypass MIDI Plug-in", |v| v[k].bypass = !v[k].bypass);
    }
    if let Some(v) = new_values {
        edit(app, t, "Load MIDI Plug-in Setting", |s| s[k].values = v);
    } else if let Some((what, step)) = changed {
        // a drag is one undo step: a checkpoint when it starts
        let dragging = ctx.input(|i| i.pointer.any_down());
        let first = !ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("mfx-dragging")).unwrap_or(false));
        let what = if step || first { what } else { "" };
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("mfx-dragging"), dragging));
        edit(app, t, what, |s| s[k].values = values);
    } else if !ctx.input(|i| i.pointer.any_down()) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("mfx-dragging"), false));
    }
    if let Some(c) = cmd {
        app.engine.lock().unwrap_or_else(|e| e.into_inner()).fx.commands.push((t, k, c.to_string()));
    }
    if status.step.is_some() || !status.output.is_empty() {
        ctx.request_repaint();
    }
}

fn arp_header_max() -> u32 {
    midifx::arp::RANGE
}

/// The Arpeggiator's Control and Note Order area, then the grid.
fn arp_editor(ui: &mut egui::Ui, v: &mut [f32], st: &Status, playing: bool, changed: &mut Option<(&'static str, bool)>, cmd: &mut Option<&'static str>) {
    use midifx::arp::*;
    ui.horizontal(|ui| {
        // playing: Play turns arpeggiation on / off; stopped: it runs the arpeggio from its
        // first step (or stops it) without the song
        let play = v[PLAY as usize] >= 0.5;
        let lit = if playing { play } else { play && st.free };
        if ui.add(egui::Button::selectable(lit, "Play")).on_hover_text("Starts or stops arpeggiation (while Logic is stopped: from the first step)").clicked() {
            if playing || !play {
                v[PLAY as usize] = if play { 0.0 } else { 1.0 };
                *changed = Some(("Arpeggiator Play", true));
            }
            if !playing {
                *cmd = Some(if lit { "Stop" } else { "Play" });
            }
        }
        let latch = v[LATCH as usize] >= 0.5;
        if ui.add(egui::Button::selectable(latch, "Latch")).clicked() {
            v[LATCH as usize] = if latch { 0.0 } else { 1.0 };
            *changed = Some(("Arpeggiator Latch", true));
        }
        if ui.button("Clear").on_hover_text("Empties the latch memory and stops the arpeggio").clicked() {
            *cmd = Some("Clear");
        }
        if ui.button("Delete Last").clicked() {
            *cmd = Some("Delete Last");
        }
        let rate = midifx::params("Arpeggiator").into_iter().find(|p| p.id == RATE).map(|p| p.choices).unwrap_or(&[]);
        let r = v[RATE as usize].round() as usize;
        egui::ComboBox::from_id_salt("arp-rate").selected_text(format!("Rate {}", rate.get(r).copied().unwrap_or(""))).show_ui(ui, |ui| {
            for (i, n) in rate.iter().enumerate() {
                if ui.selectable_label(i == r, *n).clicked() {
                    v[RATE as usize] = i as f32;
                    *changed = Some(("Arpeggiator Rate", true));
                }
            }
        });
        // the Rate light: lit each new cycle
        let (lr, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        ui.painter().circle_filled(lr.center(), 5.0, if st.step == Some(0) { look::accent() } else { look::group_bg() });
    });
    ui.horizontal(|ui| {
        for (i, n) in ["Up", "Down", "Up/Down", "Outside-in", "Random", "As Played"].iter().enumerate() {
            if ui.add(egui::Button::selectable(v[ORDER as usize].round() as usize == i, *n)).clicked() {
                v[ORDER as usize] = i as f32;
                *changed = Some(("Arpeggiator Note Order", true));
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Variation");
        for i in 0..4 {
            if ui.add(egui::Button::selectable(v[VARIATION as usize].round() as usize == i, format!("{}", i + 1))).clicked() {
                v[VARIATION as usize] = i as f32;
                *changed = Some(("Arpeggiator Variation", true));
            }
        }
        ui.separator();
        let inv = v[OCT_MODE as usize] >= 0.5;
        if ui.add(egui::Button::selectable(inv, if inv { "Inversions" } else { "Oct Range" })).on_hover_text("Switches the four positions between Octave Range and Inversions").clicked() {
            v[OCT_MODE as usize] = if inv { 0.0 } else { 1.0 };
            *changed = Some(("Arpeggiator Oct Range/Inversions", true));
        }
        for i in 0..4 {
            if ui.add(egui::Button::selectable(v[RANGE as usize].round() as usize == i, format!("{}", i + 1))).clicked() {
                v[RANGE as usize] = i as f32;
                *changed = Some(("Arpeggiator Range", true));
            }
        }
    });
    ui.add_space(4.0);
    // the grid (Grid mode); in Live mode it shows but doesn't edit
    let grid = v[PATTERN as usize] >= 0.5;
    ui.horizontal(|ui| {
        if ui.add(egui::Button::selectable(!grid, "Live")).clicked() {
            v[PATTERN as usize] = 0.0;
            *changed = Some(("Arpeggiator Live", true));
        }
        if ui.add(egui::Button::selectable(grid, "Grid")).clicked() {
            v[PATTERN as usize] = 1.0;
            *changed = Some(("Arpeggiator Grid", true));
        }
        ui.label(format!("Steps: {}", v[GRID_LEN as usize].round()));
    });
    let len = (v[GRID_LEN as usize].round() as usize).clamp(1, STEPS);
    let (gr, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 110.0), Sense::click_and_drag());
    let p = ui.painter_at(gr);
    p.rect_filled(gr, 4.0, look::lcd_bg());
    let shown = STEPS;
    let w = gr.width() / shown as f32;
    let bars_top = gr.min.y + 18.0;
    let bars_bottom = gr.max.y - 30.0;
    for s in 0..shown {
        let b = DATA + s * STEP_FLOATS;
        let x0 = gr.min.x + s as f32 * w;
        let on = v[b] >= 0.5;
        let active = s < len;
        // the cycle length bar with its running light
        let cb = Rect::from_min_size(pos2(x0 + 1.0, gr.min.y + 3.0), vec2(w - 2.0, 8.0));
        p.rect_filled(cb, 2.0, if st.step == Some(s) { look::accent() } else if active { look::text_dim().gamma_multiply(0.5) } else { look::group_bg() });
        let h = (bars_bottom - bars_top) * v[b + 1].clamp(0.0, 127.0) / 127.0;
        let bw = (w - 3.0) * (v[b + 2].clamp(1.0, 100.0) / 100.0);
        let col = if !active { look::group_bg() } else if !on { look::text_dim().gamma_multiply(0.3) } else if v[b + 3] >= 0.5 { Color32::from_rgb(0xd6, 0x9a, 0x3a) } else { look::lcd_text() };
        p.rect_filled(Rect::from_min_max(pos2(x0 + 1.5, bars_bottom - h), pos2(x0 + 1.5 + bw, bars_bottom)), 1.0, col);
        // on/off, tie, chord buttons
        let row = |k: usize| Rect::from_min_size(pos2(x0 + 1.0, bars_bottom + 2.0 + k as f32 * 9.0), vec2(w - 2.0, 8.0));
        p.rect_filled(row(0), 1.5, if on { look::accent().gamma_multiply(0.8) } else { look::group_bg() });
        p.rect_filled(row(1), 1.5, if v[b + 3] >= 0.5 { Color32::from_rgb(0xd6, 0x9a, 0x3a) } else { look::group_bg() });
        p.rect_filled(row(2), 1.5, if v[b + 4] >= 0.5 { Color32::from_rgb(0x8e, 0x6c, 0xd6) } else { look::group_bg() });
    }
    p.text(pos2(gr.max.x - 4.0, gr.max.y - 3.0), Align2::RIGHT_BOTTOM, "on / tie / chord", look::regular(9.0), look::lcd_dim());
    if grid {
        if let Some(pp) = resp.interact_pointer_pos() {
            let s = (((pp.x - gr.min.x) / w).floor().max(0.0) as usize).min(shown - 1);
            let b = DATA + s * STEP_FLOATS;
            if pp.y < bars_top - 4.0 && resp.dragged() {
                // the cycle length bar's end: drag to change the grid length
                v[GRID_LEN as usize] = (s + 1) as f32;
                *changed = Some(("Arpeggiator Grid Length", false));
            } else if pp.y < bars_top - 4.0 && resp.clicked() {
                v[GRID_LEN as usize] = (s + 1) as f32;
                *changed = Some(("Arpeggiator Grid Length", true));
            } else if pp.y < bars_bottom {
                if resp.dragged() || resp.clicked() {
                    let shift = ui.input(|i| i.modifiers.shift);
                    let horiz = ui.input(|i| i.pointer.delta().x.abs() > i.pointer.delta().y.abs());
                    if resp.dragged() && horiz {
                        // left / right: the step's length, 25 / 50 / 75 / 100 % (Shift: free)
                        let frac = ((pp.x - (gr.min.x + s as f32 * w)) / w).clamp(0.05, 1.0) * 100.0;
                        v[b + 2] = if shift { frac.round() } else { ((frac / 25.0).round() * 25.0).clamp(25.0, 100.0) };
                    } else {
                        v[b + 1] = ((bars_bottom - pp.y) / (bars_bottom - bars_top) * 127.0).round().clamp(1.0, 127.0);
                    }
                    *changed = Some(("Arpeggiator Step", resp.clicked()));
                }
            } else if resp.clicked() {
                let k = (((pp.y - bars_bottom - 2.0) / 9.0).floor().max(0.0) as usize).min(2);
                let i = b + [0, 3, 4][k];
                v[i] = if v[i] >= 0.5 { 0.0 } else { 1.0 };
                *changed = Some(("Arpeggiator Step", true));
            }
        }
    }
    // the output notes on a keyboard
    let (kr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    keyboard(ui, kr, 36, 96, &st.input, &st.output, None, "arp-keys");
}

fn chord_editor(app: &mut App, ui: &mut egui::Ui, t: usize, v: &mut Vec<f32>, st: &Status, changed: &mut Option<(&'static str, bool)>) {
    use midifx::chord::*;
    let multi = v[MODE as usize] >= 0.5;
    let learn = app.mfx.learn;
    ui.horizontal(|ui| {
        if ui.add(egui::Button::selectable(!multi, "Single")).clicked() {
            v[MODE as usize] = 0.0;
            *changed = Some(("Chord Trigger Mode", true));
        }
        if ui.add(egui::Button::selectable(multi, "Multi")).clicked() {
            v[MODE as usize] = 1.0;
            *changed = Some(("Chord Trigger Mode", true));
        }
        ui.separator();
        let label = match learn {
            None => "Learn",
            Some(None) => "Trigger Key",
            Some(Some(_)) => "Chord",
        };
        if ui.add(egui::Button::selectable(learn.is_some(), label)).on_hover_text("Learn: click a trigger key, then the chord's notes below; Learn again ends").clicked() {
            app.mfx.learn = if learn.is_some() { None } else { Some(None) };
        }
        let clear_label = if app.mfx.clear_armed { "Trigger Key" } else { "Clear" };
        if ui.button(clear_label).on_hover_text("Single: erases the chord. Multi: click a trigger key to erase its chord (Option-click: all)").clicked() {
            if !multi {
                set_chord(v, 128, -1, &[]);
                *changed = Some(("Clear Chord", true));
            } else if ui.input(|i| i.modifiers.alt) {
                for k in 0..128 {
                    set_chord(v, k, -1, &[]);
                }
                *changed = Some(("Clear All Chords", true));
            } else {
                app.mfx.clear_armed = !app.mfx.clear_armed;
            }
        }
    });
    let (lo, hi) = (v[LO as usize].round() as u8, v[HI as usize].round() as u8);
    // the upper keyboard: the trigger range (blue), incoming notes, dots on keys with chords
    let dots: Vec<u8> = (0..128u8).filter(|&k| if multi { !chord_of(v, k as usize).1.is_empty() } else { chord_of(v, 128).0 == k as i32 }).collect();
    let (ur, _) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::hover());
    if let Some(key) = keyboard(ui, ur, 24, 96, &st.input, &dots, Some((lo, hi)), "ct-upper") {
        if app.mfx.clear_armed {
            set_chord(v, key as usize, -1, &[]);
            app.mfx.clear_armed = false;
            *changed = Some(("Clear Chord", true));
        } else if learn.is_some() {
            app.mfx.learn = Some(Some(key));
            if !multi {
                // Single: one chord, learned relative to its trigger key
                let notes = chord_of(v, 128).1;
                set_chord(v, 128, key as i32, &notes);
                *changed = Some(("Learn Chord", true));
            }
        } else {
            let notes = midifx::chord::ChordTrigger::notes_for(v, key);
            let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
            for n in notes {
                e.live_note(t, n, 100);
                e.live_note(t, n, 0);
            }
        }
    }
    ui.label(egui::RichText::new(format!("Trigger range {} – {}", midifx::note_name(lo.min(hi)), midifx::note_name(lo.max(hi)))).small());
    // the lower keyboard: the chord's notes (click during Learn to add / remove them)
    let cur = match learn {
        Some(Some(k)) => if multi { chord_of(v, k as usize).1 } else { chord_of(v, 128).1 },
        _ => st.output.clone(),
    };
    let (lr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::hover());
    if let Some(note) = keyboard(ui, lr, 24, 96, &[], &cur, None, "ct-lower") {
        if let Some(Some(k)) = learn {
            let slot = if multi { k as usize } else { 128 };
            let (key, mut notes) = chord_of(v, slot);
            if let Some(i) = notes.iter().position(|&n| n == note) {
                notes.remove(i);
            } else if notes.len() < NOTES {
                notes.push(note);
                let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
                e.live_note(t, note, 100);
                e.live_note(t, note, 0);
            }
            notes.sort();
            set_chord(v, slot, if multi { k as i32 } else if key >= 0 { key } else { k as i32 }, &notes);
            *changed = Some(("Learn Chord", true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_controls_follow_region_and_loop() {
        let mut t = lg_core::Track::new("K", lg_core::TrackKind::Instrument);
        let mut r = lg_core::Region { name: "r".into(), start: 3840, length: 3840, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
        r.ext.events.push(lg_core::events::Control { kind: 0xB0, channel: 0, pos: 100, number: 1, value: 64 });
        r.ext.events.push(lg_core::regions::bend_event(200, 0, 0));
        r.ext.events.push(lg_core::events::Control { kind: 0xB0, channel: 0, pos: 5000, number: 1, value: 1 });
        r.props.looped = true;
        r.props.loop_to = 3840 * 3;
        t.regions.push(r);
        let c = region_controls(&t, 100_000);
        // two in the region (the one past its end is not played), repeated once by the loop
        assert_eq!(c.iter().map(|x| x.0).collect::<Vec<_>>(), vec![3940, 4040, 7780, 7880]);
        assert_eq!(c[1].1, Msg::Bend { ch: 0, val: 0 });
    }
}
