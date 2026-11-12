//! The track header as Logic's: which components show (Track ▸ Configure Track Header…, ⌥T, the
//! popover with Buttons / Controls / Additional Name Column / Other and its More menu: Store
//! and Apply User Configuration, Revert to Defaults), the On/Off, Protect and Freeze buttons
//! beside Mute, Solo, Record Enable and Input Monitoring (slide activation over them, Command-
//! and Option-click), the colour bar and the Color palette (⌥C), track icons and the Icon
//! palette with custom icons, the groove track's star and Match Groove Track checkboxes, the
//! track number (focus), the corner drag for track zoom, Search and Select Track, the
//! Clear/Recall Solo and Hide buttons over the headers.

use std::collections::{HashMap, HashSet};

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Rect, Sense, Stroke};
use lg_core::trackmix::{self, FreezeMode};
use lg_core::TrackKind;

use crate::app::App;
use crate::glyphs::{self, ph, ph_fill};
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Configure Track Header…",
    "Configure Track Header",
    "Store User Configuration",
    "Apply User Configuration",
    "Revert to Defaults",
    "Assign Track Color…",
    "Show Colors",
    "Assign Track Icon…",
    "Search and Select Track…",
    "Toggle Track On",
    "Toggle Track Protect",
    "Toggle Track Freeze",
    "Toggle Track Mute of all Tracks of Folder",
    "Clear/Recall Solo",
    "Toggle Activation of All Tracks",
    "Set Groove Track",
    "Toggle Match Groove Track",
    "Individual Track Zoom Reset for All Tracks",
];

/// The header components shown (Logic's Track Header Components).
#[derive(Clone, Debug, PartialEq)]
pub struct Cfg {
    pub on_off: bool,
    pub mute: bool,
    pub solo: bool,
    pub protect: bool,
    pub freeze: bool,
    pub record: bool,
    pub input: bool,
    pub volume: bool,
    pub pan: bool,
    pub name_col: bool,
    /// Additional Name Column Mode: 0 Automatic, 1 Patch or Channel Strip Setting Name,
    /// 2 Software Instrument Setting Name, 3 Channel Strip Name, 4 Channel Strip Type and Number.
    pub name_mode: u8,
    pub surface_bars: bool,
    pub numbers: bool,
    pub color_bars: bool,
    pub groove: bool,
    pub icons: bool,
}

pub const NAME_MODES: [&str; 5] = ["Automatic", "Patch or Channel Strip Setting Name", "Software Instrument Setting Name", "Channel Strip Name", "Channel Strip Type and Number"];

impl Default for Cfg {
    fn default() -> Self {
        Cfg { on_off: false, mute: true, solo: true, protect: false, freeze: false, record: true, input: true, volume: true, pan: true, name_col: false, name_mode: 0, surface_bars: true, numbers: true, color_bars: false, groove: false, icons: true }
    }
}

impl Cfg {
    fn flags(&mut self) -> [(&'static str, &mut bool); 15] {
        [("on_off", &mut self.on_off), ("mute", &mut self.mute), ("solo", &mut self.solo), ("protect", &mut self.protect), ("freeze", &mut self.freeze), ("record", &mut self.record), ("input", &mut self.input), ("volume", &mut self.volume), ("pan", &mut self.pan), ("name_col", &mut self.name_col), ("surface_bars", &mut self.surface_bars), ("numbers", &mut self.numbers), ("color_bars", &mut self.color_bars), ("groove", &mut self.groove), ("icons", &mut self.icons)]
    }

    pub fn to_text(&self) -> String {
        let mut c = self.clone();
        let mode = c.name_mode;
        let on: Vec<&str> = c.flags().into_iter().filter(|(_, v)| **v).map(|(k, _)| k).collect();
        format!("{} name_mode={mode}", on.join(" "))
    }

    pub fn from_text(t: &str) -> Cfg {
        let mut c = Cfg::default();
        let words: Vec<&str> = t.split_whitespace().collect();
        for (k, v) in c.flags() {
            *v = words.contains(&k);
        }
        c.name_mode = words.iter().find_map(|w| w.strip_prefix("name_mode=")).and_then(|x| x.parse().ok()).unwrap_or(0);
        c
    }

    fn user_path() -> Option<std::path::PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
        Some(base.join("bramble-studio").join("track-header.txt"))
    }

    /// The stored user configuration (new projects start with it), else the defaults.
    pub fn user_default() -> Cfg {
        Cfg::user_path().and_then(|p| std::fs::read_to_string(p).ok()).map_or_else(Cfg::default, |t| Cfg::from_text(&t))
    }

    pub fn store_user(&self) {
        if let Some(p) = Cfg::user_path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, self.to_text());
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    OnOff,
    Mute,
    Solo,
    Protect,
    Freeze,
    Hide,
}

/// What a colour is being chosen for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorFor {
    Tracks,
    Strip(usize),
}

#[derive(Default)]
pub struct Ui {
    /// The Track Header Configuration popover (where it opened).
    pub popover: Option<egui::Pos2>,
    popover_frames: u32,
    pub colors: Option<ColorFor>,
    pub icons: Option<usize>,
    /// Slide activation: the button kind and the state being spread.
    pub swipe: Option<(Btn, bool)>,
    /// Search and Select Track: the text typed, the row highlighted.
    pub search: Option<String>,
    search_row: usize,
    pub alert: Option<String>,
    /// Instrument tracks whose input monitoring was turned off (Option-click the I).
    pub monitor_off: HashSet<u64>,
    /// Clear/Recall Solo: the tracks that were soloed (ids), and whether the button shows.
    pub solo_recall: Vec<u64>,
    pub solo_button: bool,
    /// The Toggle Hide View button shows (after the first hide).
    pub hide_button: bool,
    custom: HashMap<String, egui::TextureHandle>,
    /// A custom icon's menu: Replace picks a file for this one.
    zoom_from: Option<(f32, f32)>,
}

/// The built-in icons of the Icon palette: (name, glyph), by instrument family.
pub const ICONS: &[(&str, &str)] = &[
    ("Keys", ph::PIANO_KEYS),
    ("Drum Kit", glyphs::br::DRUM),
    ("Guitar", ph::GUITAR),
    ("Microphone", ph::MICROPHONE),
    ("Stage Microphone", ph::MICROPHONE_STAGE),
    ("Waveform", ph::WAVEFORM),
    ("Sine", ph::WAVE_SINE),
    ("Square", ph::WAVE_SQUARE),
    ("Sawtooth", ph::WAVE_SAWTOOTH),
    ("Triangle", ph::WAVE_TRIANGLE),
    ("Speaker", ph::SPEAKER_HIGH),
    ("Hi-Fi", ph::SPEAKER_HIFI),
    ("Headphones", ph::HEADPHONES),
    ("Record", ph::VINYL_RECORD),
    ("Note", ph::MUSIC_NOTE),
    ("Notes", ph::MUSIC_NOTES),
    ("Bell", ph::BELL),
    ("Metronome", ph::METRONOME),
    ("Radio", ph::RADIO),
    ("Tape", ph::CASSETTE_TAPE),
    ("Disc", ph::DISC),
    ("Sliders", ph::SLIDERS),
    ("Faders", ph::FADERS),
    ("Equalizer", ph::EQUALIZER),
    ("Playlist", ph::PLAYLIST),
    ("Singer", ph::USER_SOUND),
    ("Choir", ph::USERS),
    ("Megaphone", ph::MEGAPHONE),
    ("Claps", ph::HANDS_CLAPPING),
    ("Lightning", ph::LIGHTNING),
    ("Fire", ph::FIRE),
    ("Drop", ph::DROP),
    ("Sun", ph::SUN),
    ("Moon", ph::MOON),
    ("Cloud", ph::CLOUD),
    ("Waves", ph::WAVES),
    ("Tree", ph::TREE),
    ("Flower", ph::FLOWER),
    ("Heart", ph::HEART),
    ("Robot", ph::ROBOT),
    ("Rocket", ph::ROCKET),
    ("Planet", ph::PLANET),
    ("Film", ph::FILM_STRIP),
    ("Camera", ph::VIDEO_CAMERA),
    ("Game", ph::GAME_CONTROLLER),
    ("Circuit", ph::CIRCUITRY),
    ("Plugs", ph::PLUGS),
    ("MIDI", glyphs::br::MIDI),
    ("Tuning Fork", glyphs::br::TUNING_FORK),
    ("Bird", ph::BIRD),
    ("Crown", ph::CROWN),
    ("Diamond", ph::DIAMOND),
    ("Sparkle", ph::SPARKLE),
    ("Wand", ph::MAGIC_WAND),
    ("Folder", ph::FOLDER),
    ("Star", ph::STAR),
];

/// Where custom track icons are kept (copied there; not stored with the project).
pub fn custom_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))?;
    Some(base.join("bramble-studio").join("Track Icons"))
}

pub fn custom_icons() -> Vec<String> {
    let Some(d) = custom_dir() else { return Vec::new() };
    let mut v: Vec<String> = std::fs::read_dir(d).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.to_lowercase().ends_with(".png")).collect();
    v.sort();
    v
}

/// Copies a picture into the custom icons' folder (as PNG, at most 1024 × 1024); its name.
pub fn add_custom_icon(from: &std::path::Path, replace: Option<&str>) -> Option<String> {
    let dir = custom_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let img = image::open(from).ok()?.to_rgba8();
    let img = if img.width() > 1024 || img.height() > 1024 { image::imageops::resize(&img, 1024, 1024, image::imageops::FilterType::Triangle) } else { img };
    let name = match replace {
        Some(n) => n.to_string(),
        None => {
            let stem = from.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Icon".into());
            let mut name = format!("{stem}.png");
            let mut k = 2;
            while dir.join(&name).exists() {
                name = format!("{stem} {k}.png");
                k += 1;
            }
            name
        }
    };
    img.save_with_format(dir.join(&name), image::ImageFormat::Png).ok()?;
    Some(name)
}

fn custom_texture(app: &mut App, ctx: &egui::Context, name: &str) -> Option<egui::TextureHandle> {
    if let Some(t) = app.tm.ui.custom.get(name) {
        return Some(t.clone());
    }
    let img = image::open(custom_dir()?.join(name)).ok()?.to_rgba8();
    let img = image::imageops::resize(&img, 128, 128, image::imageops::FilterType::Triangle);
    let ci = egui::ColorImage::from_rgba_unmultiplied([128, 128], img.as_raw());
    let tex = ctx.load_texture(format!("track-icon-{name}"), ci, egui::TextureOptions::LINEAR);
    app.tm.ui.custom.insert(name.to_string(), tex.clone());
    Some(tex)
}

/// The track's colour: the one assigned from the palette, else its type's (audio blue,
/// instrument green, Session Player yellow).
pub fn color_of(t: &lg_core::Track) -> Color32 {
    match t.extra.set.strip.color {
        Some(c) => {
            let [r, g, b] = trackmix::palette(c);
            Color32::from_rgb(r, g, b)
        }
        None => look::region_color(t.kind),
    }
}

pub fn strip_color(app: &App, s: usize) -> Option<Color32> {
    crate::tm::strip(app, s).color.map(|c| {
        let [r, g, b] = trackmix::palette(c);
        Color32::from_rgb(r, g, b)
    })
}

/// The glyph of the track's chosen built-in icon (None: its patch's own, or a custom picture).
pub fn chosen_glyph(t: &lg_core::Track) -> Option<&'static str> {
    let name = t.extra.set.icon.as_deref()?;
    ICONS.iter().find(|(n, _)| *n == name).map(|(_, g)| *g)
}

/// The header's left edge: the colour bar, the groove track's star / Match checkbox, and the
/// corner where dragging zooms the track. Returns how far the rest moves right.
pub fn left_edge(app: &mut App, ui: &mut egui::Ui, hp: &egui::Painter, row: Rect, i: usize) -> f32 {
    let Some(t) = app.project.as_ref().and_then(|p| p.tracks.get(i)).cloned() else { return 0.0 };
    let mut x = 0.0;
    // control surface bars: the surface's colour along the left of the tracks it controls
    if app.tm.hdr.surface_bars {
        if let Some(c) = crate::surfaces::controlled_tracks(app).get(&i) {
            hp.rect_filled(Rect::from_min_max(pos2(row.min.x, row.min.y + 1.0), pos2(row.min.x + 2.5, row.max.y - 1.0)), 0.0, Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, *c as u8));
        }
    }
    if app.tm.hdr.color_bars {
        let bar = Rect::from_min_max(pos2(row.min.x + 1.0, row.min.y + 1.0), pos2(row.min.x + 6.0, row.max.y - 1.0));
        hp.rect_filled(bar, 2.0, color_of(&t));
        let resp = ui.interact(bar.expand2(vec2(1.0, 0.0)), ui.scope_id().with(("colorbar", i)), Sense::click());
        if resp.clicked() || resp.secondary_clicked() {
            if !app.ta.selected.contains(&i) {
                crate::trackarea::click_track(app, i, false, false);
            }
            app.tm.ui.colors = Some(ColorFor::Tracks);
        }
        x += 6.0;
    }
    if app.tm.hdr.groove {
        let any = app.project.as_ref().is_some_and(|p| p.tracks.iter().any(|t| t.extra.set.groove));
        let r = Rect::from_min_size(pos2(row.min.x + x + 2.0, row.min.y + 4.0), vec2(14.0, 14.0));
        let resp = ui.interact(r, ui.scope_id().with(("groove", i)), Sense::click());
        let set = &t.extra.set;
        if set.groove {
            glyphs::paint(hp, r, ph_fill::STAR, 13.0, Color32::from_rgb(0xf0, 0xc0, 0x30));
            if resp.on_hover_text("Groove Track (click to turn off)").clicked() {
                if let Some(s) = crate::tm::set_mut(app, i) {
                    s.groove = false;
                }
                app.load_engine();
            }
        } else if any {
            // the others: a Match Groove Track checkbox
            hp.rect_stroke(r.shrink(1.5), 2.0, Stroke::new(1.0, look::text_dim()), egui::StrokeKind::Inside);
            if set.match_groove {
                glyphs::paint(hp, r, ph::CHECK, 11.0, look::text());
            }
            if resp.on_hover_text("Match Groove Track").clicked() {
                if let Some(s) = crate::tm::set_mut(app, i) {
                    s.match_groove = !s.match_groove;
                }
                app.load_engine();
            }
        } else if resp.hovered() {
            // (hovering the edge shows the star; a click makes this the groove track)
            glyphs::paint(hp, r, ph::STAR, 13.0, look::text_dim());
            if resp.on_hover_text("Set as Groove Track").clicked() {
                set_groove(app, i);
            }
        }
        x += 14.0;
    }
    // the bottom-left corner: drag up or down to zoom this track (Command: all, Shift: reset)
    let corner = Rect::from_min_max(pos2(row.min.x, row.max.y - 7.0), pos2(row.min.x + 22.0, row.max.y));
    let resp = ui.interact(corner, ui.scope_id().with(("zoomcorner", i)), Sense::click_and_drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    if resp.drag_started() {
        app.tm.ui.zoom_from = Some((t.zoom(), if app.ta.v_zoom > 0.0 { app.ta.v_zoom } else { 1.0 }));
    }
    if resp.dragged() {
        let (shift, cmd) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        let total = resp.interact_pointer_pos().map_or(0.0, |p| p.y) - ui.input(|i| i.pointer.press_origin()).map_or(0.0, |p| p.y);
        if let Some((z0, a0)) = app.tm.ui.zoom_from {
            if shift {
                reset_all_zoom(app);
                app.help_tag = Some(vec![("Zoom".into(), "Default".into())]);
            } else if cmd {
                app.ta.v_zoom = (a0 + total / crate::tracks::ROW_H).clamp(0.5, 4.0);
                app.help_tag = Some(vec![("Zoom All Tracks".into(), format!("{:.2}", app.ta.v_zoom))]);
            } else {
                let z = (z0 + total / crate::tracks::ROW_H).clamp(0.5, 6.0);
                // (a group linking Track Zoom zooms its tracks together)
                let with: Vec<usize> = crate::mixstate::linked(app, i, (2, 0x02)).into_iter().chain([i]).collect();
                for k in with {
                    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(k)) {
                        tr.extra.height = z;
                    }
                }
                app.help_tag = Some(vec![("Zoom".into(), format!("{z:.2} × window zoom"))]);
            }
        }
    }
    if resp.drag_stopped() {
        app.tm.ui.zoom_from = None;
    }
    x
}

pub fn reset_all_zoom(app: &mut App) {
    if let Some(p) = app.project.as_mut() {
        for t in &mut p.tracks {
            t.extra.height = 0.0;
        }
    }
}

/// Makes track `i` the project's groove track (one only).
pub fn set_groove(app: &mut App, i: usize) {
    if let Some(p) = app.project.as_mut() {
        for (k, t) in p.tracks.iter_mut().enumerate() {
            t.extra.set.groove = k == i;
            if k == i {
                t.extra.set.match_groove = false;
            }
        }
    }
    app.tm.hdr.groove = true;
    app.dirty = true;
    app.load_engine();
}

/// The track number: the focused track's in a lighter grey than the other selected ones;
/// a click on it inside a multiple selection moves the focus and keeps the selection.
pub fn number(app: &mut App, ui: &mut egui::Ui, hp: &egui::Painter, row: Rect, i: usize, x: f32, dim: bool) {
    if !app.tm.hdr.numbers {
        return;
    }
    let focused = app.selected_track == Some(i);
    let selected = app.ta.selected.contains(&i);
    // (Logic: white numbers; inside a multiple selection the others a shade dimmer)
    let col = if dim { look::text_dim().gamma_multiply(0.5) } else if selected && !focused && app.ta.selected.len() > 1 { look::mix(look::text(), look::text_dim(), 0.5) } else { look::text() };
    let c = pos2(row.min.x + 10.0 + x, row.center().y - 3.0);
    hp.text(c, Align2::CENTER_CENTER, format!("{}", i + 1), look::regular(13.0), col);
    // (Logic's hairline between the number and the rest of the header)
    hp.line_segment([pos2(row.min.x + 20.5 + x, row.min.y), pos2(row.min.x + 20.5 + x, row.max.y)], Stroke::new(1.0, look::ta_header_line()));
    if selected && app.ta.selected.len() > 1 {
        let r = Rect::from_center_size(c, vec2(22.0, 18.0));
        if ui.interact(r, ui.scope_id().with(("tracknum", i)), Sense::click()).clicked() {
            app.selected_track = Some(i);
        }
    }
}

/// The track icon (hidden when Track Icons is off): the chosen built-in or custom icon over the
/// patch's own. Control-click (right-click) opens the Icon palette. True when it drew.
pub fn icon(app: &mut App, ui: &mut egui::Ui, hp: &egui::Painter, r: Rect, i: usize, large: bool) -> Option<bool> {
    if !app.tm.hdr.icons {
        return None;
    }
    let t = app.project.as_ref()?.tracks.get(i)?.clone();
    let resp = ui.interact(r, ui.scope_id().with(("trackicon", i)), Sense::click());
    if resp.secondary_clicked() {
        app.tm.ui.icons = Some(i);
    }
    if resp.double_clicked() {
        // a track with a patch: Smart Controls; without one: the Library (Browser)
        let ctx = ui.ctx().clone();
        app.selected_track = Some(i);
        if app.instrument_choice.contains_key(&i) {
            app.key_command(&ctx, "Show/Hide Smart Controls");
        } else {
            app.show_browser = !app.show_browser;
        }
    }
    let c = color_of(&t);
    // (Logic draws the icon a brighter shade of the track colour)
    let ink = if kn_ui::theme::is_light() { c } else { look::icon_color(c) };
    if let Some(name) = t.extra.set.icon.as_deref().and_then(|n| n.strip_prefix("custom:")) {
        let ctx = ui.ctx().clone();
        if let Some(tex) = custom_texture(app, &ctx, name) {
            hp.image(tex.id(), r.shrink(2.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            return Some(true);
        }
    }
    let Some(g) = chosen_glyph(&t) else {
        // (the patch's own icon, in the track's colour when one is assigned)
        if t.extra.set.strip.color.is_some() && t.extra.stack.is_none() {
            glyphs::paint(hp, r, glyphs::TrackIcon::of(&t).glyph(), if large { r.width() * 0.8 } else { 27.0 }, ink);
            return Some(true);
        }
        return Some(false);
    };
    glyphs::paint(hp, r, g, if large { r.width() * 0.8 } else { 27.0 }, ink);
    Some(true)
}

/// What the header buttons did this frame.
pub struct Pressed {
    pub mute: bool,
    pub solo: bool,
    pub changed: bool,
    /// Where the controls after the buttons start.
    pub end_x: f32,
}

fn no_input(app: &App, i: usize) -> bool {
    app.project.as_ref().and_then(|p| p.tracks.get(i)).is_some_and(|t| t.kind == TrackKind::Audio) && app.mix.inputs.get(&i) == Some(&crate::mixstate::Input::None)
}

/// Whether anything is soloed, and the tracks in a soloed strip's signal flow.
pub fn solo_state(app: &App) -> (bool, Vec<usize>) {
    let Some(p) = app.project.as_ref() else { return (false, Vec::new()) };
    let any = p.tracks.iter().any(|t| t.solo) || app.auxes.iter().any(|a| a.solo);
    let soloed_buses: Vec<usize> = app.auxes.iter().filter(|a| a.solo).map(|a| a.bus).collect();
    let flow = (0..p.tracks.len())
        .filter(|t| !p.tracks[*t].solo)
        .filter(|t| matches!(app.mix.outputs.get(t), Some(crate::audio::Out::Bus(b)) if soloed_buses.contains(b)) || app.sends.get(t).is_some_and(|v| v.iter().any(|s| soloed_buses.contains(&s.bus))))
        .collect();
    (any, flow)
}

/// The header's buttons in Logic's order: On/Off, Mute, Solo, Protect, Freeze, Record Enable,
/// Input Monitoring (each when its component is on).
pub fn buttons(app: &mut App, ui: &mut egui::Ui, row: Rect, i: usize, bx: f32, by: f32) -> Pressed {
    let Some(t) = app.project.as_ref().and_then(|p| p.tracks.get(i)).cloned() else { return Pressed { mute: false, solo: false, changed: false, end_x: bx } };
    let _ = row;
    let cfg = app.tm.hdr.clone();
    let set = t.extra.set.clone();
    let mut out = Pressed { mute: t.mute, solo: t.solo, changed: false, end_x: bx };
    let (alt, cmd) = ui.input(|i| (i.modifiers.alt, i.modifiers.command));
    let down = ui.input(|i| i.pointer.primary_down());
    let pointer = ui.ctx().pointer_latest_pos();
    let blink = (ui.input(|i| i.time) * 2.5).fract() < 0.5;
    let (any_solo, flow) = solo_state(app);
    let mut x = bx;
    // Logic joins the buttons into pills (M|S, R|I…): each group's buttons touch, a divider
    // between them, the outer corners rounded, 5 pt between groups
    let shown = [
        (cfg.on_off, 0),
        (cfg.mute, 1),
        (cfg.solo, 1),
        (cfg.protect, 2),
        (cfg.freeze, 2),
        (cfg.record && !no_input(app, i), 3),
        (cfg.input && !no_input(app, i), 3),
        (app.ta.show_hidden && app.tm.ui.hide_button, 4),
    ];
    let order: Vec<u8> = shown.iter().filter(|s| s.0).map(|s| s.1).collect();
    let mut k = 0usize;
    let corners = std::cell::Cell::new(egui::CornerRadius::default());
    let mut next = |x: &mut f32| {
        let g = order.get(k).copied().unwrap_or(9);
        let first = k == 0 || order.get(k - 1) != Some(&g);
        let last = order.get(k + 1) != Some(&g);
        if first && k > 0 {
            *x += 6.0;
        }
        // (Logic: 20.5 × 17.5 pt halves, 6 pt between pills)
        let r = Rect::from_min_size(pos2(*x, by + 0.5), vec2(20.5, 17.5));
        *x += 20.5;
        k += 1;
        let (a, b) = (if first { 5 } else { 0 }, if last { 5 } else { 0 });
        corners.set(egui::CornerRadius { nw: a, sw: a, ne: b, se: b });
        r
    };
    // one button: its look, and what the pointer does to it — a click or the start of a slide
    // gives Some(new state); a slide passing over gives the state being spread
    let press = |app: &mut App, ui: &mut egui::Ui, r: Rect, kind: Btn, on: bool, id: &str| -> Option<bool> {
        let resp = ui.interact(r, ui.scope_id().with((id, i)), Sense::click_and_drag());
        if resp.drag_started() {
            app.tm.ui.swipe = Some((kind, !on));
            return Some(!on);
        }
        if resp.clicked() {
            return Some(!on);
        }
        if let (Some((k, state)), true, Some(pp)) = (app.tm.ui.swipe, down, pointer) {
            if k == kind && r.expand2(vec2(1.5, 6.0)).contains(pp) && on != state {
                return Some(state);
            }
        }
        None
    };
    // (the button's fill with its pill corners; a hairline divider where it joins the one before)
    let paint = |ui: &egui::Ui, r: Rect, fill: Color32, cr: egui::CornerRadius| {
        ui.painter().rect_filled(r, cr, fill);
        if cr.nw == 0 {
            ui.painter().line_segment([pos2(r.min.x, r.min.y + 4.5), pos2(r.min.x, r.max.y - 4.5)], Stroke::new(1.0, Color32::from_white_alpha(40)));
        }
    };
    let cr = |c: &std::cell::Cell<egui::CornerRadius>| c.get();
    if cfg.on_off {
        let r = next(&mut x);
        let on = !set.off;
        paint(ui, r, look::header_pill(), cr(&corners));
        glyphs::paint(ui.painter(), r, ph::POWER, 12.0, if on { Color32::from_rgb(0x52, 0xc0, 0x3c) } else { look::text_dim().gamma_multiply(0.6) });
        if let Some(state) = press(app, ui, r, Btn::OnOff, on, "onoff") {
            if cmd {
                // Command-click: every track on or off (Option-Command: plug-ins kept loaded)
                let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
                for k in 0..n {
                    if let Some(s) = crate::tm::set_mut(app, k) {
                        s.off = !state;
                        s.offline = !state && alt;
                    }
                }
            }
            if let Some(s) = crate::tm::set_mut(app, i) {
                s.off = !state;
                // (Option-click: offline, its plug-ins kept loaded)
                s.offline = !state && alt;
            }
            app.load_engine();
        }
    }
    if cfg.mute {
        let r = next(&mut x);
        // (while something is soloed: the tracks it silences flash their M blue)
        let silenced = any_solo && !t.solo && !t.mute && !set.strip.solo_safe && !flow.contains(&i) && set.external.is_none();
        let blue = Color32::from_rgb(0x4f, 0xa8, 0xe8);
        paint(ui, r, if t.mute { blue } else if silenced && blink { blue.gamma_multiply(0.45) } else { look::header_pill() }, cr(&corners));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "M", look::semibold(12.0), if t.mute { Color32::BLACK } else { look::text() });
        if let Some(state) = press(app, ui, r, Btn::Mute, t.mute, "mute") {
            if cmd {
                // Command-click: every track in the same state follows
                if let Some(p) = app.project.as_mut() {
                    for o in p.tracks.iter_mut().filter(|o| o.mute == t.mute) {
                        o.mute = state;
                    }
                }
            } else if alt {
                // Option-click: this track's mute, every other track's mute off
                if let Some(p) = app.project.as_mut() {
                    for (k, o) in p.tracks.iter_mut().enumerate() {
                        if k != i {
                            o.mute = false;
                        }
                    }
                }
            }
            out.mute = state;
            out.changed = true;
        }
    }
    if cfg.solo {
        let r = next(&mut x);
        let yellow = Color32::from_rgb(0xe8, 0xc8, 0x3c);
        let in_flow = flow.contains(&i);
        paint(ui, r, if t.solo { yellow } else if in_flow && blink { yellow.gamma_multiply(0.5) } else { look::header_pill() }, cr(&corners));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "S", look::semibold(12.0), if t.solo { Color32::BLACK } else { look::text() });
        if set.strip.solo_safe {
            ui.painter().line_segment([r.left_bottom() + vec2(3.0, -3.0), r.right_top() + vec2(-3.0, 3.0)], Stroke::new(1.5, look::record_red()));
        }
        let resp = ui.interact(r, ui.scope_id().with(("solo-menu", i)), Sense::hover());
        if resp.hovered() && ui.input(|i| i.pointer.secondary_clicked()) {
            // Control-click: solo-safe
            let was = set.strip.solo_safe;
            crate::tm::edit_strip(app, i, |s| s.solo_safe = !was);
        } else if let Some(state) = press(app, ui, r, Btn::Solo, t.solo, "solo") {
            if cmd {
                // Command-click: every track's solo on or off
                if let Some(p) = app.project.as_mut() {
                    for o in p.tracks.iter_mut() {
                        o.solo = state;
                    }
                }
                out.solo = state;
            } else if alt {
                // Option-click: this track alone; on a soloed one: every solo off
                let clear = t.solo;
                if let Some(p) = app.project.as_mut() {
                    for (k, o) in p.tracks.iter_mut().enumerate() {
                        o.solo = !clear && k == i;
                    }
                }
                out.solo = !clear;
            } else {
                out.solo = state;
            }
            out.changed = true;
            if out.solo {
                app.tm.ui.solo_button = true;
            }
        }
    }
    if cfg.protect {
        let r = next(&mut x);
        paint(ui, r, look::header_pill(), cr(&corners));
        glyphs::paint(ui.painter(), r, if set.protected { ph_fill::LOCK } else { ph::LOCK_OPEN }, 12.0, if set.protected { Color32::from_rgb(0x52, 0xc0, 0x3c) } else { look::text_dim() });
        if let Some(state) = press(app, ui, r, Btn::Protect, set.protected, "protect") {
            if cmd {
                if let Some(p) = app.project.as_mut() {
                    for o in &mut p.tracks {
                        o.extra.set.protected = state;
                    }
                }
                app.dirty = true;
            } else if let Some(s) = crate::tm::set_mut(app, i) {
                s.protected = state;
            }
            if state {
                app.armed.remove(&i);
            }
        }
    }
    if cfg.freeze {
        let r = next(&mut x);
        let col = match set.freeze_mode {
            FreezeMode::SourceOnly => Color32::from_rgb(0x52, 0xc0, 0x3c),
            FreezeMode::PreFader => Color32::from_rgb(0x4f, 0xa8, 0xe8),
        };
        paint(ui, r, if set.frozen { col } else { look::header_pill() }, cr(&corners));
        glyphs::paint(ui.painter(), r, ph::SNOWFLAKE, 12.0, if set.frozen { Color32::WHITE } else { look::text_dim() });
        if let Some(state) = press(app, ui, r, Btn::Freeze, set.frozen, "freeze") {
            crate::freeze::set_frozen(app, i, state);
        }
    }
    let locked = set.protected || set.frozen;
    if cfg.record && !no_input(app, i) && set.external.as_ref().is_none_or(|e| !e.use_plugin || true) {
        let r = next(&mut x);
        let armed = app.armed.contains(&i);
        let red = look::record_red();
        // off: grey; record-ready: blinking red; recording: solid red; the focused track when
        // none is armed: grey with a red R (it records)
        let recording = app.recording() && armed;
        let implied = !armed && app.armed.is_empty() && app.selected_track == Some(i);
        paint(ui, r, if recording || (armed && blink) { red } else if armed { red.gamma_multiply(0.55) } else { look::header_pill() }, cr(&corners));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "R", look::semibold(12.0), if armed { Color32::WHITE } else if implied { Color32::from_rgb(0xfe, 0x6e, 0x6e) } else { look::text() });
        if ui.interact(r, ui.scope_id().with(("rec", i)), Sense::click()).clicked() {
            if locked {
                app.tm.ui.alert = Some(if set.frozen { "The track is frozen.".into() } else { "The track is locked.\nTurn off its Protect button to record on it.".into() });
            } else if alt {
                // Option-click: only this one record-ready; on an armed one: none
                app.armed.clear();
                if !armed {
                    app.armed.insert(i);
                }
            } else if armed {
                app.armed.remove(&i);
            } else {
                app.armed.insert(i);
            }
        }
    }
    if cfg.input && !no_input(app, i) {
        let r = next(&mut x);
        // (Logic's I: #F2A33C)
        let orange = Color32::from_rgb(0xf2, 0xa3, 0x3c);
        let inst = t.kind != TrackKind::Audio;
        // instrument tracks: monitored while focused or record-enabled (an orange I); Option-click
        // turns that off (a white I)
        let auto = inst && (app.selected_track == Some(i) || app.armed.contains(&i));
        let off = app.tm.ui.monitor_off.contains(&t.extra.id);
        let mon = app.monitoring.contains(&i);
        paint(ui, r, if mon { orange } else { look::header_pill() }, cr(&corners));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "I", look::semibold(12.0), if mon { Color32::WHITE } else if auto && !off { orange } else if inst { look::text() } else { orange });
        if ui.interact(r, ui.scope_id().with(("mon", i)), Sense::click()).clicked() {
            if inst && alt {
                if !app.tm.ui.monitor_off.remove(&t.extra.id) {
                    app.tm.ui.monitor_off.insert(t.extra.id);
                }
            } else if mon {
                app.monitoring.remove(&i);
            } else {
                app.monitoring.insert(i);
                app.tm.ui.monitor_off.remove(&t.extra.id);
            }
        }
    }
    // Hide: only while the Hide button over the headers is green
    if app.ta.show_hidden && app.tm.ui.hide_button {
        let r = next(&mut x);
        let on = t.extra.hidden;
        paint(ui, r, if on { Color32::from_rgb(0x52, 0xc0, 0x3c) } else { look::header_pill() }, cr(&corners));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "H", look::semibold(12.0), if on { Color32::BLACK } else { look::text() });
        if let Some(state) = press(app, ui, r, Btn::Hide, on, "hide") {
            set_hidden(app, i, state);
        }
    }
    out.end_x = x + 3.0;
    out
}

/// Hides or shows a track; with a group linking Hide, its whole group.
pub fn set_hidden(app: &mut App, i: usize, hidden: bool) {
    let members = crate::mixstate::linked(app, i, crate::mixstate::L_HIDE);
    if let Some(p) = app.project.as_mut() {
        for k in members.into_iter().chain([i]) {
            if let Some(t) = p.tracks.get_mut(k) {
                t.extra.hidden = hidden;
            }
        }
    }
    app.tm.ui.hide_button = true;
    app.dirty = true;
}

/// The header's Pan knob set to a send (Track Header Components ▸ Pan/Send Knob Function):
/// it shows and sets that send's level. True when the knob was drawn here.
pub fn send_knob(app: &mut App, ui: &mut egui::Ui, hp: &egui::Painter, row: Rect, by: f32, i: usize) -> bool {
    let Some(k) = crate::tm::set(app, i).and_then(|s| s.knob_send).map(|k| k as usize) else { return false };
    let Some(snd) = app.sends.get(&i).and_then(|v| v.get(k)).copied() else { return false };
    let kc = pos2(row.max.x - 23.75, by + 8.0);
    let resp = ui.interact(Rect::from_center_size(kc, vec2(20.0, 20.0)), ui.scope_id().with(("hsend", i)), Sense::click_and_drag()).on_hover_text(format!("Send {} to Bus {}: {:.1} dB", k + 1, snd.bus + 1, snd.level_db));
    hp.circle_filled(kc, 9.0, Color32::from_rgb(0x2a, 0x5a, 0x86));
    let f = crate::tracks::db_to_frac(snd.level_db);
    let ang = std::f32::consts::PI * (0.75 + 1.5 * f);
    hp.line_segment([kc, kc + vec2(ang.cos(), ang.sin()) * 7.5], Stroke::new(1.8, Color32::WHITE));
    let mut new = None;
    if resp.dragged() {
        new = Some(crate::tracks::frac_to_db((f - resp.drag_delta().y / 100.0).clamp(0.0, 1.0)).min(6.0));
    }
    if resp.double_clicked() || (resp.clicked() && ui.input(|i| i.modifiers.alt)) {
        new = Some(0.0);
    }
    if let Some(db) = new {
        if let Some(x) = app.sends.get_mut(&i).and_then(|v| v.get_mut(k)) {
            x.level_db = db;
        }
        app.help_tag = Some(vec![(format!("Send {}", k + 1), format!("{db:.1} dB"))]);
        app.dirty = true;
        app.load_engine();
    }
    resp.context_menu(|ui| knob_menu(app, ui));
    true
}

/// The level meter inside the header's Volume slider: the track's output while it plays.
pub fn volume_meter(app: &App, hp: &egui::Painter, vr: Rect, i: usize) {
    let lv = app.engine.lock().map(|e| e.meters.get(i).copied().unwrap_or(0.0)).unwrap_or(0.0);
    if lv > 0.003 {
        let f = crate::tracks::db_to_frac(20.0 * lv.log10()).clamp(0.0, 1.0);
        let col = if lv > 0.98 { look::record_red() } else { Color32::from_rgb(0x4c, 0xd9, 0x64) };
        hp.rect_filled(Rect::from_min_max(pos2(vr.min.x, vr.center().y - 1.5), pos2(vr.min.x + f * vr.width(), vr.center().y + 1.5)), 1.0, col.gamma_multiply(0.8));
    }
}

/// The Add Tracks (+) button: a plain click opens New Tracks; Option-Shift-click makes an
/// empty track of the focused track's type, Shift-Command-click one of the other type.
pub fn add_clicked(app: &mut App, ui: &egui::Ui) {
    let m = ui.input(|i| i.modifiers);
    let ctx = ui.ctx().clone();
    if m.alt && m.shift {
        crate::newtrack::command(app, &ctx, "New Empty Track of Focused Type");
    } else if m.shift && m.command {
        crate::newtrack::command(app, &ctx, "New Empty Track of Opposite Type");
    } else if m.alt || m.command {
        // Option-click: the focused type with the default patch; Command-click: the other type
        crate::newtrack::with_default_patch(app, &ctx, m.command);
    } else {
        app.new_track_open = true;
    }
}

/// The Duplicate Track button: a click duplicates as the button is configured (with or
/// without content: its Control-click menu); Shift-click makes a track on the same channel
/// strip; Option-click duplicates with the content.
pub fn duplicate_button(app: &mut App, ui: &egui::Ui, resp: &egui::Response) {
    let key = egui::Id::unique("dup-with-content");
    let with_content: bool = ui.data(|d| d.get_temp(key)).unwrap_or(false);
    resp.context_menu(|ui| {
        ui.label(egui::RichText::new("Configure Button").color(look::text_dim()));
        for (v, label) in [(false, "New Track With Duplicate Settings"), (true, "New Track With Duplicate Settings and Content")] {
            if ui.add(egui::Button::selectable(with_content == v, label)).clicked() {
                ui.data_mut(|d| d.insert_temp(key, v));
                ui.close();
            }
        }
    });
    if !resp.clicked() {
        return;
    }
    let m = ui.input(|i| i.modifiers);
    let ctx = ui.ctx().clone();
    let cmd = if m.shift { "New Track With Same Channel" } else if m.alt || with_content { "New Track With Duplicate Settings and Content" } else { "New Track With Duplicate Settings" };
    crate::trackops::command(app, &ctx, cmd);
}

/// The empty header area under the tracks: a double-click makes a track like the selected
/// one; Control-click gives the track types (the new track goes under the selection).
pub fn below_headers(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    if r.height() < 4.0 {
        return;
    }
    let resp = ui.interact(r, ui.scope_id().with("below-headers"), Sense::click());
    let ctx = ui.ctx().clone();
    if resp.double_clicked() {
        crate::newtrack::command(app, &ctx, "New Track Like Selected");
    }
    resp.context_menu(|ui| {
        for c in ["New Audio Track", "New Software Instrument Track", "New Session Player SI Track…", "New External MIDI Track"] {
            if ui.button(c).clicked() {
                let before = app.selected_track;
                app.command(&ctx, c);
                // under the track that was selected
                if let (Some(sel), Some(new)) = (before, app.selected_track) {
                    let to = app.project.as_ref().map_or(new, |p| lg_core::arrange::group(&p.tracks, sel).end);
                    if to < new {
                        if let Some(p) = app.project.as_mut() {
                            let t = p.tracks.remove(new);
                            p.tracks.insert(to, t);
                        }
                        app.after_edit();
                        app.selected_track = Some(to);
                    }
                }
                ui.close();
            }
        }
    });
}

/// The Additional Name Column's text for a track.
pub fn name_column(app: &App, i: usize) -> Option<String> {
    if !app.tm.hdr.name_col {
        return None;
    }
    let t = app.project.as_ref()?.tracks.get(i)?;
    let inst = t.plugins.iter().find(|u| u.slot == 2);
    let kind = match t.kind {
        TrackKind::Audio => "Audio",
        _ => "Inst",
    };
    let nth = app.project.as_ref()?.tracks[..=i].iter().filter(|o| (o.kind == TrackKind::Audio) == (t.kind == TrackKind::Audio)).count();
    Some(match app.tm.hdr.name_mode {
        1 => inst.map(|u| u.preset.trim_end_matches(".pst").trim_start_matches('#').to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| t.name.clone()),
        2 => inst.map(|u| u.preset.trim_end_matches(".pst").trim_start_matches('#').to_string()).unwrap_or_default(),
        3 => t.name.clone(),
        4 => format!("{kind} {nth}"),
        _ => inst.map(|u| u.name.clone()).unwrap_or_else(|| format!("{kind} {nth}")),
    })
}

/// The buttons over the headers, right of Logic's three: Clear/Recall Solo once something was
/// soloed, and the Hide button (grey: all showing; orange: tracks are hidden; green: hidden
/// tracks show with their H buttons).
pub fn top_row(app: &mut App, ui: &mut egui::Ui, head: Rect) {
    let p = ui.painter_at(head);
    let mut x = head.min.x + 22.0 + 3.0 * 34.0;
    let any_solo = app.project.as_ref().is_some_and(|p| p.tracks.iter().any(|t| t.solo));
    if any_solo {
        app.tm.ui.solo_button = true;
    }
    if app.tm.ui.solo_button {
        let r = Rect::from_center_size(pos2(x, head.center().y), vec2(28.0, 24.0));
        let resp = ui.interact(r, ui.scope_id().with("clear-solo"), Sense::click()).on_hover_text(if any_solo { "Clear Solo" } else { "Recall Solo" });
        let yellow = Color32::from_rgb(0xe8, 0xc8, 0x3c);
        p.rect_filled(r, 6.0, if any_solo { yellow } else if resp.hovered() { look::button_on() } else { look::group_bg() });
        p.text(r.center(), Align2::CENTER_CENTER, "S", look::bold(13.0), if any_solo { Color32::BLACK } else { yellow });
        if resp.clicked() {
            clear_recall_solo(app);
        }
        x += 34.0;
    }
    let hidden = app.project.as_ref().is_some_and(|p| p.tracks.iter().any(|t| t.extra.hidden));
    if hidden {
        app.tm.ui.hide_button = true;
    }
    if app.tm.ui.hide_button {
        let r = Rect::from_center_size(pos2(x, head.center().y), vec2(28.0, 24.0));
        let resp = ui.interact(r, ui.scope_id().with("hide-view"), Sense::click()).on_hover_text("Toggle Hide View (H)");
        let (fill, ink) = if app.ta.show_hidden { (Color32::from_rgb(0x52, 0xc0, 0x3c), Color32::BLACK) } else if hidden { (Color32::from_rgb(0xe8, 0x9a, 0x2c), Color32::BLACK) } else { (look::group_bg(), look::text()) };
        p.rect_filled(r, 6.0, fill);
        p.text(r.center(), Align2::CENTER_CENTER, "H", look::bold(13.0), ink);
        if resp.clicked() {
            app.ta.show_hidden = !app.ta.show_hidden;
        }
    }
}

/// Clear/Recall Solo: every solo off (remembered), or the remembered ones back.
pub fn clear_recall_solo(app: &mut App) {
    let Some(p) = app.project.as_mut() else { return };
    if p.tracks.iter().any(|t| t.solo) {
        app.tm.ui.solo_recall = p.tracks.iter().filter(|t| t.solo).map(|t| t.extra.id).collect();
        for t in &mut p.tracks {
            t.solo = false;
        }
    } else {
        let ids = app.tm.ui.solo_recall.clone();
        for t in &mut p.tracks {
            t.solo = ids.contains(&t.extra.id);
        }
    }
    app.tm.ui.solo_button = true;
    app.sync_mix();
}

pub fn frame(app: &mut App, ctx: &egui::Context) {
    if !ctx.input(|i| i.pointer.primary_down()) {
        app.tm.ui.swipe = None;
    }
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    let sel = crate::trackarea::selection(app);
    match name {
        "Configure Track Header…" | "Configure Track Header" => {
            let at = ctx.pointer_latest_pos().unwrap_or(pos2(300.0, 200.0));
            app.tm.ui.popover = if app.tm.ui.popover.is_some() { None } else { Some(at) };
            app.tm.ui.popover_frames = 0;
        }
        "Store User Configuration" => {
            app.tm.hdr.store_user();
            app.tell("Track header configuration stored");
        }
        "Apply User Configuration" => {
            app.tm.hdr = Cfg::user_default();
            app.ta.show_alternatives = false;
        }
        "Revert to Defaults" => app.tm.hdr = Cfg::default(),
        "Assign Track Color…" | "Show Colors" => app.tm.ui.colors = if app.tm.ui.colors.is_some() { None } else { Some(ColorFor::Tracks) },
        "Assign Track Icon…" => app.tm.ui.icons = app.selected_track,
        "Search and Select Track…" => {
            app.tm.ui.search = Some(String::new());
            app.tm.ui.search_row = 0;
        }
        "Toggle Track On" | "Toggle Track Protect" | "Toggle Track Freeze" => {
            if sel.is_empty() {
                return false;
            }
            for t in sel {
                let Some(s) = crate::tm::set(app, t).cloned() else { continue };
                match name {
                    "Toggle Track On" => {
                        if let Some(x) = crate::tm::set_mut(app, t) {
                            x.off = !s.off;
                            x.offline = false;
                        }
                    }
                    "Toggle Track Protect" => {
                        if let Some(x) = crate::tm::set_mut(app, t) {
                            x.protected = !s.protected;
                        }
                    }
                    _ => crate::freeze::set_frozen(app, t, !s.frozen),
                }
            }
            app.load_engine();
        }
        "Toggle Activation of All Tracks" => {
            let Some(p) = app.project.as_mut() else { return false };
            let off = !p.tracks.iter().all(|t| t.extra.set.off);
            for t in &mut p.tracks {
                t.extra.set.off = off;
            }
            app.load_engine();
        }
        "Toggle Track Mute of all Tracks of Folder" => {
            // the tracks of the focused track's stack or folder (the whole list at the top level)
            let Some(t) = app.selected_track else { return false };
            let Some(p) = app.project.as_mut() else { return false };
            let range = match lg_core::arrange::parent(&p.tracks, t) {
                Some(par) => lg_core::arrange::members(&p.tracks, par),
                None if p.tracks.get(t).is_some_and(|x| x.extra.stack.is_some()) => lg_core::arrange::members(&p.tracks, t),
                None => 0..p.tracks.len(),
            };
            let mute = !p.tracks[range.clone()].iter().all(|x| x.mute);
            for x in &mut p.tracks[range] {
                x.mute = mute;
            }
            app.sync_mix();
        }
        "Clear/Recall Solo" => clear_recall_solo(app),
        "Set Groove Track" => {
            let Some(t) = app.selected_track else { return false };
            if crate::tm::set(app, t).is_some_and(|s| s.groove) {
                if let Some(s) = crate::tm::set_mut(app, t) {
                    s.groove = false;
                }
                app.load_engine();
            } else {
                set_groove(app, t);
            }
        }
        "Toggle Match Groove Track" => {
            for t in sel {
                if let Some(s) = crate::tm::set_mut(app, t) {
                    s.match_groove = !s.match_groove;
                }
            }
            app.load_engine();
        }
        "Individual Track Zoom Reset for All Tracks" => reset_all_zoom(app),
        _ => return false,
    }
    true
}

pub fn checked(app: &App, label: &str) -> Option<bool> {
    let _ = (app, label);
    None
}

/// The component switches by the labels of Logic's Track Header Components menu.
fn component<'a>(cfg: &'a mut Cfg, label: &str) -> Option<&'a mut bool> {
    Some(match label {
        "On/Off" => &mut cfg.on_off,
        "Mute" => &mut cfg.mute,
        "Solo" => &mut cfg.solo,
        "Track Protect" | "Protect" => &mut cfg.protect,
        "Freeze" => &mut cfg.freeze,
        "Record Enable" => &mut cfg.record,
        "Input Monitoring" => &mut cfg.input,
        "Volume" => &mut cfg.volume,
        "Pan/Send" | "Pan" => &mut cfg.pan,
        "Additional Name Column" => &mut cfg.name_col,
        "Control Surface Bars" => &mut cfg.surface_bars,
        "Track Numbers" => &mut cfg.numbers,
        "Track Color Bars" | "Color Bars" => &mut cfg.color_bars,
        "Groove Track" => &mut cfg.groove,
        "Track Icons" => &mut cfg.icons,
        _ => return None,
    })
}

/// The knob's function for the selected tracks: Pan, or one of their sends.
pub fn knob_menu(app: &mut App, ui: &mut egui::Ui) {
    let t = app.selected_track.unwrap_or(0);
    let cur = crate::tm::set(app, t).and_then(|s| s.knob_send);
    let n = app.sends.get(&t).map_or(0, |v| v.len());
    let mut pick: Option<Option<u8>> = None;
    if ui.add(egui::Button::selectable(cur.is_none(), "Pan")).clicked() {
        pick = Some(None);
    }
    for k in 0..8u8 {
        let label = match app.sends.get(&t).and_then(|v| v.get(k as usize)) {
            Some(s) => format!("Send {} (Bus {})", k + 1, s.bus + 1),
            None => format!("Send {}", k + 1),
        };
        if ui.add_enabled((k as usize) < n, egui::Button::selectable(cur == Some(k), label)).clicked() {
            pick = Some(Some(k));
        }
    }
    if let Some(v) = pick {
        for t in crate::trackarea::selection(app) {
            if let Some(s) = crate::tm::set_mut(app, t) {
                s.knob_send = v;
            }
        }
        ui.close();
    }
}

pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    match it.label.as_str() {
        "Track Header Components" => {
            ui.menu_button("Track Header Components", |ui| {
                ui.set_min_width(230.0);
                for c in &it.children {
                    if c.separator {
                        ui.separator();
                        continue;
                    }
                    match c.label.as_str() {
                        "Pan/Send Knob Function" => {
                            ui.menu_button("Pan/Send Knob Function", |ui| knob_menu(app, ui));
                        }
                        "Additional Name Column Mode" => {
                            ui.menu_button("Additional Name Column Mode", |ui| {
                                for (k, m) in NAME_MODES.iter().enumerate() {
                                    if ui.add(egui::Button::selectable(app.tm.hdr.name_mode == k as u8, *m)).clicked() {
                                        app.tm.hdr.name_mode = k as u8;
                                        app.tm.hdr.name_col = true;
                                        ui.close();
                                    }
                                }
                            });
                        }
                        "Track Alternatives" => {
                            let on = app.ta.show_alternatives;
                            if ui.button(if on { "✓ Track Alternatives" } else { "Track Alternatives" }).clicked() {
                                app.ta.show_alternatives = !on;
                                ui.close();
                            }
                        }
                        l => {
                            let mut cfg = app.tm.hdr.clone();
                            if let Some(v) = component(&mut cfg, l) {
                                let on = *v;
                                if ui.button(if on { format!("✓ {l}") } else { l.to_string() }).clicked() {
                                    *v = !on;
                                    app.tm.hdr = cfg;
                                    ui.close();
                                }
                            }
                        }
                    }
                }
            });
            true
        }
        _ => false,
    }
}

fn check(ui: &mut egui::Ui, label: &str, v: &mut bool) {
    ui.checkbox(v, label);
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    popover(app, ctx);
    color_palette(app, ctx);
    icon_palette(app, ctx);
    search(app, ctx);
    if let Some(text) = app.tm.ui.alert.clone() {
        let mut close = false;
        egui::Window::new("Track Locked").id(egui::Id::unique("tm-alert")).title_bar(false).resizable(false).collapsible(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -60.0)).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.add_space(6.0);
            ui.vertical_centered(|ui| {
                glyphs::paint(ui.painter(), Rect::from_center_size(ui.cursor().min + vec2(150.0, 16.0), vec2(30.0, 30.0)), ph::LOCK, 28.0, look::text_dim());
                ui.add_space(36.0);
                ui.label(egui::RichText::new(text).font(look::regular(13.0)));
                ui.add_space(8.0);
                if ui.add(egui::Button::new(egui::RichText::new("OK").color(Color32::WHITE)).fill(look::accent()).min_size(vec2(80.0, 26.0))).clicked() {
                    close = true;
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
        if close {
            app.tm.ui.alert = None;
        }
    }
}

/// The Track Header Configuration popover: Buttons, Controls, Additional Name Column, Other,
/// and the More menu; a click outside closes it.
fn popover(app: &mut App, ctx: &egui::Context) {
    let Some(at) = app.tm.ui.popover else { return };
    app.tm.ui.popover_frames += 1;
    let mut cfg = app.tm.hdr.clone();
    let mut alts = app.ta.show_alternatives;
    let mut close = false;
    let resp = egui::Area::new(egui::Id::unique("track-header-config")).order(egui::Order::Foreground).fixed_pos(at + vec2(8.0, 8.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(250.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Track Header Configuration").font(look::semibold(13.0)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(egui::RichText::new(ph::DOTS_THREE).family(glyphs::family()), |ui| {
                        if ui.button("Store User Configuration").clicked() {
                            cfg.store_user();
                            ui.close();
                        }
                        if ui.button("Apply User Configuration").clicked() {
                            cfg = Cfg::user_default();
                            ui.close();
                        }
                        if ui.button("Revert to Defaults").clicked() {
                            cfg = Cfg::default();
                            alts = false;
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text("More");
                });
            });
            ui.separator();
            ui.label(egui::RichText::new("Buttons").color(look::text_dim()));
            ui.columns(2, |c| {
                check(&mut c[0], "On/Off", &mut cfg.on_off);
                check(&mut c[0], "Mute", &mut cfg.mute);
                check(&mut c[0], "Solo", &mut cfg.solo);
                check(&mut c[0], "Protect", &mut cfg.protect);
                check(&mut c[1], "Freeze", &mut cfg.freeze);
                check(&mut c[1], "Record Enable", &mut cfg.record);
                check(&mut c[1], "Input Monitoring", &mut cfg.input);
            });
            ui.separator();
            ui.label(egui::RichText::new("Controls").color(look::text_dim()));
            check(ui, "Volume", &mut cfg.volume);
            ui.horizontal(|ui| {
                check(ui, "Pan", &mut cfg.pan);
                let t = app.selected_track.unwrap_or(0);
                let cur = crate::tm::set(app, t).and_then(|s| s.knob_send);
                ui.menu_button(cur.map_or("Pan".to_string(), |k| format!("Send {}", k + 1)), |ui| knob_menu(app, ui));
            });
            ui.separator();
            ui.horizontal(|ui| {
                check(ui, "Additional Name Column", &mut cfg.name_col);
            });
            egui::ComboBox::from_id_salt("hdr-name-mode").width(220.0).selected_text(NAME_MODES[cfg.name_mode.min(4) as usize]).show_ui(ui, |ui| {
                for (k, m) in NAME_MODES.iter().enumerate() {
                    ui.selectable_value(&mut cfg.name_mode, k as u8, *m);
                }
            });
            ui.separator();
            ui.label(egui::RichText::new("Other").color(look::text_dim()));
            check(ui, "Control Surface Bars", &mut cfg.surface_bars);
            check(ui, "Track Numbers", &mut cfg.numbers);
            check(ui, "Color Bars", &mut cfg.color_bars);
            check(ui, "Groove Track", &mut cfg.groove);
            check(ui, "Track Icons", &mut cfg.icons);
            check(ui, "Track Alternatives", &mut alts);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Revert").clicked() {
                    cfg = Cfg::default();
                    alts = false;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Done").clicked() {
                        close = true;
                    }
                });
            });
        });
    });
    app.tm.hdr = cfg;
    app.ta.show_alternatives = alts;
    // a click outside (not the one that opened it) closes it, and so does Escape
    let outside = ctx.input(|i| i.pointer.any_pressed()) && ctx.pointer_latest_pos().is_some_and(|p| !resp.response.rect.expand(4.0).contains(p)) && !egui::Popup::is_any_open(ctx);
    if close || (outside && app.tm.ui.popover_frames > 2) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.tm.ui.popover = None;
    }
}

/// Sets the colour of the selected tracks (a group linking Color takes its members along) or of
/// one channel strip; a track's colour is its channel strip's too.
pub fn assign_color(app: &mut App, target: ColorFor, color: Option<u8>) {
    if crate::markers::assign_color(app, color) {
        return;
    }
    match target {
        ColorFor::Tracks => {
            let mut tracks = crate::trackarea::selection(app);
            for t in tracks.clone() {
                tracks.extend(crate::mixstate::linked(app, t, crate::mixstate::L_COLOR));
            }
            tracks.sort();
            tracks.dedup();
            if tracks.is_empty() {
                return;
            }
            app.checkpoint("Assign Track Color");
            for t in tracks {
                if let Some(s) = crate::tm::set_mut(app, t) {
                    s.strip.color = color;
                }
            }
        }
        ColorFor::Strip(s) if s < crate::mixer::AUX => {
            app.checkpoint("Assign Channel Strip Color");
            if let Some(x) = crate::tm::set_mut(app, s) {
                x.strip.color = color;
            }
        }
        ColorFor::Strip(s) => {
            app.tm.strips.entry(s).or_default().color = color;
            app.dirty = true;
        }
    }
}

fn color_palette(app: &mut App, ctx: &egui::Context) {
    let Some(target) = app.tm.ui.colors else { return };
    let mut open = true;
    let mut pick: Option<Option<u8>> = None;
    let current = match target {
        ColorFor::Tracks => app.selected_track.and_then(|t| crate::tm::set(app, t)).and_then(|s| s.strip.color),
        ColorFor::Strip(s) => crate::tm::strip(app, s).color,
    };
    egui::Window::new("Colors").id(egui::Id::unique("tm-colors")).open(&mut open).resizable(false).collapsible(false).default_pos(pos2(420.0, 160.0)).show(ctx, |ui| {
        let (cols, rows) = (trackmix::PALETTE_COLS, trackmix::PALETTE_ROWS);
        let cell = 18.0;
        let (r, _) = ui.allocate_exact_size(vec2(cols as f32 * cell, rows as f32 * cell), Sense::hover());
        for k in 0..(cols * rows) as u8 {
            let c = trackmix::palette(k);
            let cr = Rect::from_min_size(r.min + vec2((k as usize % cols) as f32 * cell, (k as usize / cols) as f32 * cell), vec2(cell, cell)).shrink(1.5);
            ui.painter().rect_filled(cr, 3.0, Color32::from_rgb(c[0], c[1], c[2]));
            if current == Some(k) || crate::palette::is_selected(app, k) {
                ui.painter().rect_stroke(cr.expand(1.0), 4.0, Stroke::new(2.0, look::text()), egui::StrokeKind::Outside);
            }
            if ui.interact(cr, ui.scope_id().with(("pal", k)), Sense::click()).clicked() && !crate::palette::swatch_clicked(app, k) {
                pick = Some(Some(k));
            }
        }
        ui.add_space(4.0);
        if ui.button("Default Color").on_hover_text("The track type's colour").clicked() {
            pick = Some(None);
        }
        // Bramble's custom palette: edit, save, load, reset
        ui.separator();
        crate::palette::controls(app, ui);
    });
    if let Some(c) = pick {
        assign_color(app, target, c);
    }
    if !open {
        app.tm.ui.colors = None;
    }
}

fn set_icon(app: &mut App, t: usize, icon: Option<String>) {
    app.checkpoint("Assign Track Icon");
    if let Some(s) = crate::tm::set_mut(app, t) {
        s.icon = icon;
    }
}

/// The Icon palette: the built-in icons, then Custom Icons with its Add (+) button; a custom
/// icon's shortcut menu removes or replaces it, or shows its file.
fn icon_palette(app: &mut App, ctx: &egui::Context) {
    let Some(t) = app.tm.ui.icons else { return };
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)).cloned() else {
        app.tm.ui.icons = None;
        return;
    };
    let mut open = true;
    let mut pick: Option<Option<String>> = None;
    let mut add = false;
    let mut remove: Option<String> = None;
    let mut replace: Option<String> = None;
    let customs = custom_icons();
    let textures: Vec<Option<egui::TextureHandle>> = customs.iter().map(|n| custom_texture(app, ctx, n)).collect();
    let col = color_of(&tr);
    egui::Window::new("Track Icon").id(egui::Id::unique("tm-icons")).open(&mut open).resizable(false).collapsible(false).default_pos(pos2(360.0, 140.0)).show(ctx, |ui| {
        ui.set_width(10.0 * 36.0);
        let cell = 36.0;
        let grid = |ui: &mut egui::Ui, n: usize| {
            let rows = n.div_ceil(10).max(1);
            ui.allocate_exact_size(vec2(10.0 * cell, rows as f32 * cell), Sense::hover()).0
        };
        let r = grid(ui, ICONS.len());
        for (k, (name, g)) in ICONS.iter().enumerate() {
            let cr = Rect::from_min_size(r.min + vec2((k % 10) as f32 * cell, (k / 10) as f32 * cell), vec2(cell, cell)).shrink(2.0);
            let resp = ui.interact(cr, ui.scope_id().with(("icon", k)), Sense::click()).on_hover_text(*name);
            let on = tr.extra.set.icon.as_deref() == Some(*name);
            ui.painter().rect_filled(cr, 6.0, if on { look::accent().gamma_multiply(0.45) } else if resp.hovered() { look::button_on() } else { look::group_bg() });
            glyphs::paint(ui.painter(), cr, g, 22.0, col);
            if resp.clicked() {
                pick = Some(Some(name.to_string()));
            }
        }
        if ui.button("Default Icon").on_hover_text("The icon of the track's patch").clicked() {
            pick = Some(None);
        }
        // (Option: the MMC icon, under Other)
        if let Some(n) = crate::syncmmc::icon_row(ui, tr.extra.set.icon.as_deref()) {
            pick = Some(Some(n));
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(egui::RichText::new(ph::PLUS).family(glyphs::family())).on_hover_text("Add a custom icon (a square PNG, 1024 × 1024, with transparency)").clicked() {
                add = true;
            }
            ui.label(egui::RichText::new("Custom Icons").color(look::text_dim()));
        });
        let r = grid(ui, customs.len());
        for (k, name) in customs.iter().enumerate() {
            let cr = Rect::from_min_size(r.min + vec2((k % 10) as f32 * cell, (k / 10) as f32 * cell), vec2(cell, cell)).shrink(2.0);
            let resp = ui.interact(cr, ui.scope_id().with(("custom-icon", k)), Sense::click()).on_hover_text(name.trim_end_matches(".png"));
            let on = tr.extra.set.icon.as_deref() == Some(&format!("custom:{name}"));
            ui.painter().rect_filled(cr, 6.0, if on { look::accent().gamma_multiply(0.45) } else if resp.hovered() { look::button_on() } else { look::group_bg() });
            if let Some(tex) = &textures[k] {
                ui.painter().image(tex.id(), cr.shrink(2.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            if resp.clicked() {
                pick = Some(Some(format!("custom:{name}")));
            }
            resp.context_menu(|ui| {
                if ui.button("Remove").clicked() {
                    remove = Some(name.clone());
                    ui.close();
                }
                if ui.button("Replace…").clicked() {
                    replace = Some(name.clone());
                    ui.close();
                }
                if ui.button("Show in Files").clicked() {
                    if let Some(d) = custom_dir() {
                        let _ = std::process::Command::new("xdg-open").arg(d).spawn();
                    }
                    ui.close();
                }
            });
        }
        if customs.is_empty() {
            ui.label(egui::RichText::new("No custom icons yet").color(look::text_dim()));
        }
    });
    if let Some(icon) = pick {
        set_icon(app, t, icon);
    }
    let choose = || kn_ui::files::FileDialog::new().set_title("Choose an icon").add_filter("Pictures", &["png", "jpg", "jpeg"]).pick_file();
    if add && !app.automated {
        if let Some(name) = choose().and_then(|f| add_custom_icon(&f, None)) {
            set_icon(app, t, Some(format!("custom:{name}")));
        }
    }
    if let Some(name) = replace {
        if !app.automated && choose().and_then(|f| add_custom_icon(&f, Some(&name))).is_some() {
            app.tm.ui.custom.remove(&name);
        }
    }
    if let Some(name) = remove {
        if let Some(d) = custom_dir() {
            let _ = std::fs::remove_file(d.join(&name));
        }
        app.tm.ui.custom.remove(&name);
    }
    if !open {
        app.tm.ui.icons = None;
    }
}

/// The tracks matching what was typed in Search and Select Track: by name or number.
pub fn search_matches(app: &App, text: &str) -> Vec<(usize, String, bool)> {
    let Some(p) = app.project.as_ref() else { return Vec::new() };
    let q = text.trim().to_lowercase();
    p.tracks
        .iter()
        .enumerate()
        .filter(|(i, t)| q.is_empty() || t.name.to_lowercase().contains(&q) || (i + 1).to_string() == q || (i + 1).to_string().starts_with(&q) && q.chars().all(|c| c.is_ascii_digit()))
        .map(|(i, t)| (i, t.name.clone(), t.extra.hidden))
        .collect()
}

/// Selects a track found by the search: a hidden one shows again, the Tracks area scrolls to it.
pub fn select_found(app: &mut App, t: usize) {
    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
        tr.extra.hidden = false;
    }
    // (inside a closed stack: open its stacks)
    if let Some(p) = app.project.as_mut() {
        for a in lg_core::arrange::ancestors(&p.tracks, t) {
            if let Some(s) = p.tracks[a].extra.stack.as_mut() {
                s.open = true;
            }
        }
    }
    crate::trackarea::click_track(app, t, false, false);
    let rows = crate::trackarea::rows(app);
    if let Some(r) = rows.iter().find(|r| r.t == t) {
        app.scroll_y = (r.y - 60.0).max(0.0);
    }
}

fn search(app: &mut App, ctx: &egui::Context) {
    let Some(mut text) = app.tm.ui.search.clone() else { return };
    let matches = search_matches(app, &text);
    let mut row = app.tm.ui.search_row.min(matches.len().saturating_sub(1));
    let mut close = false;
    let mut choose: Option<usize> = None;
    let screen = ctx.content_rect();
    let resp = egui::Area::new(egui::Id::unique("track-search")).order(egui::Order::Foreground).fixed_pos(pos2(screen.center().x - 170.0, screen.min.y + 120.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(340.0);
            ui.horizontal(|ui| {
                glyphs::paint(ui.painter(), Rect::from_min_size(ui.cursor().min, vec2(18.0, 20.0)), ph::MAGNIFYING_GLASS, 14.0, look::text_dim());
                ui.add_space(20.0);
                let te = ui.add(egui::TextEdit::singleline(&mut text).hint_text("Track name or number").desired_width(270.0));
                te.request_focus();
                // the Close button at the right clears the field
                if ui.add(egui::Button::new(egui::RichText::new(ph::X_CIRCLE).family(glyphs::family())).frame(false)).on_hover_text("Clear").clicked() {
                    text.clear();
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for (k, (i, name, hidden)) in matches.iter().enumerate() {
                    let label = format!("{:>3}   {}{}", i + 1, name, if *hidden { "   (hidden)" } else { "" });
                    let r = ui.add(egui::Button::selectable(k == row, label).min_size(vec2(330.0, 0.0)));
                    if r.clicked() {
                        choose = Some(*i);
                    }
                }
                if matches.is_empty() {
                    ui.label(egui::RichText::new("No tracks found").color(look::text_dim()));
                }
            });
        });
    });
    ctx.input(|i| {
        if i.key_pressed(egui::Key::ArrowDown) {
            row = (row + 1).min(matches.len().saturating_sub(1));
        }
        if i.key_pressed(egui::Key::ArrowUp) {
            row = row.saturating_sub(1);
        }
        if i.key_pressed(egui::Key::Enter) {
            choose = matches.get(row).map(|m| m.0);
        }
        if i.key_pressed(egui::Key::Escape) {
            close = true;
        }
        if i.pointer.any_pressed() && i.pointer.latest_pos().is_some_and(|p| !resp.response.rect.expand(4.0).contains(p)) {
            close = true;
        }
    });
    app.tm.ui.search_row = row;
    if let Some(t) = choose {
        select_found(app, t);
        close = true;
    }
    app.tm.ui.search = if close { None } else { Some(text) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_configuration_round_trips() {
        let mut c = Cfg::default();
        assert!(c.mute && c.solo && c.record && c.input && c.volume && c.pan && c.numbers && c.icons && !c.on_off && !c.protect && !c.freeze && !c.color_bars && !c.groove);
        c.on_off = true;
        c.freeze = true;
        c.mute = false;
        c.name_mode = 4;
        c.color_bars = true;
        assert_eq!(Cfg::from_text(&c.to_text()), c);
    }

    #[test]
    fn components_by_menu_label() {
        let mut c = Cfg::default();
        for l in ["On/Off", "Mute", "Solo", "Track Protect", "Freeze", "Record Enable", "Input Monitoring", "Volume", "Pan/Send", "Additional Name Column", "Control Surface Bars", "Track Numbers", "Track Color Bars", "Groove Track", "Track Icons"] {
            assert!(component(&mut c, l).is_some(), "{l}");
        }
    }

    #[test]
    fn search_finds_by_name_or_number_and_unhides() {
        let mut app = crate::tm::test_app();
        let n = app.project.as_ref().unwrap().tracks.len();
        assert!(n >= 2);
        let name = app.project.as_ref().unwrap().tracks[1].name.clone();
        let m = search_matches(&app, &name.to_uppercase());
        assert!(m.iter().any(|x| x.0 == 1));
        assert_eq!(search_matches(&app, "2").iter().filter(|x| x.0 == 1).count(), 1);
        assert_eq!(search_matches(&app, "").len(), n);
        app.project.as_mut().unwrap().tracks[1].extra.hidden = true;
        assert!(search_matches(&app, &name).iter().any(|x| x.0 == 1 && x.2), "hidden tracks are found, marked");
        select_found(&mut app, 1);
        assert!(!app.project.as_ref().unwrap().tracks[1].extra.hidden);
        assert_eq!(app.selected_track, Some(1));
    }

    #[test]
    fn one_groove_track_and_clear_recall_solo() {
        let mut app = crate::tm::test_app();
        set_groove(&mut app, 0);
        set_groove(&mut app, 1);
        let p = app.project.as_ref().unwrap();
        assert_eq!(p.tracks.iter().filter(|t| t.extra.set.groove).count(), 1);
        assert!(p.tracks[1].extra.set.groove);
        app.project.as_mut().unwrap().tracks[0].solo = true;
        clear_recall_solo(&mut app);
        assert!(!app.project.as_ref().unwrap().tracks[0].solo);
        clear_recall_solo(&mut app);
        assert!(app.project.as_ref().unwrap().tracks[0].solo, "recalled");
    }

    #[test]
    fn colours_follow_the_palette_and_the_strip() {
        let mut app = crate::tm::test_app();
        app.selected_track = Some(0);
        app.ta.selected = vec![0];
        assign_color(&mut app, ColorFor::Tracks, Some(5));
        let t = &app.project.as_ref().unwrap().tracks[0];
        let [r, g, b] = trackmix::palette(5);
        assert_eq!(color_of(t), Color32::from_rgb(r, g, b));
        assert_eq!(strip_color(&app, 0), Some(Color32::from_rgb(r, g, b)), "the channel strip takes the track's colour");
        app.undo();
        assert_eq!(app.project.as_ref().unwrap().tracks[0].extra.set.strip.color, None);
        assign_color(&mut app, ColorFor::Strip(crate::mixer::AUX), Some(9));
        assert!(strip_color(&app, crate::mixer::AUX).is_some());
    }
}
