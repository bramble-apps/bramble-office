//! Logic 13's Create New Track dialog (⌥⌘N, the + over the track headers): the MIDI / Pattern /
//! Session Player / Audio cards with their own choices (Software Instrument, External MIDI;
//! Drummer, Bass Player, Keyboard Player; Mic or Line, Guitar or Bass), the Details area behind
//! its disclosure arrow (searchable Audio Input and Output with Ascending, the devices with an
//! arrow to the Audio settings, Load Default Patch, Open Browser, Input Monitoring, Record
//! Enable; Instrument, Multi-timbral; Use External Instrument plug-in, MIDI Destination with
//! Ascending; Style, Use Default Chord Progression, Open Editor), the number of tracks, Create.
//! The track key commands and the Add button's modifier clicks use the same Details.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::trackmix::{External, RegionType};
use lg_core::TrackKind;

use crate::app::App;
use crate::audio::Out;
use crate::look;
use crate::mixstate::Input;

#[allow(dead_code)] // (read by build.rs and tool/checklist.py)
pub const COMMANDS: &[&str] = &["New Empty Track of Focused Type", "New Empty Track of Opposite Type", "New Track Like Selected"];

/// The card chosen (kept for the older callers: the control bar and key commands set it).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    #[default]
    Instrument,
    Pattern,
    Drummer,
    Bass,
    Keys,
    Audio,
    Guitar,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Main {
    Audio,
    #[default]
    Midi,
    Pattern,
    Player,
}

/// What the Instrument pop-up loads.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Inst {
    #[default]
    DefaultPatch,
    Empty,
    Plugin(String),
}

/// The New Tracks dialog's choices (kept between uses; the key commands read them).
#[derive(Clone, Debug, PartialEq)]
pub struct Details {
    pub open: bool,
    pub main: Main,
    /// Which choice under the type button (Mic or Line / Guitar or Bass; Software Instrument /
    /// External MIDI; Drummer / Bass Player / Keyboard Player).
    pub sub: usize,
    pub input: Input,
    pub input_ascending: bool,
    pub output: Out,
    pub output_ascending: bool,
    pub load_default_patch: bool,
    pub open_browser: bool,
    pub monitoring: bool,
    pub record_enable: bool,
    pub instrument: Inst,
    pub multi_timbral: bool,
    pub parts: u8,
    pub external_plugin: bool,
    pub midi_port: String,
    pub midi_channel: u8,
    pub midi_ascending: bool,
    pub style: usize,
    pub default_chords: bool,
    pub open_editor: bool,
    /// Each card's own choice, kept while another card is chosen (MIDI, Pattern, Session Player,
    /// Audio; Logic shows it lightly filled on the cards not chosen).
    pub remembered: [usize; 4],
    search: String,
    /// The Number of tracks to create field's text while typing.
    count_text: Option<String>,
}

impl Default for Details {
    fn default() -> Self {
        Details { open: false, main: Main::Midi, sub: 0, input: Input::Mono0, input_ascending: false, output: Out::Stereo, output_ascending: false, load_default_patch: true, open_browser: false, monitoring: false, record_enable: false, instrument: Inst::DefaultPatch, multi_timbral: false, parts: 2, external_plugin: false, midi_port: String::new(), midi_channel: 1, midi_ascending: true, style: 0, default_chords: true, open_editor: true, remembered: [0; 4], search: String::new(), count_text: None }
    }
}

impl Details {
    pub fn kind(&self) -> Kind {
        match (self.main, self.sub) {
            (Main::Audio, 1) => Kind::Guitar,
            (Main::Audio, _) => Kind::Audio,
            (Main::Midi, _) => Kind::Instrument,
            (Main::Pattern, _) => Kind::Pattern,
            (Main::Player, 1) => Kind::Bass,
            (Main::Player, 2) => Kind::Keys,
            (Main::Player, _) => Kind::Drummer,
        }
    }
    pub fn set_kind(&mut self, k: Kind) {
        (self.main, self.sub) = match k {
            Kind::Audio => (Main::Audio, 0),
            Kind::Guitar => (Main::Audio, 1),
            Kind::Instrument => (Main::Midi, if self.main == Main::Midi { self.sub.min(1) } else { 0 }),
            Kind::Pattern => (Main::Pattern, if self.main == Main::Pattern { self.sub.min(1) } else { 0 }),
            Kind::Drummer => (Main::Player, 0),
            Kind::Bass => (Main::Player, 1),
            Kind::Keys => (Main::Player, 2),
        };
    }
    /// Chooses a card, with the choice it had last.
    pub fn choose_card(&mut self, m: Main) {
        if self.main != m {
            self.main = m;
            self.sub = self.remembered[card(m)].min(subs(m).len() - 1);
            self.style = 0;
        }
    }
    /// Chooses a card's option.
    pub fn choose(&mut self, m: Main, k: usize) {
        if self.main != m || self.sub != k {
            self.style = 0;
        }
        self.main = m;
        self.sub = k.min(subs(m).len() - 1);
        self.remembered[card(m)] = self.sub;
    }
    /// The arrow keys: left/right go to the next card, up/down to the next option on the card.
    pub fn arrow(&mut self, dx: i32, dy: i32) {
        if dx != 0 {
            let i = (card(self.main) as i32 + dx).clamp(0, 3) as usize;
            self.choose_card(CARDS[i].0);
        }
        if dy != 0 {
            let n = subs(self.main).len() as i32;
            self.choose(self.main, (self.sub as i32 + dy).clamp(0, n - 1) as usize);
        }
    }
    /// External MIDI chosen (MIDI or Pattern).
    pub fn external(&self) -> bool {
        matches!(self.main, Main::Midi | Main::Pattern) && self.sub == 1
    }
}

/// The cards in Logic's order: (type, name).
const CARDS: [(Main, &str); 4] = [(Main::Midi, "MIDI"), (Main::Pattern, "Pattern"), (Main::Player, "Session Player"), (Main::Audio, "Audio")];

fn card(m: Main) -> usize {
    CARDS.iter().position(|c| c.0 == m).unwrap_or(0)
}

fn subs(m: Main) -> &'static [&'static str] {
    match m {
        Main::Audio => &["Mic or Line", "Guitar or Bass"],
        Main::Midi | Main::Pattern => &["Software Instrument", "External MIDI"],
        Main::Player => &["Drummer", "Bass Player", "Keyboard Player"],
    }
}

/// The Session Player styles offered for the chosen player.
pub fn styles(sub: usize) -> Vec<String> {
    match sub {
        1 => lg_players::bass::BassStyle::ALL.iter().map(|s| s.id().to_string()).collect(),
        2 => lg_players::keys::KeysStyle::ALL.iter().map(|s| s.id().to_string()).collect(),
        _ => lg_players::character::DRUMMERS.iter().map(|c| format!("{} ({})", c.name, c.personality)).collect(),
    }
}

fn inputs(app: &App) -> Vec<(Input, String)> {
    let mut v = vec![(Input::None, "No Input".to_string())];
    for k in 0..8 {
        v.push((if k == 0 { Input::Mono0 } else { Input::Mono(k) }, crate::mixext::io_label(app, crate::mixext::Io::Input(k), true)));
    }
    for k in 0..4 {
        v.push((Input::Stereo(2 * k), format!("Input {}-{}", 2 * k + 1, 2 * k + 2)));
    }
    for b in 0..8 {
        v.push((Input::Bus(b), crate::mixext::io_label(app, crate::mixext::Io::Bus(b), true)));
    }
    v
}

fn outputs(app: &App) -> Vec<(Out, String)> {
    let mut v = vec![(Out::Stereo, "Stereo Out".to_string()), (Out::Off, "No Output".to_string())];
    for b in 0..16 {
        v.push((Out::Bus(b), crate::mixext::io_label(app, crate::mixext::Io::Bus(b), true)));
    }
    v
}

fn midi_destinations(app: &App) -> Vec<(String, String)> {
    let mut v = vec![(String::new(), crate::midiout::VIRTUAL.to_string())];
    for p in app.hub.outs.ports() {
        if p != crate::midiout::VIRTUAL {
            v.push((p.clone(), crate::live::port_label(&p)));
        }
    }
    v
}

/// A pop-up with a search field at its top; the choice made.
pub fn searchable<T: Clone + PartialEq>(ui: &mut egui::Ui, current: &T, items: &[(T, String)], search: &mut String) -> Option<T> {
    let shown = items.iter().find(|x| x.0 == *current).map_or("—", |x| x.1.as_str());
    let mut pick = None;
    ui.menu_button(format!("{shown}  ⌄"), |ui| {
        ui.set_min_width(220.0);
        ui.add(egui::TextEdit::singleline(search).hint_text("Search").desired_width(200.0)).request_focus();
        ui.separator();
        let q = search.to_lowercase();
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for (v, name) in items.iter().filter(|x| q.is_empty() || x.1.to_lowercase().contains(&q)) {
                if ui.add(egui::Button::selectable(v == current, name.as_str())).clicked() {
                    pick = Some(v.clone());
                    ui.close();
                }
            }
        });
    });
    if pick.is_some() {
        search.clear();
    }
    pick
}

fn instruments() -> Vec<(Inst, String)> {
    let mut v = vec![(Inst::DefaultPatch, "Default Patch".to_string()), (Inst::Empty, "Empty Channel Strip".to_string())];
    for i in lg_synth::list() {
        if let Some(n) = i.logic_names.first() {
            v.push((Inst::Plugin(n.to_string()), n.to_string()));
        }
    }
    v
}

// Logic 13's Create New Track dialog, measured on its screenshot (points): 930 wide, the title
// 13.75 from the top, four 212×228 cards (radius 16, 13 apart, 21.5 in) from 45, each with a 56
// circle 16 in, the name centred at 90.75 and 175×28 options (radius 6) every 34 from 112; the
// Details row at 311.5, the bottom row (? · Number of tracks to create · Cancel Create) at 353.
const W: f32 = 930.0;
const CARD_W: f32 = 212.0;
const CARD_H: f32 = 228.0;
const CARD_GAP: f32 = 13.0;
const CARD_TOP: f32 = 45.0;
const OPT_W: f32 = 175.0;
const OPT_H: f32 = 28.0;

fn rgb(dark: u32, light: u32) -> Color32 {
    kn_ui::theme::rgb(if kn_ui::theme::is_light() { light } else { dark })
}
fn dialog_bg() -> Color32 {
    rgb(0x262523, 0xececec)
}
fn card_bg() -> Color32 {
    rgb(0x3b3a38, 0xf6f6f6)
}
fn option_line() -> Color32 {
    rgb(0x464543, 0xd4d4d4)
}
fn option_kept() -> Color32 {
    rgb(0x43423f, 0xe4e4e4)
}
fn strong_text() -> Color32 {
    rgb(0xffffff, 0x1d1d1f)
}
/// The card's circle colour and its selection colour (the border and the chosen option).
fn card_colors(m: Main) -> (Color32, Color32) {
    match m {
        Main::Midi => (Color32::from_rgb(35, 153, 65), Color32::from_rgb(75, 151, 76)),
        Main::Pattern => (Color32::from_rgb(73, 71, 179), Color32::from_rgb(98, 96, 196)),
        Main::Player => (Color32::from_rgb(179, 149, 0), Color32::from_rgb(170, 144, 32)),
        Main::Audio => (Color32::from_rgb(8, 106, 204), Color32::from_rgb(44, 118, 210)),
    }
}

pub fn sheet(app: &mut App, ctx: &egui::Context) {
    if !app.new_track_open {
        return;
    }
    // (callers that set the kind directly: the dialog opens on it)
    if app.tm.nt.kind() != app.new_track_kind {
        let k = app.new_track_kind;
        app.tm.nt.set_kind(k);
        app.tm.nt.remembered[card(app.tm.nt.main)] = app.tm.nt.sub;
    }
    let mut d = app.tm.nt.clone();
    let mut create = false;
    let mut cancel = false;
    let mut help = false;
    let mut open_audio_settings = false;
    // an empty project needs a track: Cancel is dimmed (Logic's forced first track)
    let can_cancel = app.project.as_ref().is_some_and(|p| !p.tracks.is_empty());
    let popup = egui::Popup::is_any_open(ctx);
    // the arrow keys move between the cards and their options (the count field keeps the rest)
    if !popup {
        let (dx, dy) = ctx.input_mut(|i| {
            let mut k = |key| i.consume_key(egui::Modifiers::NONE, key);
            (k(egui::Key::ArrowRight) as i32 - k(egui::Key::ArrowLeft) as i32, k(egui::Key::ArrowDown) as i32 - k(egui::Key::ArrowUp) as i32)
        });
        d.arrow(dx, dy);
    }
    let radius = 16.0;
    egui::Window::new("Create New Track")
        .id(egui::Id::unique("new-track"))
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        // centred with Details closed; opening Details grows it downwards
        // (less the frame's 1-point outline)
        .fixed_pos(pos2(ctx.content_rect().center().x - W / 2.0 - 1.0, ctx.content_rect().center().y - 383.0 / 2.0 - 1.0))
        .frame(egui::Frame::NONE.fill(dialog_bg()).corner_radius(radius).stroke(Stroke::new(1.0, rgb(0x0d0d0d, 0xb8b8b8))).shadow(egui::Shadow { offset: [0, 10], blur: 40, spread: 0, color: Color32::from_black_alpha(110) }))
        .show(ctx, |ui| {
            ui.set_width(W);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let (r, _) = ui.allocate_exact_size(vec2(W, 321.5), Sense::hover());
            let p = ui.painter().clone();
            // the 1-point light edge along the top
            p.hline(r.min.x + radius..=r.max.x - radius, r.min.y + 0.5, Stroke::new(1.0, rgb(0x353432, 0xf8f8f8)));
            p.text(pos2(r.center().x, r.min.y + 13.75), Align2::CENTER_CENTER, "Create New Track", look::bold(13.0), rgb(0xdededd, 0x1d1d1f));
            for (i, (m, name)) in CARDS.into_iter().enumerate() {
                let cr = Rect::from_min_size(pos2(r.min.x + 21.5 + i as f32 * (CARD_W + CARD_GAP), r.min.y + CARD_TOP), vec2(CARD_W, CARD_H));
                let on = d.main == m;
                let (circle, sel) = card_colors(m);
                let resp = ui.interact(cr, ui.scope_id().with(("nt-card", i)), Sense::click());
                // the card: a dark hairline outside, a light one along its top, the colour border when chosen
                // (Apple's continuous 16-point corners reach as far as a 20-point circle's)
                let cr_r = 20.0;
                p.rect_filled(cr, cr_r, card_bg());
                if on {
                    p.rect_stroke(cr, cr_r, Stroke::new(2.0, sel), egui::StrokeKind::Inside);
                } else {
                    // a dark hairline outside, a light rim inside, brightest along the top and bottom
                    p.rect_stroke(cr, cr_r, Stroke::new(1.0, rgb(0x1c1b1a, 0xc6c6c6)), egui::StrokeKind::Outside);
                    p.rect_stroke(cr, cr_r, Stroke::new(1.0, rgb(0x42413f, 0xffffff)), egui::StrokeKind::Inside);
                    p.hline(cr.min.x + cr_r..=cr.max.x - cr_r, cr.min.y + 0.5, Stroke::new(1.0, rgb(0x625f5d, 0xffffff)));
                    p.hline(cr.min.x + cr_r..=cr.max.x - cr_r, cr.max.y - 0.5, Stroke::new(1.0, rgb(0x52514e, 0xffffff)));
                }
                let cc = pos2(cr.center().x, cr.min.y + 16.0 + 28.0);
                p.circle_filled(cc, 28.0, circle);
                card_icon(&p, Rect::from_center_size(cc, vec2(56.0, 56.0)), m);
                p.text(pos2(cr.center().x, cr.min.y + 90.75), Align2::CENTER_CENTER, name, look::bold(16.0), strong_text());
                if resp.clicked() {
                    d.choose_card(m);
                }
                if resp.double_clicked() {
                    create = true;
                }
                for (k, opt) in subs(m).iter().enumerate() {
                    let orr = Rect::from_min_size(pos2(cr.center().x - OPT_W / 2.0, cr.min.y + 112.0 + k as f32 * 34.0), vec2(OPT_W, OPT_H));
                    let o = ui.interact(orr, ui.scope_id().with(("nt-opt", i, k)), Sense::click());
                    let kept = d.remembered[i].min(subs(m).len() - 1) == k;
                    if on && d.sub == k {
                        p.rect_filled(orr, 6.0, sel);
                        p.text(orr.center() - vec2(0.0, 1.0), Align2::CENTER_CENTER, *opt, look::regular(13.0), Color32::WHITE);
                    } else {
                        if kept {
                            p.rect_filled(orr, 6.0, option_kept());
                        }
                        p.rect_stroke(orr, 6.0, Stroke::new(1.0, option_line()), egui::StrokeKind::Inside);
                        p.text(orr.center() - vec2(0.0, 1.0), Align2::CENTER_CENTER, *opt, look::regular(13.0), strong_text());
                    }
                    if o.clicked() {
                        d.choose(m, k);
                    }
                    if o.double_clicked() {
                        create = true;
                    }
                }
            }
            // › Details (the disclosure)
            let dr = Rect::from_min_max(pos2(r.min.x + 18.0, r.min.y + 301.5), pos2(r.min.x + 100.0, r.min.y + 321.5));
            let dresp = ui.interact(dr, ui.scope_id().with("nt-details"), Sense::click());
            let chev = Rect::from_center_size(pos2(r.min.x + 26.75, r.min.y + 311.5), vec2(12.0, 12.0));
            crate::glyphs::paint(&p, chev, if d.open { crate::glyphs::tb::CHEVRON_DOWN } else { crate::glyphs::tb::CHEVRON_RIGHT }, 15.0, strong_text());
            p.text(pos2(r.min.x + 39.0, r.min.y + 311.5), Align2::LEFT_CENTER, "Details", look::regular(13.0), strong_text());
            if dresp.clicked() {
                d.open = !d.open;
            }
            if d.open {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(39.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                        details_ui(app, ui, &mut d, &mut open_audio_settings);
                    });
                });
                ui.add_space(4.0);
            }
            // the bottom row
            let (b, _) = ui.allocate_exact_size(vec2(W, 61.5), Sense::hover());
            let cy = b.min.y + 31.5;
            let q = Rect::from_center_size(pos2(b.min.x + 32.0, cy), vec2(24.0, 24.0));
            let qr = ui.interact(q, ui.scope_id().with("nt-help"), Sense::click()).on_hover_text("Help");
            p.circle_filled(q.center(), 12.0, rgb(0x363533, 0xffffff));
            p.circle_stroke(q.center(), 12.0, Stroke::new(0.5, rgb(0x41403e, 0xc8c8c8)));
            p.text(q.center(), Align2::CENTER_CENTER, "?", look::regular(15.0), strong_text());
            help = qr.clicked();
            // Number of tracks to create: [ n ] (centred on the dialog, focused like Logic's)
            let field = Rect::from_center_size(pos2(b.center().x, cy), vec2(42.0, 22.0));
            p.text(pos2(field.min.x - 5.5, cy), Align2::RIGHT_CENTER, "Number of tracks to create:", look::regular(13.0), strong_text());
            p.rect_filled(field, 4.0, rgb(0x181818, 0xffffff));
            let text = d.count_text.get_or_insert_with(|| app.new_track_count.max(1).to_string());
            let fid = ui.scope_id().with("nt-count");
            // focused while nothing else is (selecting its text, as Logic opens it)
            if ui.memory(|m| m.focused().is_none()) && !popup {
                ui.memory_mut(|m| m.request_focus(fid));
                let n = text.chars().count();
                let mut st = egui::TextEdit::load_state(ui.ctx(), fid).unwrap_or_default();
                st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
                st.store(ui.ctx(), fid);
            }
            // (Logic's selected text in the field: a muted blue)
            ui.visuals_mut().selection.bg_fill = rgb(0x476288, 0xb3d1f7);
            let te = ui.put(field.shrink2(vec2(4.0, 2.0)).translate(vec2(0.0, -3.0)), egui::TextEdit::singleline(text).id(fid).frame(egui::Frame::NONE).font(look::regular(13.0)).text_color(strong_text()).align(egui::Align2::CENTER_CENTER).char_limit(2));
            if ui.memory(|m| m.has_focus(fid)) {
                p.rect_stroke(field.expand(0.5), 4.5, Stroke::new(3.5, rgb(0x3b6b98, 0x7fb1f0)), egui::StrokeKind::Outside);
            }
            if te.changed() {
                text.retain(|c| c.is_ascii_digit());
                if let Ok(n) = text.parse::<u32>() {
                    app.new_track_count = n.clamp(1, 64);
                }
            }
            // Cancel  Create
            let create_r = Rect::from_min_size(pos2(b.max.x - 20.0 - 72.0, cy - 12.0), vec2(72.0, 24.0));
            let cancel_r = create_r.translate(vec2(-82.0, 0.0));
            let cr = ui.interact(create_r, ui.scope_id().with("nt-create"), Sense::click());
            p.rect_filled(create_r, 6.0, if cr.is_pointer_button_down_on() { Color32::from_rgb(40, 100, 220) } else { Color32::from_rgb(52, 120, 246) });
            p.text(create_r.center(), Align2::CENTER_CENTER, "Create", look::regular(13.0), Color32::WHITE);
            let xr = ui.interact(cancel_r, ui.scope_id().with("nt-cancel"), if can_cancel { Sense::click() } else { Sense::hover() });
            p.rect_filled(cancel_r, 6.0, rgb(0x2e2d2b, 0xffffff));
            p.text(cancel_r.center(), Align2::CENTER_CENTER, "Cancel", look::regular(13.0), if can_cancel { strong_text() } else { rgb(0x62615f, 0xa8a8a8) });
            create |= cr.clicked();
            cancel |= xr.clicked();
            if !popup && ui.input(|i| i.key_pressed(egui::Key::Escape)) && can_cancel {
                cancel = true;
            }
            if !popup && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                create = true;
            }
        });
    app.new_track_kind = d.kind();
    if create || cancel {
        d.count_text = None;
    }
    app.tm.nt = d.clone();
    if open_audio_settings {
        app.settings_tab = "Audio".into();
        app.settings_sub = "Devices".into();
        app.show_settings = true;
    }
    if help {
        app.key_command(ctx, "Bramble Studio Help");
    }
    if create {
        app.new_track_open = false;
        let n = app.new_track_count.max(1) as usize;
        create_tracks(app, ctx, &d, n);
    } else if cancel {
        app.new_track_open = false;
    }
}

/// A Details row the Mac way: the label right-aligned in the first column, its controls after.
fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if !label.is_empty() {
            ui.label(egui::RichText::new(label).font(look::regular(13.0)));
        }
    });
    ui.horizontal(add);
    ui.end_row();
}

fn details_ui(app: &mut App, ui: &mut egui::Ui, d: &mut Details, open_audio_settings: &mut bool) {
    let mut search = std::mem::take(&mut d.search);
    egui::Grid::new("nt-details-grid").num_columns(2).spacing(vec2(8.0, 8.0)).show(ui, |ui| match d.main {
        Main::Audio => {
            row(ui, "Audio Input:", |ui| {
                if let Some(v) = searchable(ui, &d.input, &inputs(app), &mut search) {
                    d.input = v;
                }
                ui.checkbox(&mut d.input_ascending, "Ascending");
            });
            row(ui, "Audio Output:", |ui| {
                if let Some(v) = searchable(ui, &d.output, &outputs(app), &mut search) {
                    d.output = v;
                }
                ui.checkbox(&mut d.output_ascending, "Ascending");
            });
            // the devices in use, with an arrow to the Audio settings
            let dev = |key: &str| {
                let v = app.logic_settings.get(key);
                if v.is_empty() { "System Default".to_string() } else { v }
            };
            for (label, key) in [("Input Device:", "Audio / Devices / Input Device:"), ("Output Device:", "Audio / Devices / Output Device:")] {
                let name = dev(key);
                row(ui, label, |ui| {
                    ui.label(egui::RichText::new(name).color(look::text_dim()));
                    if ui.add(egui::Button::new("→").frame(false)).on_hover_text("Open Settings ▸ Audio ▸ Devices").clicked() {
                        *open_audio_settings = true;
                    }
                });
            }
            row(ui, "", |ui| {
                ui.checkbox(&mut d.load_default_patch, "Load Default Patch").on_hover_text("Off: an empty track with no plug-ins");
                ui.checkbox(&mut d.open_browser, "Open Browser");
            });
            row(ui, "", |ui| {
                ui.checkbox(&mut d.monitoring, "Input Monitoring");
                ui.checkbox(&mut d.record_enable, "Record Enable");
            });
        }
        Main::Midi | Main::Pattern if d.sub == 0 => {
            row(ui, "Instrument:", |ui| {
                if let Some(v) = searchable(ui, &d.instrument, &instruments(), &mut search) {
                    d.instrument = v;
                }
                ui.checkbox(&mut d.multi_timbral, "Multi-timbral");
                ui.add_enabled(d.multi_timbral, egui::DragValue::new(&mut d.parts).range(2..=16).suffix(" parts"));
            });
            row(ui, "Audio Output:", |ui| {
                if let Some(v) = searchable(ui, &d.output, &outputs(app), &mut search) {
                    d.output = v;
                }
                ui.checkbox(&mut d.output_ascending, "Ascending");
            });
            row(ui, "", |ui| {
                ui.checkbox(&mut d.open_browser, "Open Browser");
                if d.main == Main::Pattern {
                    ui.checkbox(&mut d.open_editor, "Open Editor");
                }
            });
        }
        Main::Midi | Main::Pattern => {
            // External MIDI (a Pattern track always plays through the External Instrument plug-in)
            row(ui, "", |ui| {
                let mut always = true;
                if d.main == Main::Pattern {
                    ui.add_enabled(false, egui::Checkbox::new(&mut always, "Use External Instrument plug-in"));
                } else {
                    ui.checkbox(&mut d.external_plugin, "Use External Instrument plug-in");
                }
            });
            row(ui, "MIDI Destination:", |ui| {
                if let Some(v) = searchable(ui, &d.midi_port, &midi_destinations(app), &mut search) {
                    d.midi_port = v;
                }
                ui.label("Channel:");
                ui.add(egui::DragValue::new(&mut d.midi_channel).range(1..=16));
                ui.checkbox(&mut d.midi_ascending, "Ascending").on_hover_text("Each new track takes the next MIDI channel");
            });
            if d.main == Main::Pattern {
                row(ui, "", |ui| {
                    ui.checkbox(&mut d.open_editor, "Open Editor");
                });
            }
        }
        Main::Player => {
            let list = styles(d.sub);
            row(ui, "Style:", |ui| {
                let items: Vec<(usize, String)> = list.into_iter().enumerate().collect();
                if let Some(v) = searchable(ui, &d.style, &items, &mut search) {
                    d.style = v;
                }
            });
            row(ui, "", |ui| {
                ui.checkbox(&mut d.default_chords, "Use Default Chord Progression for New Regions");
            });
            row(ui, "", |ui| {
                ui.checkbox(&mut d.open_editor, "Open Editor");
                ui.checkbox(&mut d.open_browser, "Open Browser");
            });
        }
    });
    d.search = search;
}

fn ascend_input(i: Input, k: usize) -> Input {
    match i {
        Input::Mono0 if k == 0 => Input::Mono0,
        Input::Mono0 => Input::Mono(k),
        Input::Mono(n) => Input::Mono(n + k),
        Input::Stereo(n) => Input::Stereo(n + 2 * k),
        Input::Bus(b) => Input::Bus(b + k),
        Input::None => Input::None,
    }
}

fn ascend_output(o: Out, k: usize) -> Out {
    match o {
        Out::Bus(b) => Out::Bus(b + k),
        o => o,
    }
}

fn push_track(app: &mut App, t: lg_core::Track, program: u8) -> usize {
    let Some(p) = app.project.as_mut() else { return 0 };
    p.tracks.push(t);
    app.programs.push(program);
    let i = p.tracks.len() - 1;
    crate::trackarea::sync(app);
    app.selected_track = Some(i);
    i
}

/// Creates `n` tracks as the Details say; the new tracks' indexes.
pub fn create_tracks(app: &mut App, ctx: &egui::Context, d: &Details, n: usize) -> Vec<usize> {
    let mut made = Vec::new();
    if app.project.is_none() {
        return made;
    }
    match d.main {
        Main::Player => {
            for _ in 0..n {
                match d.sub {
                    0 => {
                        app.new_drummer();
                        let t = app.selected_track.unwrap_or(0);
                        if let Some(c) = lg_players::character::DRUMMERS.get(d.style).filter(|_| d.style > 0) {
                            let seed = app.drum_params.get(&(t, 0)).map_or(1, |p| p.seed);
                            app.drum_params.insert((t, 0), crate::drummer::Params { seed, ..crate::drummer::Params::for_character(c) });
                            if let Some(r) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.first_mut()) {
                                r.name = format!("Drummer - {}", c.name);
                            }
                            crate::drummer::regenerate(app, t, 0);
                        }
                    }
                    k => {
                        app.spx.tonic_only = !d.default_chords;
                        crate::players::new_track(app, k == 2);
                        let t = app.selected_track.unwrap_or(0);
                        if let Some(pl) = app.players.get_mut(&(t, 0)) {
                            match pl {
                                crate::players::Player::Bass(b) => b.style = lg_players::bass::BassStyle::ALL[d.style.min(lg_players::bass::BassStyle::ALL.len() - 1)],
                                crate::players::Player::Keys(b) => b.style = lg_players::keys::KeysStyle::ALL[d.style.min(lg_players::keys::KeysStyle::ALL.len() - 1)],
                            }
                            let name = pl.region_name();
                            if let Some(r) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.first_mut()) {
                                r.name = name;
                            }
                            crate::players::regenerate(app, t, 0);
                        }
                    }
                }
                let t = app.selected_track.unwrap_or(0);
                if let Some(s) = crate::tm::set_mut(app, t) {
                    s.default_region = RegionType::SessionPlayer;
                }
                if !d.open_editor && app.bottom == crate::app::Bottom::PianoRoll {
                    app.bottom = crate::app::Bottom::None;
                }
                made.push(t);
            }
            app.after_edit();
        }
        Main::Audio => {
            app.checkpoint("New Tracks");
            for k in 0..n {
                let count = app.project.as_ref().map_or(0, |p| p.tracks.iter().filter(|t| t.kind == TrackKind::Audio).count());
                let name = if d.sub == 1 { format!("Guitar {}", k + 1) } else { format!("Audio {}", count + 1) };
                let t = push_track(app, lg_core::Track::new(&name, TrackKind::Audio), 0);
                app.mix.inputs.insert(t, if d.input_ascending { ascend_input(d.input, k) } else { d.input });
                let out = if d.output_ascending { ascend_output(d.output, k) } else { d.output };
                if out != Out::Stereo {
                    app.mix.outputs.insert(t, out);
                }
                if d.monitoring {
                    app.monitoring.insert(t);
                }
                if d.record_enable {
                    app.armed.insert(t);
                }
                if d.load_default_patch && !crate::trackops::load_default_patch(app, t) && d.sub == 1 {
                    // Guitar or Bass without a stored default: stompboxes and an amp
                    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                        tr.plugins.push(crate::mixstate::new_plugin("Pedalboard"));
                        tr.plugins.push(crate::mixstate::new_plugin("Amp Designer"));
                    }
                }
                made.push(t);
            }
            app.after_edit();
        }
        Main::Midi | Main::Pattern => {
            app.checkpoint("New Tracks");
            let pattern = d.main == Main::Pattern;
            for k in 0..n {
                let count = app.project.as_ref().map_or(0, |p| p.tracks.iter().filter(|t| t.kind != TrackKind::Audio).count());
                let mut nt = lg_core::Track::new(&format!("Inst {}", count + 1), TrackKind::Instrument);
                if pattern {
                    nt.extra.set.default_region = RegionType::Pattern;
                }
                if d.external() {
                    let ch = if d.midi_ascending { (d.midi_channel as usize - 1 + k) % 16 + 1 } else { d.midi_channel as usize } as u8;
                    nt.name = format!("MIDI {ch}");
                    nt.extra.set.external = Some(External { port: d.midi_port.clone(), channel: ch, use_plugin: pattern || d.external_plugin, ..Default::default() });
                    let t = push_track(app, nt, 0);
                    // (External Instrument loaded: the module's audio comes back on the strip)
                    if d.external_plugin && !pattern {
                        crate::utilwin::picked(app, t, Some("External Instrument"));
                    }
                    made.push(t);
                    continue;
                }
                let t = push_track(app, nt, 0);
                made.push(t);
                let out = if d.output_ascending { ascend_output(d.output, k) } else { d.output };
                if out != Out::Stereo {
                    app.mix.outputs.insert(t, out);
                }
                match &d.instrument {
                    Inst::Empty => {}
                    Inst::Plugin(name) => {
                        if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                            let mut u = crate::mixstate::new_plugin(name);
                            u.slot = 2;
                            tr.plugins.push(u);
                            tr.name = name.clone();
                        }
                    }
                    Inst::DefaultPatch => {
                        if !crate::trackops::load_default_patch(app, t) {
                            if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                                tr.name = "Deluxe Classic".into();
                            }
                            if let Some(pg) = app.programs.get_mut(t) {
                                *pg = 4;
                            }
                        }
                    }
                }
                // Multi-timbral: further tracks on the same channel strip, one MIDI channel each
                if d.multi_timbral {
                    crate::trackarea::sync(app);
                    let (id, name) = app.project.as_ref().map(|p| (p.tracks[t].extra.id, p.tracks[t].name.clone())).unwrap_or((0, String::new()));
                    if let Some(s) = crate::tm::set_mut(app, t) {
                        s.midi.out_channel = 1;
                    }
                    for part in 1..d.parts.max(2) {
                        let mut pt = lg_core::Track::new(&format!("{name} {}", part + 1), TrackKind::Instrument);
                        pt.extra.set.same_strip = Some(id);
                        pt.extra.set.midi.out_channel = part + 1;
                        pt.extra.set.default_region = if pattern { RegionType::Pattern } else { RegionType::Midi };
                        made.push(push_track(app, pt, 0));
                    }
                }
            }
            app.after_edit();
            if pattern {
                let at = app.position() as u32;
                for &t in &made {
                    crate::stepseq::new_pattern_region(app, ctx, t, at);
                }
                if d.open_editor {
                    if let Some(&t) = made.last() {
                        app.selected_track = Some(t);
                        app.selected_region = Some((t, 0));
                        app.key_command(ctx, "Open Step Sequencer");
                    }
                }
                app.after_edit();
            }
        }
    }
    if !made.is_empty() {
        app.selected_track = made.last().copied();
        app.ta.selected = made.clone();
    }
    if d.open_browser {
        app.show_browser = true;
    }
    made
}

/// The Add Tracks button with a modifier, a double-click under the headers, and their key
/// commands: an empty track of the focused track's type, of the opposite type, or one like
/// the selected track — with the default patch when New Tracks was last set to load it.
pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    let focused_audio = app.selected_track.and_then(|t| app.project.as_ref()?.tracks.get(t)).is_some_and(|t| t.kind == TrackKind::Audio);
    let audio = match name {
        "New Empty Track of Focused Type" | "New Track Like Selected" => focused_audio,
        "New Empty Track of Opposite Type" => !focused_audio,
        _ => return false,
    };
    let last = app.tm.nt.clone();
    let default_patch = last.load_default_patch && last.instrument == Inst::DefaultPatch;
    let d = Details { main: if audio { Main::Audio } else { Main::Midi }, sub: 0, load_default_patch: default_patch, instrument: if default_patch { Inst::DefaultPatch } else { Inst::Empty }, multi_timbral: false, open_browser: false, ..last };
    // under the selected track
    let before = app.selected_track;
    let made = create_tracks(app, ctx, &d, 1);
    if let (Some(sel), Some(&new)) = (before, made.first()) {
        let to = app.project.as_ref().map_or(new, |p| lg_core::arrange::group(&p.tracks, sel).end);
        if to < new {
            if let Some(p) = app.project.as_mut() {
                let t = p.tracks.remove(new);
                p.tracks.insert(to, t);
            }
            app.after_edit();
            app.selected_track = Some(to);
            app.ta.selected = vec![to];
        }
    }
    !made.is_empty()
}

/// Option-click (focused type) and Command-click (the other type) on the Add Tracks button: a
/// new track with the default patch, whatever New Tracks was last set to.
pub fn with_default_patch(app: &mut App, ctx: &egui::Context, opposite: bool) -> bool {
    let keep = app.tm.nt.clone();
    app.tm.nt.load_default_patch = true;
    app.tm.nt.instrument = Inst::DefaultPatch;
    let made = command(app, ctx, if opposite { "New Empty Track of Opposite Type" } else { "New Empty Track of Focused Type" });
    app.tm.nt = keep;
    made
}

/// The card's white icon on its colour circle.
fn card_icon(p: &egui::Painter, r: Rect, m: Main) {
    use crate::glyphs::{br, ph, ph_fill, tb};
    let white = Color32::WHITE;
    match m {
        Main::Midi => crate::glyphs::paint(p, r, ph_fill::MUSIC_NOTES, 35.0, white),
        // a rounded square holding a 3×3 grid of steps
        Main::Pattern => {
            crate::glyphs::paint(p, r, tb::SQUARE, 34.5, white);
            crate::glyphs::paint(p, r, ph_fill::GRID_NINE, 26.0, white);
        }
        Main::Player => crate::glyphs::paint(p, r, br::DRUM, 40.0, white),
        Main::Audio => crate::glyphs::paint(p, r, ph::WAVEFORM, 37.0, white),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_keep_their_choice_and_arrows_move() {
        let mut d = Details::default();
        assert_eq!((d.main, d.sub), (Main::Midi, 0));
        d.choose(Main::Player, 2);
        assert_eq!(d.kind(), Kind::Keys);
        d.choose_card(Main::Audio);
        assert_eq!((d.main, d.sub), (Main::Audio, 0));
        d.choose_card(Main::Player);
        assert_eq!(d.kind(), Kind::Keys, "the card keeps its choice");
        // left/right: the cards in Logic's order (MIDI, Pattern, Session Player, Audio)
        d.arrow(1, 0);
        assert_eq!(d.main, Main::Audio);
        d.arrow(1, 0);
        assert_eq!(d.main, Main::Audio, "stops at the last card");
        d.arrow(-3, 0);
        assert_eq!(d.main, Main::Midi);
        // up/down: the options
        d.arrow(0, 1);
        assert!(d.external());
        d.arrow(0, 1);
        assert_eq!(d.sub, 1, "stops at the last option");
        d.arrow(0, -1);
        assert_eq!(d.kind(), Kind::Instrument);
    }

    #[test]
    fn kinds_and_choices() {
        let mut d = Details::default();
        assert_eq!(d.kind(), Kind::Instrument);
        d.set_kind(Kind::Guitar);
        assert_eq!((d.main, d.sub, d.kind()), (Main::Audio, 1, Kind::Guitar));
        d.set_kind(Kind::Keys);
        assert_eq!((d.main, d.sub), (Main::Player, 2));
        assert_eq!(subs(Main::Audio), ["Mic or Line", "Guitar or Bass"]);
        assert_eq!(subs(Main::Midi), ["Software Instrument", "External MIDI"]);
        assert_eq!(subs(Main::Pattern), ["Software Instrument", "External MIDI"]);
        assert!(!styles(0).is_empty() && !styles(1).is_empty() && !styles(2).is_empty());
        assert_eq!(ascend_input(Input::Mono0, 2), Input::Mono(2));
        assert_eq!(ascend_input(Input::Stereo(0), 1), Input::Stereo(2));
        assert_eq!(ascend_output(Out::Bus(3), 2), Out::Bus(5));
        assert_eq!(ascend_output(Out::Stereo, 2), Out::Stereo);
    }

    #[test]
    fn audio_tracks_take_the_details() {
        let mut app = crate::tm::test_app();
        let ctx = egui::Context::default();
        let n = app.project.as_ref().unwrap().tracks.len();
        let d = Details { main: Main::Audio, input: Input::Mono(1), input_ascending: true, output: Out::Bus(2), output_ascending: true, monitoring: true, record_enable: true, load_default_patch: false, ..Default::default() };
        let made = create_tracks(&mut app, &ctx, &d, 3);
        assert_eq!(made, vec![n, n + 1, n + 2]);
        assert_eq!(app.mix.inputs[&(n + 2)], Input::Mono(3), "Ascending: the next input each");
        assert_eq!(app.mix.outputs[&(n + 1)], Out::Bus(3));
        assert!(app.monitoring.contains(&n) && app.armed.contains(&(n + 2)));
        let p = app.project.as_ref().unwrap();
        assert!(p.tracks[n].plugins.is_empty(), "no default patch: an empty track");
        assert_eq!(p.tracks[n].kind, TrackKind::Audio);
        app.undo();
        assert_eq!(app.project.as_ref().unwrap().tracks.len(), n);
    }

    #[test]
    fn external_midi_and_multi_timbral_tracks() {
        let mut app = crate::tm::test_app();
        let ctx = egui::Context::default();
        let n = app.project.as_ref().unwrap().tracks.len();
        let d = Details { main: Main::Midi, sub: 1, midi_channel: 15, midi_ascending: true, midi_port: "Synth".into(), ..Default::default() };
        let made = create_tracks(&mut app, &ctx, &d, 3);
        let p = app.project.as_ref().unwrap();
        let chans: Vec<u8> = made.iter().map(|&t| p.tracks[t].extra.set.external.as_ref().unwrap().channel).collect();
        assert_eq!(chans, vec![15, 16, 1]);
        assert_eq!(p.tracks[n].extra.set.external.as_ref().unwrap().port, "Synth");
        let d = Details { main: Main::Midi, sub: 0, multi_timbral: true, parts: 4, instrument: Inst::Plugin("Sampler".into()), ..Default::default() };
        let made = create_tracks(&mut app, &ctx, &d, 1);
        assert_eq!(made.len(), 4);
        let p = app.project.as_ref().unwrap();
        let owner = p.tracks[made[0]].extra.id;
        assert!(owner != 0 && made[1..].iter().all(|&t| p.tracks[t].extra.set.same_strip == Some(owner)));
        assert_eq!(p.tracks[made[3]].extra.set.midi.out_channel, 4);
        assert!(p.tracks[made[0]].plugins.iter().any(|u| u.slot == 2 && u.name == "Sampler"));
    }

    #[test]
    fn empty_track_of_the_opposite_type() {
        let mut app = crate::tm::test_app();
        let ctx = egui::Context::default();
        let t = app.project.as_ref().unwrap().tracks.iter().position(|x| x.kind != TrackKind::Audio).unwrap();
        app.selected_track = Some(t);
        assert!(command(&mut app, &ctx, "New Empty Track of Opposite Type"));
        let s = app.selected_track.unwrap();
        assert_eq!(app.project.as_ref().unwrap().tracks[s].kind, TrackKind::Audio);
        assert_eq!(s, t + 1, "under the selected track");
    }
}
