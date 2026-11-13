//! The control bar (Logic Pro 13, top of ui-tracks-with-pianoroll.png), left to right:
//! - views: Browser, Inspector, Quick Help, Toolbar | Editors, Smart Controls, Mixer
//! - transport: Stop, Play, Record, Capture as Recording, Replace, Cycle
//! - the LCD (bar, beat, tempo, signature, key) with its ⌄ display menu
//! - modes: Low Latency, Tuner, Solo | Count-in, Metronome (purple when on)
//! - the master volume slider
//! - right panes: List Editors, Notes, Loop Browser, Browsers
//! The buttons sit in the suite's capsules (kn_ui::capsule: liquid glass when it's on, the
//! classic strip, or the dark rimmed capsules), as Keydeck's, Letters' and Notes' toolbars.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::{App, Bottom, RightPane};
use crate::glyphs::{br, ph, ph_fill, tb};
use crate::look;

/// Logic's control-bar buttons: 38 pt apart (win-tracks-1-instrument-track.png at 2x).
const BTN: f32 = 38.0;
const H: f32 = 38.0;
const PAD: f32 = 3.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LcdMode {
    /// Bar, beat, tempo, signature and key (Logic's default "Beats & Project").
    BeatsProject,
    /// Hours:minutes:seconds and bar.beat.division.tick.
    BeatsTime,
    /// The musical position alone (lcdcustom.rs).
    Beats,
    Time,
    /// Columns ticked in Customize Control Bar and Display (lcdcustom.rs).
    Custom,
}

/// The ⌄ menu's modes, in Logic's order.
pub const LCD_MODES: [(LcdMode, &str); 5] = [(LcdMode::BeatsProject, "Beats & Project"), (LcdMode::BeatsTime, "Beats & Time"), (LcdMode::Beats, "Beats"), (LcdMode::Time, "Time"), (LcdMode::Custom, "Custom")];

/// A button in a capsule: hover and lit cells as the suite's grouped buttons.
fn button(ui: &mut egui::Ui, r: Rect, id: &str, on: bool, enabled: bool, tip: &str, draw: impl FnOnce(&egui::Painter, Rect, Color32)) -> egui::Response {
    let resp = ui.interact(r, ui.scope_id().with(("cb", id)), if enabled { Sense::click() } else { Sense::hover() }).on_hover_text(tip);
    let p = ui.painter();
    let cell = r.shrink2(vec2(1.0, 2.0));
    if on {
        p.rect_filled(cell, cell.height() / 2.0, look::button_on());
    } else if resp.hovered() && enabled {
        p.rect_filled(cell, cell.height() / 2.0, look::button_on().gamma_multiply(0.55));
    }
    let col = if enabled { look::text() } else { look::text_dim().gamma_multiply(0.6) };
    draw(p, r, col);
    resp
}

/// Lays out a run of buttons in one capsule from `x`; returns their rects and the capsule.
fn run(x: f32, cy: f32, widths: &[f32]) -> (Vec<Rect>, Rect) {
    let mut bx = x + PAD;
    let mut out = Vec::new();
    for w in widths {
        out.push(Rect::from_min_size(pos2(bx, cy - H / 2.0 + 2.0), vec2(*w, H - 4.0)));
        bx += w;
    }
    (out, Rect::from_min_max(pos2(x, cy - H / 2.0), pos2(bx + PAD, cy + H / 2.0)))
}

pub fn show(app: &mut App, ui: &mut egui::Ui, bar: Rect) {
    let p = ui.painter().clone();
    p.rect_filled(bar, 0.0, look::bar_bg());
    p.hline(bar.x_range(), bar.max.y, Stroke::new(1.0, look::line()));
    // the capsules go under the buttons: reserve their place now, fill it at the end
    let caps_idx = p.add(egui::Shape::Noop);
    let mut caps: Vec<Rect> = Vec::new();
    let cy = bar.center().y;
    let narrow = bar.width() < 1180.0;
    let mut x = bar.min.x + 10.0;

    // ---- views
    let (b, c) = run(x, cy, &[BTN; 4]);
    caps.push(c);
    if button(ui, b[0], "browser", app.show_browser, true, "Browser (B)", |p, r, c| crate::cbicons::browser(p, r, c, if app.show_browser { look::button_on() } else { look::capsule_fill() })).clicked() {
        app.show_browser = !app.show_browser;
    }
    if button(ui, b[1], "inspector", app.show_inspector, true, "Inspector (I)", |p, r, c| crate::cbicons::inspector(p, r, c, if app.show_inspector { look::button_on() } else { look::capsule_fill() })).clicked() {
        app.show_inspector = !app.show_inspector;
    }
    if button(ui, b[2], "quick-help", app.quick_help, true, "Quick Help (⇧/)", |p, r, c| icon(p, r, if app.quick_help { ph_fill::QUESTION } else { ph::QUESTION }, c)).clicked() {
        app.quick_help = !app.quick_help;
    }
    if button(ui, b[3], "toolbar", app.show_toolbar, true, "Toolbar (⌃⌥⌘T)", |p, r, c| icon(p, r, if app.show_toolbar { ph_fill::TRAY_ARROW_DOWN } else { ph::TRAY_ARROW_DOWN }, c)).clicked() {
        app.show_toolbar = !app.show_toolbar;
    }
    x = c.max.x + 8.0;
    let (b, c) = run(x, cy, &[BTN; 3]);
    caps.push(c);
    let editors_on = matches!(app.bottom, Bottom::PianoRoll);
    if button(ui, b[0], "editors", editors_on, true, "Editors (E)", |p, r, c| crate::cbicons::pencil(p, r, c)).clicked() {
        app.bottom = if editors_on { Bottom::None } else { Bottom::PianoRoll };
    }
    let smart_on = app.bottom == Bottom::SmartControls;
    if button(ui, b[1], "smart", smart_on, true, "Smart Controls (B)", |p, r, c| crate::cbicons::smart_controls(p, r, c)).clicked() {
        app.bottom = if smart_on { Bottom::None } else { Bottom::SmartControls };
    }
    let mixer_on = app.bottom == Bottom::Mixer;
    if button(ui, b[2], "mixer", mixer_on, true, "Mixer (X)", |p, r, c| icon(p, r, ph::FADERS, c)).clicked() {
        app.bottom = if mixer_on { Bottom::None } else { Bottom::Mixer };
    }
    let left_end = c.max.x;

    // ---- the LCD, centred; the transport before it
    let lcd_w = crate::lcdcustom::width(app);
    // (Logic's default transport: Go to Beginning / Stop, Play, Record, Cycle; Capture as
    // Recording, Replace and Autopunch only once ticked in Customize Control Bar)
    let extras = [app.cb_show.capture, app.cb_show.replace, app.cb_show.autopunch];
    let optional = if narrow { Vec::new() } else { crate::transportx::shown(app) };
    let tw = if narrow { 4 } else { 4 + extras.iter().filter(|&&on| on).count() + optional.len() };
    let widths = vec![BTN + 6.0; tw];
    let total_t = widths.iter().sum::<f32>() + 2.0 * PAD;
    // (as Logic: the LCD in the middle of the window, the transport to its left; pushed right
    // only when the window is too narrow for the buttons before it)
    let center = bar.center().x.max(left_end + 12.0 + total_t + 12.0 + lcd_w / 2.0);
    let lcd = Rect::from_center_size(pos2(center, cy), vec2(lcd_w, 50.0));
    let (b, c) = run(lcd.min.x - total_t - 12.0, cy, &widths);
    caps.push(c);
    let playing = app.playing();
    // (stopped, Logic's Stop button shows Go to Beginning: a bar and a triangle)
    let stop = button(ui, b[0], "stop", false, true, if playing { "Stop (Space)" } else { "Go to Beginning (Return)" }, |p, r, c| {
        // (Logic's Stop is a square whether playing or not; stopped, a click goes to the start)
        icon(p, r, ph_fill::STOP, c);
    });
    if stop.clicked() {
        app.stop();
    }
    stop.context_menu(|ui| crate::stopbutton::context_menu(app, ui));
    if button(ui, b[1], "play", playing, true, "Play (Space)", |p, r, c| {
        icon(p, r, ph_fill::PLAY, if playing { Color32::from_rgb(0x4c, 0xd9, 0x64) } else { c });
    })
    .clicked()
    {
        app.play_stop();
    }
    if button(ui, b[2], "record", app.recording(), true, "Record (R)", |p, r, _| {
        icon(p, r, ph_fill::CIRCLE, look::record_red());
    })
    .clicked()
    {
        app.key_command(ui.ctx(), "Record");
    }
    let mut cyc_i = 3;
    if !narrow {
        let mut slot = 3;
        if app.cb_show.capture && button(ui, b[slot], "capture", false, true, "Capture as Recording (⇧R)", |p, r, _| {
            crate::cbicons::capture(p, r, look::record_red());
        })
        .clicked()
        {
            app.key_command(ui.ctx(), "Capture as Recording");
        }
        slot += usize::from(app.cb_show.capture);
        if app.cb_show.replace && button(ui, b[slot], "replace", app.replace_mode, true, "Replace", |p, r, c| {
            icon(p, r, tb::REPLACE, if app.replace_mode { look::record_red() } else { c });
        })
        .clicked()
        {
            app.replace_mode = !app.replace_mode;
        }
        slot += usize::from(app.cb_show.replace);
        // Autopunch (its locators: the red strip in the ruler)
        let ap = app.autopunch;
        if app.cb_show.autopunch && button(ui, b[slot], "autopunch", ap, true, "Autopunch (⌃⌥⌘P)", |p, r, c| {
            icon(p, r, tb::BRACKETS_CONTAIN, if ap { look::record_red() } else { c });
        })
        .clicked()
        {
            app.autopunch = !app.autopunch;
        }
        slot += usize::from(app.cb_show.autopunch);
        // the optional transport buttons (transportx.rs)
        for (name, glyph, cmd) in &optional {
            let resp = button(ui, b[slot], name, false, true, name, |p, r, c| icon(p, r, glyph, c));
            let resp = resp.interact(Sense::click_and_drag());
            crate::transportx::button(app, ui.ctx(), name, cmd, &resp);
            slot += 1;
        }
        cyc_i = slot;
    }
    let cycle_on = app.cycle_on();
    if button(ui, b[cyc_i], "cycle", cycle_on, true, "Cycle (C)", |p, r, c| if cycle_on { icon(p, r, ph_fill::REPEAT, look::cycle()) } else { icon(p, r, ph::REPEAT, c) }).clicked() {
        app.toggle_cycle();
    }

    // ---- the LCD
    lcd_show(app, ui, &p, lcd);

    // ---- modes: Low Latency, Tuner, Solo | Count-in, Metronome
    x = lcd.max.x + 12.0;
    let right_w = 4.0 * BTN + 2.0 * PAD;
    // the Sync button (Customize Control Bar ▸ Modes and Functions ▸ Sync)
    if app.hub.sync.show_button {
        let (b, c) = run(x, cy, &[BTN]);
        caps.push(c);
        crate::syncmidi::sync_button(app, ui, b[0]);
        x = c.max.x + 8.0;
    }
    // Varispeed (Customize Control Bar ▸ Modes and Functions; varispeed.rs)
    if crate::varispeed::shown(ui.ctx()) {
        let c = crate::varispeed::control_bar(app, ui, x, cy, H);
        caps.push(c);
        x = c.max.x + 8.0;
    }
    // (as the bar narrows, the master volume leaves first, then Low Latency / Tuner / Solo,
    // then Count-in / Metronome; barfit.rs)
    let room = bar.max.x - 10.0 - right_w - x;
    let fits = crate::barfit::fit(
        &[crate::barfit::Slot { w: 3.0 * BTN + 2.0 * PAD + 8.0, drop: 2 }, crate::barfit::Slot { w: 2.0 * BTN + 2.0 * PAD + 12.0, drop: 1 }, crate::barfit::Slot { w: 74.0, drop: 3 }],
        room,
        0.0,
    );
    if fits[0] {
        let (b, c) = run(x, cy, &[BTN; 3]);
        caps.push(c);
        if button(ui, b[0], "low-latency", app.low_latency, true, "Low Latency Mode", |p, r, c| crate::cbicons::low_latency(p, r, if app.low_latency { look::accent() } else { c })).clicked() {
            app.low_latency = !app.low_latency;
        }
        if button(ui, b[1], "tuner", app.show_tuner, true, "Tuner", |p, r, c| icon(p, r, br::TUNING_FORK, c)).clicked() {
            app.show_tuner = !app.show_tuner;
        }
        if button(ui, b[2], "solo", app.solo_mode, true, "Solo (⌃S)", |p, r, c| {
            icon(p, r, tb::SQUARE_ROUNDED_LETTER_S, if app.solo_mode { look::cycle() } else { c });
        })
        .clicked()
        {
            // Option-click: Set Solo Lock Mode (regsolo.rs)
            let alt = ui.input(|i| i.modifiers.alt);
            crate::regsolo::button(app, alt);
        }
        x = c.max.x + 8.0;
    }
    if fits[1] {
        let (b, c) = run(x, cy, &[BTN; 2]);
        caps.push(c);
        // (Logic draws these two lit purple)
        let purple = Color32::from_rgb(0x8e, 0x3c, 0xe6);
        for (i, (id, tip)) in [("count-in", "Count-in"), ("metronome", "Metronome (K)")].into_iter().enumerate() {
            let on = if i == 0 { app.count_in } else { app.engine.lock().map(|e| e.metronome).unwrap_or(false) };
            let resp = ui.interact(b[i], ui.scope_id().with(("cb", id)), Sense::click()).on_hover_text(tip);
            let cell = b[i].shrink2(vec2(1.0, 2.0));
            if on {
                p.rect_filled(cell, cell.height() / 2.0, purple);
            } else if resp.hovered() {
                p.rect_filled(cell, cell.height() / 2.0, look::button_on().gamma_multiply(0.55));
            }
            // (clicking only while recording: grey with a purple symbol)
            let rec_only = i == 1 && crate::metroset::recording_only(app, on);
            if rec_only {
                p.rect_filled(cell, cell.height() / 2.0, look::button_on());
            }
            let col = if on { Color32::WHITE } else if rec_only { purple } else { look::text() };
            if i == 1 {
                crate::metroset::menu(app, &resp);
            }
            if i == 0 {
                count_in_mark(&p, b[i].center(), col);
            } else {
                // playing with the click on: the arm swings, one swing per beat (cbicons.rs);
                // repainting only while it does
                let swing = on && app.playing();
                let angle = if swing {
                    ui.ctx().request_repaint();
                    let den = app.project.as_ref().map_or(4, |pr| pr.signature.1 as u32);
                    crate::cbicons::metronome_angle(crate::cbicons::beats_at(app.position(), den))
                } else {
                    0.0
                };
                crate::cbicons::metronome(&p, b[i], 19.0, col, angle);
            }
            if resp.clicked() {
                if i == 0 {
                    app.count_in = !app.count_in;
                } else {
                    app.key_command(ui.ctx(), "Toggle Metronome Click");
                }
            }
        }
        x = c.max.x + 12.0;
    }

    // ---- the master volume (Logic's black bar), then the right panes
    let rg_x = bar.max.x - 10.0 - right_w;
    let vol = Rect::from_min_max(pos2(x, cy - 10.0), pos2(rg_x - 14.0, cy + 10.0));
    if fits[2] && vol.width() > 60.0 {
        master_volume(app, ui, vol);
    }
    let (b, c) = run(rg_x, cy, &[BTN; 4]);
    caps.push(c);
    let panes = [(RightPane::Lists, "List Editors (D)"), (RightPane::Notes, "Notes (⌥⌘P)"), (RightPane::Loops, "Loop Browser (O)"), (RightPane::Files, "Browsers (F)")];
    for (i, (pane, tip)) in panes.into_iter().enumerate() {
        let on = app.right_pane == Some(pane);
        if button(ui, b[i], tip, on, true, tip, |p, r, c| match pane {
            RightPane::Lists => icon(p, r, ph::LIST_BULLETS, c),
            RightPane::Notes => icon(p, r, ph::ARTICLE, c),
            RightPane::Loops => icon(p, r, ph::ARROW_CLOCKWISE, c),
            RightPane::Files => crate::cbicons::files(p, r, c),
        })
        .clicked()
        {
            app.right_pane = if on { None } else { Some(pane) };
        }
    }
    p.set(caps_idx, egui::Shape::Vec(kn_ui::capsule::shapes(ui.ctx(), &caps, &|c| kn_ui::capsule::dark_rimmed(c, look::capsule_fill()))));
}

/// The LCD: Beats & Project (bar, beat, tempo, signature/key), or with time, as the ⌄ menu picks.
fn lcd_show(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, lcd: Rect) {
    p.rect_filled(lcd, 10.0, look::lcd_bg());
    let menu_r = Rect::from_min_max(pos2(lcd.max.x - 22.0, lcd.min.y), lcd.max);
    let lcd = Rect::from_min_max(lcd.min, pos2(menu_r.min.x, lcd.max.y));
    let pos = app.position();
    let bar_t = app.bar_ticks() as f64;
    let beat_t = lg_core::events::PPQ as f64 * 4.0 / app.project.as_ref().map_or(4.0, |pr| pr.signature.1 as f64);
    let bar_n = (pos / bar_t).floor() as i64 + 1;
    let beat_n = (pos.rem_euclid(bar_t) / beat_t).floor() as i64 + 1;
    let big = look::regular(30.0);
    match app.lcd_mode {
        LcdMode::BeatsProject => {
            let col_w = lcd.width() / 4.0;
            let cell = |i: f32| Rect::from_min_size(pos2(lcd.min.x + i * col_w, lcd.min.y), vec2(col_w, lcd.height()));
            // bar: dimmed leading zeros, as Logic's "001"
            let bar_text = format!("{bar_n}");
            let lead = "0".repeat(3usize.saturating_sub(bar_text.len()));
            let c0 = cell(0.0);
            let g_lead = p.layout_no_wrap(lead, big.clone(), look::lcd_dim().gamma_multiply(0.45));
            let g_bar = p.layout_no_wrap(bar_text, big.clone(), look::lcd_text());
            let total = g_lead.size().x + g_bar.size().x;
            let x0 = c0.center().x - total / 2.0;
            let lead_w = g_lead.size().x;
            p.galley(pos2(x0, c0.min.y + 4.0), g_lead, Color32::WHITE);
            p.galley(pos2(x0 + lead_w, c0.min.y + 4.0), g_bar, Color32::WHITE);
            p.text(pos2(c0.center().x, c0.max.y - 8.0), Align2::CENTER_CENTER, "BAR", look::semibold(9.0), look::lcd_dim());
            let c1 = cell(1.0);
            p.text(pos2(c1.center().x, c1.min.y + 22.0), Align2::CENTER_CENTER, format!("{beat_n}"), big, look::lcd_text());
            p.text(pos2(c1.center().x, c1.max.y - 8.0), Align2::CENTER_CENTER, "BEAT", look::semibold(9.0), look::lcd_dim());
            tempo_cell(app, ui, p, cell(2.0));
            sig_key_cell(app, ui, p, cell(3.0));
        }
        LcdMode::Beats => crate::lcdcustom::beats(app, p, lcd),
        LcdMode::Custom => crate::lcdcustom::custom(app, ui, p, lcd),
        LcdMode::BeatsTime | LcdMode::Time => {
            let secs = app.seconds_at(pos);
            let h = (secs / 3600.0).floor();
            let m = ((secs % 3600.0) / 60.0).floor();
            let s = secs % 60.0;
            let _ = (h, m, s);
            // (Settings ▸ View ▸ Display Time As, the project's frame rate and SMPTE start)
            let time = crate::prefs::time_text(&app.logic_settings, &app.proj, secs);
            let div_t = beat_t / 4.0;
            let div = (pos.rem_euclid(beat_t) / div_t).floor() as i64 + 1;
            let tick = pos.rem_euclid(div_t).floor() as i64 + 1;
            let half = Rect::from_min_size(lcd.min, vec2(lcd.width() * 0.62, lcd.height()));
            let beats = crate::prefs::beats_text(&app.logic_settings, bar_n as i64, beat_n as i64, div, tick);
            let (top, bottom) = if app.lcd_mode == LcdMode::Time { (time, beats) } else { (beats, time) };
            p.text(pos2(half.center().x, half.min.y + 17.0), Align2::CENTER_CENTER, top, look::regular(19.0), look::lcd_text());
            p.text(pos2(half.center().x, half.min.y + 37.0), Align2::CENTER_CENTER, bottom, look::regular(13.0), look::lcd_dim());
            let rest = Rect::from_min_max(pos2(half.max.x, lcd.min.y), lcd.max);
            let w = rest.width() / 2.0;
            tempo_cell(app, ui, p, Rect::from_min_size(rest.min, vec2(w, rest.height())));
            sig_key_cell(app, ui, p, Rect::from_min_size(rest.min + vec2(w, 0.0), vec2(w, rest.height())));
        }
    }
    // ⌄: the display menu
    let resp = ui.interact(menu_r, ui.scope_id().with("lcd-menu"), Sense::click());
    let m = menu_r.center();
    let col = if resp.hovered() { look::lcd_text() } else { look::lcd_dim() };
    crate::glyphs::paint(p, Rect::from_center_size(m, vec2(14.0, 14.0)), ph::CARET_DOWN, 13.0, col);
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(220.0);
        for (mode, label) in LCD_MODES {
            if ui.add(egui::Button::selectable(app.lcd_mode == mode, label)).clicked() {
                app.lcd_mode = mode;
                ui.close();
            }
        }
        ui.separator();
        crate::transportx::lcd_menu(app, ui);
        if ui.button("Customize Control Bar and Display…").clicked() {
            app.bars.customize_bar = true;
            ui.close();
        }
    });
}

/// The tempo: drag up or down to change it (Shift: in hundredths), double-click to type it.
fn tempo_cell(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, c2: Rect) {
    let tempo = app.tempo();
    // (Settings ▸ View ▸ Display Tempo As: transportx.rs)
    let tempo_text = crate::transportx::tempo_text(&app.logic_settings, crate::projsettings::smpte(&app.proj).1, tempo);
    let tr = Rect::from_center_size(pos2(c2.center().x, c2.min.y + 15.0), vec2(c2.width() - 8.0, 22.0));
    if let Some(text) = app.tempo_edit.as_mut() {
        let te = ui.put(tr, egui::TextEdit::singleline(text).align(egui::Align2::CENTER_CENTER).font(look::regular(15.0)));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() {
            // Return or a click elsewhere sets it; Escape leaves the tempo as it was
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if let (false, Ok(v)) = (cancel, text.trim().parse::<f64>()) {
                app.checkpoint("Change Tempo");
                app.set_tempo(v.clamp(5.0, 990.0));
            }
            app.tempo_edit = None;
        }
    } else {
        p.text(tr.center(), Align2::CENTER_CENTER, tempo_text, look::regular(17.0), look::lcd_text());
        let resp = ui.interact(tr, ui.scope_id().with("lcd-tempo"), Sense::click_and_drag()).on_hover_text("Tempo: drag up or down, double-click to type");
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if resp.drag_started() {
            app.checkpoint("Change Tempo");
        }
        if resp.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let d = -resp.drag_delta().y as f64 * if fine { 0.01 } else { 0.25 };
            let t = app.project.as_ref().map_or(120.0, |pr| pr.tempo);
            app.set_tempo(if fine { t + d } else { (t + d).round() });
        }
        if resp.double_clicked() {
            app.tempo_edit = Some(format!("{tempo:.2}").trim_end_matches('0').trim_end_matches('.').to_string());
        }
    }
    crate::tempomode::lcd_label(app, ui, &p, pos2(c2.center().x, c2.min.y + 30.0));
    p.text(pos2(c2.center().x, c2.max.y - 8.0), Align2::CENTER_CENTER, "TEMPO", look::semibold(9.0), look::lcd_dim());
}

/// Signature over key; click either for its menu (Logic's pop-ups).
fn sig_key_cell(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, c3: Rect) {
    let (sig, key) = app.project.as_ref().map_or(((4, 4), "C major".to_string()), |pr| (pr.signature, pr.key.clone()));
    let sr = Rect::from_center_size(pos2(c3.center().x, c3.min.y + 15.0), vec2(c3.width() - 6.0, 18.0));
    let kr = Rect::from_center_size(pos2(c3.center().x, c3.min.y + 35.0), vec2(c3.width() - 6.0, 18.0));
    p.text(sr.center(), Align2::CENTER_CENTER, format!("{}/{}", sig.0, sig.1), look::regular(17.0), look::lcd_text());
    p.text(kr.center(), Align2::CENTER_CENTER, short_key(&key), look::regular(15.0), look::lcd_text());
    let s = ui.interact(sr, ui.scope_id().with("lcd-sig"), Sense::click()).on_hover_text("Time signature");
    egui::Popup::menu(&s).show(|ui| {
        for (n, d) in [(2u8, 4u8), (3, 4), (4, 4), (5, 4), (6, 8), (7, 8), (9, 8), (12, 8), (2, 2)] {
            if ui.add(egui::Button::selectable(sig == (n, d), format!("{n}/{d}"))).clicked() {
                app.checkpoint("Change Signature");
                if let Some(pr) = app.project.as_mut() {
                    pr.signature = (n, d);
                }
                app.after_edit();
                ui.close();
            }
        }
    });
    let k = ui.interact(kr, ui.scope_id().with("lcd-key"), Sense::click()).on_hover_text("Key");
    egui::Popup::menu(&k).show(|ui| {
        ui.set_min_width(140.0);
        for minor in [false, true] {
            ui.menu_button(if minor { "Minor" } else { "Major" }, |ui| {
                for root in ["C", "C♯", "D♭", "D", "E♭", "E", "F", "F♯", "G♭", "G", "A♭", "A", "B♭", "B"] {
                    let name = format!("{root} {}", if minor { "minor" } else { "major" });
                    if ui.add(egui::Button::selectable(key == name, &name)).clicked() {
                        app.checkpoint("Change Key");
                        if let Some(pr) = app.project.as_mut() {
                            pr.key = name.clone();
                        }
                        app.after_edit();
                        ui.close();
                    }
                }
            });
        }
    });
}

/// The master volume slider (Logic's dark bar with a round knob; double-click: 0 dB).
fn master_volume(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    let p = ui.painter();
    let track = Rect::from_center_size(r.center(), vec2(r.width(), 18.0));
    p.rect_filled(track, 9.0, look::lcd_bg());
    let level = app.engine.lock().map(|e| e.master_meter).unwrap_or(0.0).clamp(0.0, 1.0);
    let inner = track.shrink2(vec2(9.0, 6.0));
    // the output level glows behind the knob's track
    let w = inner.width() * level.sqrt();
    if w > 0.5 {
        p.rect_filled(Rect::from_min_size(inner.min, vec2(w, inner.height())), 3.0, if level > 0.95 { look::record_red() } else { Color32::from_rgb(0x4c, 0xd9, 0x64).gamma_multiply(0.75) });
    }
    let gain = app.engine.lock().map(|e| e.master).unwrap_or(1.0);
    let db = lg_dsp::gain_to_db(gain).clamp(-60.0, 6.0);
    let frac = crate::tracks::db_to_frac(db);
    let kx = inner.min.x + inner.width() * frac;
    let resp = ui.interact(track, ui.scope_id().with("master-vol"), Sense::click_and_drag()).on_hover_text(format!("Master Volume: {db:.1} dB"));
    p.circle_filled(pos2(kx, track.center().y), 7.0, Color32::from_gray(if resp.hovered() || resp.dragged() { 0xe0 } else { 0xb8 }));
    let set = |app: &mut App, db: f32| {
        if let Ok(mut e) = app.engine.lock() {
            e.master = lg_dsp::db_to_gain(db);
        }
    };
    if resp.double_clicked() {
        set(app, 0.0);
    } else if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
        let f = ((pp.x - inner.min.x) / inner.width()).clamp(0.0, 1.0);
        set(app, crate::tracks::frac_to_db(f));
        app.help_tag = Some(vec![("Master Volume".into(), format!("{:.1} dB", crate::tracks::frac_to_db(f)))]);
    }
}

pub(crate) fn short_key(k: &str) -> String {
    // "C major" -> "Cmaj", "G minor" -> "Gmin"
    let mut it = k.split_whitespace();
    let root = it.next().unwrap_or("C");
    let q = it.next().unwrap_or("major");
    format!("{root}{}", if q.starts_with("min") { "min" } else { "maj" })
}

/// A control-bar icon (Bramble Icons), centred in its button.
fn icon(p: &egui::Painter, r: Rect, icon: &str, c: Color32) {
    crate::glyphs::paint(p, r, icon, 19.0, c);
}

/// The transport's optional buttons (Customize Control Bar and Display…); Logic shows none of
/// them by default.
#[derive(Clone, Copy, Debug, Default)]
pub struct Shown {
    pub capture: bool,
    pub replace: bool,
    pub autopunch: bool,
}

/// Count-in's mark as Logic's: "1234" with each digit larger than the one before, on one
/// baseline, in the regular weight.
fn count_in_mark(p: &egui::Painter, c: egui::Pos2, col: Color32) {
    let sizes = [6.5f32, 8.5, 10.5, 12.5];
    let galleys: Vec<_> = ["1", "2", "3", "4"].iter().zip(sizes).map(|(d, s)| p.layout_no_wrap(d.to_string(), look::regular(s), col)).collect();
    let width: f32 = galleys.iter().map(|g| g.size().x).sum::<f32>() - 0.3 * 3.0;
    // the baseline: the largest digit's cap height centred on the button
    let base = c.y + 12.5 * 0.36;
    let mut x = c.x - width / 2.0;
    for (g, s) in galleys.into_iter().zip(sizes) {
        // (a digit's top sits about 0.95 of its size above the baseline in the UI font)
        let w = g.size().x;
        p.galley(egui::pos2(x, base - s * 0.95), g, col);
        x += w - 0.3;
    }
}
