//! The window: owns the session, routes keyboard and pointer input, draws the calculator layer
//! (keys, display, toolbar) and the overlays (history, menus, sheets) above it.

use crate::anim::{self, EqAnim, Morph, Tween};
use crate::draw::{self, col, fade, Look};
use crate::{convert, fonts, history, menu, notes, png, settings};
use calc_core::calc::Display;
use calc_core::layout::{Insets, Layout, Mode};
use calc_core::session::{key_for_char, ConvState, Session};
use calc_core::theme::{Family, Press, Theme};
use calc_core::units::RATES_URL;
use egui::emath::TSTransform;
use egui::{pos2, vec2, Area, Color32, Event, Id, Key, LayerId, Order, Pos2, Rect, Sense, Ui};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    #[default]
    None,
    Units,
    Settings,
}

pub enum PopAct {
    Copy,
    Paste,
    CopyText(String),
    DeleteNote(u64),
}

/// A small edit menu (Copy / Paste …) at a point.
pub struct Popup {
    pub pos: Pos2,
    pub items: Vec<(&'static str, PopAct)>,
}

/// What is open above (or instead of) the keypad.
pub struct Overlay {
    pub menu: bool,
    pub menu_anchor: Rect,
    pub history: bool,
    pub hist_edit: bool,
    pub hist_sel: HashSet<u64>,
    pub hist_swiped: Option<u64>,
    pub sheet_open: bool,
    pub sheet: Sheet,
    pub sheet_drag: f32,
    pub units_top: bool,
    pub units_cat: String,
    pub units_q: String,
    pub units_scrolled: bool,
    pub units_list_scrolled: bool,
    pub popup: Option<Popup>,
    pub note: Option<u64>,
    pub note_text: String,
    pub graph: Option<notes::GraphView>,
}

impl Default for Overlay {
    fn default() -> Self {
        Overlay {
            menu: false,
            menu_anchor: Rect::NOTHING,
            history: false,
            hist_edit: false,
            hist_sel: HashSet::new(),
            hist_swiped: None,
            sheet_open: false,
            sheet: Sheet::None,
            sheet_drag: 0.0,
            units_top: false,
            units_cat: String::new(),
            units_q: String::new(),
            units_scrolled: false,
            units_list_scrolled: false,
            popup: None,
            note: None,
            note_text: String::new(),
            graph: None,
        }
    }
}

impl Overlay {
    /// Something covers the calculator and takes the clicks.
    pub fn blocking(&self) -> bool {
        self.menu || self.history || self.sheet_open || self.popup.is_some()
    }
    /// Closes the top-most thing; false when nothing was open.
    pub fn close_top(&mut self) -> bool {
        if self.popup.take().is_some() {
            return true;
        }
        if self.menu {
            self.menu = false;
            return true;
        }
        if self.sheet_open {
            self.sheet_open = false;
            return true;
        }
        if self.history {
            self.history = false;
            return true;
        }
        false
    }
}

/// Environment test hooks (see README).
#[derive(Default)]
pub struct Hooks {
    pub size: Option<(f32, f32)>,
    theme: Option<String>,
    keys: VecDeque<String>,
    open: Option<String>,
    snap: Option<PathBuf>,
    pub data: Option<PathBuf>,
    settle: Option<f64>,
    shot_at: f64,
    /// `@wait:secs` holds the next key until then
    wait_until: f64,
    shot_done: Arc<AtomicBool>,
    /// the wake-up thread runs while this is set
    live: Arc<AtomicBool>,
    started: bool,
}

impl Hooks {
    pub fn from_env() -> Hooks {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Hooks {
            size: var("CALC_SIZE").and_then(|s| {
                let (w, h) = s.split_once(['x', 'X'])?;
                Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
            }),
            theme: var("CALC_THEME"),
            keys: var("CALC_KEYS").map(|s| s.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect()).unwrap_or_default(),
            open: var("CALC_OPEN"),
            snap: var("CALC_SNAP").map(PathBuf::from),
            data: var("CALC_DATA").map(PathBuf::from),
            shot_at: f64::NEG_INFINITY,
            ..Default::default()
        }
    }
    fn active(&self) -> bool {
        self.snap.is_some() || !self.keys.is_empty() || self.open.is_some()
    }
}

pub const CAFFEINE: &str = "Caffeine Half-Life\nIf you consume 100mg of caffeine, what is the six hour half life of the caffeine in your body?\nQuantity = 100mg\nHalfLife = 6\ny= Quantity × (0.5^(x / HalfLife))";

pub struct App {
    pub s: Session,
    pub lay: Layout,
    pub disp: Display,
    pub conv: Option<ConvState>,
    pub morph: Morph,
    pub tool_glide: anim::Glide,
    /// the phone history panel's open progress (0..1)
    pub hist_t: f32,
    pub press: anim::Press,
    pub eq: Option<EqAnim>,
    pub main_font: f32,
    font_tween: Tween,
    pub ov: Overlay,
    pub utc: i32,
    pub now: f64,
    pub font_name: String,
    pub swap_t0: f64,
    /// the history row last loaded (highlighted in the sidebar)
    pub hist_current: Option<u64>,
    hooks: Hooks,
    rates_rx: Option<Receiver<String>>,
    rates_tried: bool,
    size: egui::Vec2,
    sig: (bool, bool, bool, bool, bool, &'static str, Mode),
    visuals_for: &'static str,
    disp_swiped: bool,
    disp_down: Option<f64>,
    /// input arrived since the last layout: lay out again next frame
    dirty: bool,
    pub ctx: egui::Context,
    prof: Prof,
}

/// CALC_PROF=1: prints the CPU time of `update` (frames, mean, max) once a second.
#[derive(Default)]
struct Prof {
    on: bool,
    n: u32,
    sum: f64,
    max: f64,
    /// the previous frame's update + tessellation + paint (eframe's cpu_usage)
    whole: f64,
    whole_max: f64,
    /// time in relayout, the tablet sidebar and the keypad
    phase: [f64; 3],
    since: Option<std::time::Instant>,
}

impl Prof {
    fn add(&mut self, ms: f64, whole: f64) {
        self.n += 1;
        self.whole += whole;
        self.whole_max = self.whole_max.max(whole);
        self.sum += ms;
        self.max = self.max.max(ms);
        let since = *self.since.get_or_insert_with(std::time::Instant::now);
        if since.elapsed().as_secs_f64() >= 1.0 {
            self.flush();
        }
    }
    fn flush(&mut self) {
        if self.n > 0 {
            eprintln!("prof: {} frames, update mean {:.2} max {:.2} ms; whole frame mean {:.2} max {:.2} ms", self.n, self.sum / self.n as f64, self.max, self.whole / self.n as f64, self.whole_max);
            eprintln!("  phases per frame: relayout {:.2} sidebar {:.2} keypad {:.2}", self.phase[0] / self.n as f64, self.phase[1] / self.n as f64, self.phase[2] / self.n as f64);
            self.phase = [0.0; 3];
        }
        (self.n, self.sum, self.max, self.whole, self.whole_max, self.since) = (0, 0.0, 0.0, 0.0, 0.0, None);
    }
}

impl Drop for Prof {
    fn drop(&mut self) {
        if self.on {
            self.flush();
        }
    }
}

fn data_dir(h: &Hooks) -> PathBuf {
    if let Some(d) = &h.data {
        return d.clone();
    }
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("app.bramble.calculator")
}

/// Seconds east of UTC, from `date +%z` (no timezone crate).
fn utc_offset() -> i32 {
    let out = std::process::Command::new("date").arg("+%z").output().ok();
    let s = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    if s.len() == 5 {
        let sign = if s.starts_with('-') { -1 } else { 1 };
        let h: i32 = s[1..3].parse().unwrap_or(0);
        let m: i32 = s[3..5].parse().unwrap_or(0);
        return sign * (h * 3600 + m * 60);
    }
    0
}

impl App {
    pub fn new(cc: &eframe::CreationContext, hooks: Hooks) -> App {
        let mut s = Session::new(Some(data_dir(&hooks)));
        if let Some(t) = &hooks.theme {
            s.set_theme(t);
        }
        let font_name = fonts::install(&cc.egui_ctx);
        if hooks.active() {
            // keeps the hooks stepping (a hidden window gets no frames otherwise); stops once
            // they're done, so the app idles at zero repaints
            let c = cc.egui_ctx.clone();
            let live = hooks.live.clone();
            live.store(true, Ordering::SeqCst);
            std::thread::spawn(move || {
                while live.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    c.request_repaint();
                }
            });
        }
        let lay = s.layout();
        let disp = s.display();
        App {
            lay,
            disp,
            conv: None,
            s,
            morph: Morph::default(),
            tool_glide: anim::Glide::default(),
            hist_t: 0.0,
            press: anim::Press::default(),
            eq: None,
            main_font: 80.0,
            font_tween: Tween::default(),
            ov: Overlay::default(),
            utc: utc_offset(),
            now: 0.0,
            font_name,
            swap_t0: -10.0,
            hist_current: None,
            hooks,
            rates_rx: None,
            rates_tried: false,
            size: egui::Vec2::ZERO,
            sig: (false, false, false, false, false, "", Mode::Basic),
            visuals_for: "",
            disp_swiped: false,
            disp_down: None,
            dirty: true,
            ctx: cc.egui_ctx.clone(),
            prof: Prof { on: std::env::var("CALC_PROF").is_ok_and(|v| v == "1"), ..Default::default() },
        }
    }

    pub fn theme(&self) -> Theme {
        self.s.theme().clone()
    }

    /// Haptic tick: Android only (settings.haptics is kept for it).
    fn tick(&self) {}

    /// A key id, toolbar id or one of the app's own keys.
    pub fn act(&mut self, id: &str) {
        match id {
            "calc" | "mode" => {
                let a = draw::rect(self.lay.menu_anchor);
                self.open_menu(a);
            }
            "history" => {
                let t = self.s.theme();
                if self.lay.tablet && self.lay.w >= 820.0 && !t.family.classic() {
                    self.s.settings.sidebar_open = !self.s.settings.sidebar_open;
                    self.s.save_settings();
                } else {
                    self.ov.history = true;
                }
            }
            _ => self.press_key(id),
        }
        self.tick();
        self.ctx.request_repaint();
    }

    pub fn open_menu(&mut self, anchor: Rect) {
        self.ov.menu_anchor = anchor;
        self.ov.menu = true;
    }

    pub fn press_key(&mut self, id: &str) {
        let before = self.s.display();
        self.hist_current = None;
        self.s.press(id);
        let after = self.s.display();
        if id == "eq" && after.is_result && !before.is_result && !before.main.is_empty() {
            self.eq = Some(EqAnim { t0: self.now, from: before.main, from_size: self.main_font });
        }
        self.disp = after;
    }

    pub fn set_mode(&mut self, m: Mode) {
        if m != Mode::Notes {
            self.close_note();
        }
        self.s.set_mode(m);
    }

    pub fn set_convert(&mut self, on: bool) {
        self.s.set_convert(on);
        if on && self.s.settings.mode == Mode::Notes {
            self.s.set_mode(Mode::Basic);
        }
    }

    pub fn open_note(&mut self, id: u64) {
        if let Some(n) = self.s.notes.get(id) {
            self.ov.note_text = n.text.clone();
            self.ov.note = Some(id);
            self.ov.graph = None;
        }
    }

    /// Leaves the editor; an empty note is discarded.
    pub fn close_note(&mut self) {
        if let Some(id) = self.ov.note.take() {
            if self.ov.note_text.trim().is_empty() {
                self.s.notes.delete(id);
                self.s.save_notes();
            }
        }
        self.ov.graph = None;
    }

    pub fn open_units(&mut self, top: bool) {
        self.ov.units_top = top;
        self.ov.units_cat = self.s.settings.conv_cat.clone();
        self.ov.units_q.clear();
        self.ov.units_scrolled = false;
        self.ov.units_list_scrolled = false;
        self.ov.sheet = Sheet::Units;
        self.ov.sheet_open = true;
        self.ov.sheet_drag = 0.0;
    }

    pub fn open_settings(&mut self) {
        self.ov.sheet = Sheet::Settings;
        self.ov.sheet_open = true;
        self.ov.sheet_drag = 0.0;
    }

    pub fn popup_act(&mut self, a: PopAct) {
        match a {
            PopAct::Copy => self.ctx.copy_text(self.s.copy_text()),
            PopAct::Paste => self.ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
            PopAct::CopyText(t) => self.ctx.copy_text(t),
            PopAct::DeleteNote(id) => {
                self.s.notes.delete(id);
                self.s.save_notes();
            }
        }
    }

    // ---- per frame ----

    fn hooks_step(&mut self, ctx: &egui::Context) {
        if !self.hooks.active() {
            self.hooks.live.store(false, Ordering::SeqCst);
            return;
        }
        ctx.request_repaint();
        if !self.hooks.started {
            // let the first frame lay out before pressing anything
            self.hooks.started = true;
            return;
        }
        if self.now < self.hooks.wait_until {
            return;
        }
        if let Some(k) = self.hooks.keys.pop_front() {
            self.hook_key(&k);
            return;
        }
        if let Some(o) = self.hooks.open.take() {
            match o.as_str() {
                "history" => self.act("history"),
                "history-edit" => {
                    self.act("history");
                    self.ov.hist_edit = true;
                    self.ov.hist_sel.extend(self.s.history.entries.first().map(|e| e.id));
                }
                "menu" => self.act("mode"),
                "units" => {
                    if !self.s.settings.convert {
                        self.set_convert(true);
                    }
                    self.open_units(false);
                }
                "settings" => self.open_settings(),
                "notes" => {
                    self.set_mode(Mode::Notes);
                    self.close_note();
                }
                "note" => {
                    self.set_mode(Mode::Notes);
                    if let Some(id) = self.s.notes.sorted().first().map(|n| n.id) {
                        self.open_note(id);
                    }
                }
                _ => {}
            }
            return;
        }
        let Some(path) = self.hooks.snap.clone() else { return };
        let settle = *self.hooks.settle.get_or_insert(self.now);
        if self.hooks.shot_done.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if self.now - settle >= 1.0 && self.now - self.hooks.shot_at > 2.0 {
            self.hooks.shot_at = self.now;
            let done = self.hooks.shot_done.clone();
            let c = ctx.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                if let Err(e) = png::save(&path, img.size[0], img.size[1], &img.pixels) {
                    eprintln!("snapshot: {e}");
                }
                done.store(true, Ordering::SeqCst);
                c.request_repaint();
            })));
        }
    }

    fn hook_key(&mut self, k: &str) {
        let Some(cmd) = k.strip_prefix('@') else {
            self.act(k);
            return;
        };
        let parts: Vec<&str> = cmd.split(':').collect();
        match parts[0] {
            "basic" => self.set_mode(Mode::Basic),
            "sci" => self.set_mode(Mode::Scientific),
            "notes" => self.set_mode(Mode::Notes),
            "convert" => self.set_convert(true),
            "noconvert" => self.set_convert(false),
            "sidebar" => {
                self.s.settings.sidebar_open = parts.get(1) != Some(&"off");
                self.s.save_settings();
            }
            "unit" if parts.len() == 4 => {
                self.s.set_unit(true, parts[1], parts[2]);
                self.s.set_unit(false, parts[1], parts[3]);
            }
            "style" => self.s.set_style(match parts.get(1).copied() {
                Some("Dot") => calc_core::format::NumberStyle::Dot,
                Some("Space") => calc_core::format::NumberStyle::Space,
                _ => calc_core::format::NumberStyle::Comma,
            }),
            "caffeine" => {
                let id = self.s.notes.create(calc_core::session::now());
                self.s.notes.set_text(id, CAFFEINE.into(), calc_core::session::now());
                self.s.save_notes();
                self.set_mode(Mode::Notes);
                self.open_note(id);
            }
            "edit" => self.ov.hist_edit = true,
            "resize" => {
                if let Some((w, h)) = parts.get(1).and_then(|v| v.split_once('x')) {
                    if let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>()) {
                        self.ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(w, h)));
                    }
                }
            }
            "wait" => self.hooks.wait_until = self.now + parts.get(1).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.5),
            "clearhistory" => self.s.history_clear(),
            _ => {}
        }
    }

    fn poll_rates(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.rates_rx {
            if let Ok(body) = rx.try_recv() {
                self.s.set_rates_json(&body);
                self.dirty = true;
                self.rates_rx = None;
            }
        }
        if self.rates_tried || !self.s.settings.convert || !self.s.rates_stale() {
            return;
        }
        self.rates_tried = true;
        let (tx, rx) = std::sync::mpsc::channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let out = std::process::Command::new("curl").args(["-s", "--max-time", "15", RATES_URL]).output();
            if let Ok(o) = out {
                if o.status.success() && !o.stdout.is_empty() {
                    let _ = tx.send(String::from_utf8_lossy(&o.stdout).into_owned());
                    c.request_repaint();
                }
            }
        });
        self.rates_rx = Some(rx);
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let notes = self.s.settings.mode == Mode::Notes;
        let events = ctx.input(|i| i.events.clone());
        for e in events {
            match e {
                Event::Key { key: Key::Escape, pressed: true, modifiers, .. } => {
                    if self.ov.close_top() {
                        continue;
                    }
                    if notes {
                        if self.ov.note.is_some() {
                            self.close_note();
                        }
                    } else if modifiers.shift {
                        self.press_key("allclear");
                    } else {
                        self.press_key("ac");
                    }
                }
                _ if typing || notes || self.ov.blocking() => {}
                Event::Key { key: Key::Enter, pressed: true, .. } => self.press_key("eq"),
                Event::Key { key: Key::Backspace | Key::Delete, pressed: true, .. } => self.press_key("back"),
                Event::Text(t) => {
                    for ch in t.chars() {
                        if let Some(k) = key_for_char(ch) {
                            self.press_key(k);
                        }
                    }
                }
                Event::Copy => ctx.copy_text(self.s.copy_text()),
                Event::Paste(t) => {
                    self.s.paste(&t);
                }
                _ => {}
            }
        }
    }

    fn relayout(&mut self, size: egui::Vec2) {
        let resized = (size - self.size).length() > 0.5;
        // the session only changes in answer to input (or a hook, or the rates download), so
        // a frame after none of those reuses the last layout
        if !resized && !self.dirty && !self.hooks.active() {
            self.main_font = self.font_tween.at(self.now);
            return;
        }
        self.s.set_size(size.x, size.y, Insets::default());
        let lay = self.s.layout();
        let sig = (lay.scientific, lay.landscape, lay.tablet, lay.sidebar.is_some(), lay.conv_top.is_some(), self.s.theme().id, self.s.settings.mode);
        let animate = !resized || sig != self.sig;
        self.morph.update(&lay, self.now, animate);
        for tb in &lay.toolbar {
            self.tool_glide.update(tb.id, draw::rect(tb.rect), self.now, animate);
        }
        self.main_font = self.font_tween.go(lay.main_font, self.now, animate);
        self.disp = self.s.display();
        self.conv = self.s.convert_state();
        self.lay = lay;
        self.sig = sig;
        self.size = size;
    }

    fn visuals(&mut self, ctx: &egui::Context) {
        let t = self.s.theme();
        if self.visuals_for == t.id {
            return;
        }
        self.visuals_for = t.id;
        let mut v = if t.light { egui::Visuals::light() } else { egui::Visuals::dark() };
        let accent = col(t.accent);
        v.text_cursor.stroke.color = accent;
        v.selection.bg_fill = fade(accent, 0.35);
        v.selection.stroke.color = col(t.panel_text);
        v.panel_fill = col(t.bg);
        v.override_text_color = Some(col(t.panel_text));
        ctx.set_visuals(v);
        ctx.global_style_mut(|s| {
            s.spacing.scroll = egui::style::ScrollStyle::floating();
            s.spacing.scroll.floating_width = 3.0;
        });
    }

    fn frame(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.now = ctx.input(|i| i.time);
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        let screen = ctx.content_rect();
        self.visuals(&ctx);
        self.hooks_step(&ctx);
        self.poll_rates(&ctx);
        self.keyboard(&ctx);
        let tp = std::time::Instant::now();
        let input = ctx.input(|i| !i.events.is_empty() || i.pointer.any_down() || i.pointer.any_released());
        self.dirty |= input;
        self.relayout(screen.size());
        // what this frame's input changes is laid out on the next frame
        self.dirty = input;
        if input {
            ctx.request_repaint();
        }
        self.prof.phase[0] += tp.elapsed().as_secs_f64() * 1000.0;
        self.press.step(dt);
        let t = self.theme();

        let a = |id: &str, on: bool, d: f32| ctx.animate_value_with_time(Id::unique(id), if on { 1.0 } else { 0.0 }, d);
        let hist_t = anim::ease_in_out(a("hist_t", self.ov.history, 0.35));
        self.hist_t = hist_t;
        let sheet_t = anim::ease_in_out(a("sheet_t", self.ov.sheet_open, 0.35));
        let menu_t = anim::ease_out(a("menu_t", self.ov.menu, 0.2));
        if !self.ov.history && hist_t <= 0.0 {
            self.ov.hist_edit = false;
            self.ov.hist_sel.clear();
            self.ov.hist_swiped = None;
        }
        if !self.ov.sheet_open && sheet_t <= 0.0 {
            self.ov.sheet = Sheet::None;
        }

        let glass = t.family == Family::Modern26;
        let bg = if sheet_t > 0.0 && glass { Color32::BLACK } else { col(t.bg2) };
        egui::CentralPanel::no_frame().show(ui, |ui| {
            ui.painter().rect_filled(screen, 0.0, bg);
        });

        // the calculator slides right under the history panel and shrinks behind a sheet
        let panel_w = (screen.width() * 0.85).min(420.0);
        let push = if self.lay.sidebar.is_none() || self.s.settings.mode == Mode::Notes { panel_w * hist_t } else { 0.0 };
        let scale = if glass && self.ov.sheet != Sheet::None { 1.0 - 0.06 * sheet_t } else { 1.0 };
        let c = screen.center().to_vec2();
        let calc_layer = LayerId::new(Order::Middle, Id::unique("calc"));
        ctx.set_transform_layer(calc_layer, TSTransform::new(c * (1.0 - scale) + vec2(push, 0.0), scale));
        let live = !self.ov.blocking();
        Area::new(Id::unique("calc")).order(Order::Middle).fixed_pos(screen.min).movable(false).constrain(false).show(&ctx, |ui| {
            ui.expand_to_include_rect(screen);
            let p = ui.painter().clone();
            let corner = if scale < 1.0 { 38.0 * (1.0 - scale) / 0.06 } else { 0.0 };
            draw::rrect(&p, screen, corner, col(t.bg), col(t.bg2));
            if self.s.settings.mode == Mode::Notes {
                notes::ui(self, ui, live);
            } else {
                if let Some(sb) = self.lay.sidebar {
                    let tp = std::time::Instant::now();
                    history::sidebar(self, ui, draw::rect(sb), live);
                    self.prof.phase[1] += tp.elapsed().as_secs_f64() * 1000.0;
                }
                let tp = std::time::Instant::now();
                self.calc_ui(ui, live);
                self.prof.phase[2] += tp.elapsed().as_secs_f64() * 1000.0;
            }
        });

        if hist_t > 0.0 || sheet_t > 0.0 || menu_t > 0.0 {
            Area::new(Id::unique("overlay")).order(Order::Foreground).fixed_pos(screen.min).movable(false).constrain(false).show(&ctx, |ui| {
                ui.expand_to_include_rect(screen);
                if hist_t > 0.0 {
                    history::panel(self, ui, screen, hist_t, panel_w, push);
                }
                if sheet_t > 0.0 {
                    match self.ov.sheet {
                        Sheet::Units => convert::sheet(self, ui, screen, sheet_t),
                        Sheet::Settings => settings::sheet(self, ui, screen, sheet_t),
                        Sheet::None => {}
                    }
                }
                if menu_t > 0.0 {
                    menu::menu(self, ui, screen, menu_t);
                }
            });
        }
        if self.ov.popup.is_some() {
            Area::new(Id::unique("popup")).order(Order::Tooltip).fixed_pos(screen.min).movable(false).constrain(false).show(&ctx, |ui| {
                ui.expand_to_include_rect(screen);
                menu::popup(self, ui, screen);
            });
        }

        let now = self.now;
        if self.eq.as_ref().is_some_and(|e| e.done(now)) {
            self.eq = None;
        }
        if self.morph.busy(now) || self.tool_glide.busy(now) || self.press.busy(now) || self.eq.is_some() || self.font_tween.busy(now) || now - self.swap_t0 < 0.3 {
            ctx.request_repaint();
        }
    }

    // ---- the keypad ----

    fn calc_ui(&mut self, ui: &mut Ui, live: bool) {
        let t = self.theme();
        let p = ui.painter().clone();
        let now = self.now;
        if let Some(b) = self.lay.body {
            draw::body(&p, &t, draw::rect(b));
        }
        if self.lay.conv_top.is_some() {
            convert::rows(self, ui, live);
        } else {
            self.display(ui, live);
        }
        if let Some(b) = self.lay.body {
            draw::body_chrome(&p, &t, draw::rect(b), self.lay.title_h);
        }

        // input first, so the frame draws the state it produced
        let keys = self.lay.keys.clone();
        let mut held: Option<String> = None;
        let mut fire = Vec::new();
        if live {
            for k in &keys {
                let Some(sh) = self.morph.shown(&k.id, now) else { continue };
                let r = ui.interact(sh.rect, Id::unique(("key", k.id.as_str())), Sense::click());
                if r.is_pointer_button_down_on() {
                    held = Some(k.id.clone());
                }
                if r.clicked() {
                    fire.push(k.id.clone());
                }
            }
        }
        for id in fire {
            if !(id == "ac" && self.press.long_fired) {
                self.act(&id);
            }
        }
        self.press.set_down(held.as_deref(), now);
        if let Some((id, t0)) = self.press.down.clone() {
            if id == "ac" && !self.press.long_fired {
                if now - t0 >= 0.5 {
                    self.press.long_fired = true;
                    self.press_key("allclear");
                    self.tick();
                } else {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
                }
            }
        }

        for l in &self.morph.leaving {
            let look = Look { rect: l.rect, font: l.font, alpha: l.alpha(now), press: 0.0, glow: 0.0, invert: false };
            draw::key(&p, &t, &l.key, &look);
        }
        let keys = self.lay.keys.clone();
        for k in &keys {
            let Some(sh) = self.morph.shown(&k.id, now) else { continue };
            let down = self.press.is_down(&k.id);
            let mut look = Look { rect: sh.rect, font: sh.font, alpha: sh.alpha, press: 0.0, glow: 0.0, invert: false };
            match t.press {
                Press::Glass => {
                    let sc = self.press.scale(&k.id);
                    look.rect = Rect::from_center_size(sh.rect.center(), sh.rect.size() * sc);
                    look.font *= sc;
                    look.glow = ((sc - 1.0) / 0.06).clamp(0.0, 1.0);
                    look.press = self.press.mix(&k.id, now);
                }
                Press::Fade => look.press = self.press.mix(&k.id, now),
                Press::Invert => look.invert = down,
            }
            draw::key(&p, &t, k, &look);
        }

        for tb in self.lay.toolbar.clone() {
            // the engine's size; only the position glides
            let r = self.tool_glide.at(tb.id, draw::rect(tb.rect), now);
            let mut down = false;
            if live {
                let resp = ui.interact(r, Id::unique(("tb", tb.id)), Sense::click());
                down = resp.is_pointer_button_down_on();
                if resp.clicked() {
                    self.act(tb.id);
                }
            }
            // on phones the history button becomes the history panel's own: it fades out as the
            // panel opens (the panel draws the one in its header)
            if tb.id == "history" && self.hist_t > 0.0 {
                let mut p2 = p.clone();
                p2.multiply_opacity(1.0 - self.hist_t);
                toolbar_button(&p2, &t, r, tb.icon, down);
                continue;
            }
            toolbar_button(&p, &t, r, tb.icon, down);
        }
    }

    /// The number lines: right-aligned, shrunk to fit, with the "=" transition.
    fn display(&mut self, ui: &mut Ui, live: bool) {
        let t = self.theme();
        let p = ui.painter().clone();
        let d = draw::rect(self.lay.display);
        let inset = self.lay.text_inset;
        let ds = &t.display;
        if ds.bg & 0xFF != 0 {
            let rad = ds.radius * d.height();
            draw::rrect(&p, d, rad, col(ds.bg), col(ds.bg2));
            if ds.boxed {
                // a faint inner shade like an LCD
                draw::rrect(&p, Rect::from_min_size(d.min, vec2(d.width(), d.height() * 0.18)).shrink(1.0), rad, Color32::from_black_alpha(26), Color32::TRANSPARENT);
            }
            draw::rrect_stroke(&p, d, rad, if t.family == Family::Mac1984 { 2.0 } else { 1.0 }, col(ds.border));
        }
        let face = t.font;
        let (rw, lw) = (t.result_weight, t.label_weight);
        let main_f = move |sz: f32| crate::fonts::font(face, rw, sz);
        let expr_f = move |sz: f32| crate::fonts::font(face, lw.min(400), sz);
        let avail = d.width() - 2.0 * inset;
        let right = d.right() - inset;
        let main_size0 = self.main_font;
        let line = draw::segs(&p, &self.disp.main, main_size0, &main_f);
        let main_size = draw::fit(line.width, main_size0, avail);
        let line = if main_size < main_size0 { draw::segs(&p, &self.disp.main, main_size, &main_f) } else { line };
        let base = if ds.boxed { d.center().y + main_size * 0.36 } else if ds.bg & 0xFF != 0 { d.bottom() - main_size * 0.2 } else { d.bottom() };
        let expr_size0 = self.lay.expr_font;
        let expr_base = if ds.boxed { d.top() + expr_size0 * 1.15 } else { base - main_size * 0.86 };
        let text_c = col(ds.text);
        let expr_c = col(ds.expr);
        let now = self.now;
        let eq = self.eq.as_ref().map(|e| (e.p(now), e.from.clone(), e.from_size));
        match eq {
            Some((q, from, from_size)) => {
                let sz = anim::lerp(from_size.min(main_size0), expr_size0, q);
                let l = draw::segs(&p, &from, sz, &expr_f);
                let sz2 = draw::fit(l.width, sz, avail);
                let l = if sz2 < sz { draw::segs(&p, &from, sz2, &expr_f) } else { l };
                l.paint_right(&p, right, anim::lerp(base, expr_base, q), draw::mix(text_c, expr_c, q));
                line.paint_right(&p, right, base + (1.0 - q) * main_size * 0.25, fade(text_c, q));
            }
            None => {
                if !self.disp.expr.is_empty() {
                    let l = draw::segs(&p, &self.disp.expr, expr_size0, &expr_f);
                    let sz = draw::fit(l.width, expr_size0, avail);
                    let l = if sz < expr_size0 { draw::segs(&p, &self.disp.expr, sz, &expr_f) } else { l };
                    l.paint_right(&p, right, expr_base, expr_c);
                }
                line.paint_right(&p, right, base, text_c);
            }
        }
        if self.disp.rad {
            let rad = draw::segs(&p, &[calc_core::calc::Seg { t: "Rad".into(), sup: false }], expr_size0 * 0.75, &expr_f);
            rad.paint(&p, d.left() + inset, base, expr_c);
        }
        if live {
            let hit = Rect::from_min_max(pos2(d.left(), (base - main_size * 1.1).max(d.top())), d.max);
            self.display_input(ui, hit);
        }
    }

    /// Swipe = delete a digit; right-click or long-press = Copy/Paste.
    pub fn display_input(&mut self, ui: &mut Ui, hit: Rect) {
        let r = ui.interact(hit, Id::unique("display"), Sense::click_and_drag());
        if r.drag_started() {
            self.disp_swiped = false;
        }
        if r.dragged() && !self.disp_swiped {
            if let Some(dd) = r.total_drag_delta() {
                if dd.x.abs() > 30.0 && dd.x.abs() > dd.y.abs() {
                    self.disp_swiped = true;
                    self.s.swipe_delete();
                    self.tick();
                }
            }
        }
        let pos = r.interact_pointer_pos().or(r.hover_pos()).unwrap_or(hit.center());
        if r.secondary_clicked() {
            self.edit_popup(pos);
        }
        if r.is_pointer_button_down_on() && !r.dragged() {
            let t0 = *self.disp_down.get_or_insert(self.now);
            if self.now - t0 >= 0.5 {
                self.disp_down = Some(f64::INFINITY);
                self.edit_popup(pos);
            } else if t0.is_finite() {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
            }
        } else {
            self.disp_down = None;
        }
    }

    fn edit_popup(&mut self, pos: Pos2) {
        self.ov.popup = Some(Popup { pos, items: vec![("Copy", PopAct::Copy), ("Paste", PopAct::Paste)] });
    }
}

/// The fill of iOS 26's dark glass buttons (toolbar circles, capsules), brighter while held.
pub fn glass_fill(down: bool) -> (Color32, Color32) {
    if down {
        (col(0x56565AFF), col(0x4A4A4EFF))
    } else {
        (col(0x343436FF), col(0x2A2A2CFF))
    }
}

/// A toolbar button: a dark glass circle (iOS 26) or a plain accent icon.
pub fn toolbar_button(p: &egui::Painter, t: &Theme, r: Rect, icon: &str, down: bool) {
    if t.glass_toolbar {
        let c = r.center();
        let rad = r.width().min(r.height()) / 2.0;
        let (top, bot) = glass_fill(down);
        draw::glass_shape(p, Rect::from_center_size(c, vec2(rad, rad) * 2.0), rad, top, bot);
        draw::icon(p, icon, Rect::from_center_size(c, vec2(rad, rad)), Color32::WHITE);
    } else {
        let s = r.width().min(r.height()) * 0.58;
        draw::icon(p, icon, Rect::from_center_size(r.center(), vec2(s, s)), fade(col(t.accent), if down { 0.45 } else { 1.0 }));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let t0 = self.prof.on.then(std::time::Instant::now);
        self.frame(ui);
        if let Some(t0) = t0 {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let whole = frame.info().cpu_usage.unwrap_or(0.0) * 1000.0;
            if ms > 8.0 || whole > 16.0 {
                let (sz, fill) = ui.ctx().fonts(|f| (f.font_image_size(), f.font_atlas_fill_ratio()));
                eprintln!("prof: slow frame t={:.2} update {ms:.1} ms, prev whole {whole:.1} ms, atlas {sz:?} {:.0}%", self.now, fill * 100.0);
            }
            self.prof.add(t0.elapsed().as_secs_f64() * 1000.0, frame.info().cpu_usage.unwrap_or(0.0) as f64 * 1000.0);
        }
    }

    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        col(self.s.theme().bg).to_normalized_gamma_f32()
    }
}
