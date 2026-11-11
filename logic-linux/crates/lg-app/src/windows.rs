//! Studio's small windows: the Go To Position / Set Locator / Marker Number sheets, the
//! screenset sheets (Duplicate…, Rename…), the Tuner, the About window and Quick Help.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::look;
use crate::nav::Ask;

// ---------------------------------------------------------------------------------------------
// the tuner's pure part

/// The pitch of `x` (mono at `rate`), Hz: the normalised autocorrelation's first strong peak
/// (YIN's difference function with its cumulative mean), 40 Hz to 2 kHz; None when it's quiet or
/// has no clear pitch.
pub fn pitch(x: &[f32], rate: f64) -> Option<f64> {
    let n = x.len();
    if n < 1024 || x.iter().fold(0f32, |m, v| m.max(v.abs())) < 0.01 {
        return None;
    }
    let min_lag = (rate / 2000.0) as usize;
    let max_lag = ((rate / 40.0) as usize).min(n / 2);
    let w = n - max_lag;
    let mut d = vec![0f64; max_lag + 1];
    for (lag, dl) in d.iter_mut().enumerate().skip(1) {
        let mut s = 0f64;
        for i in 0..w {
            let e = (x[i] - x[i + lag]) as f64;
            s += e * e;
        }
        *dl = s;
    }
    // the cumulative mean normalised difference
    let mut sum = 0f64;
    let mut cm = vec![1f64; max_lag + 1];
    for lag in 1..=max_lag {
        sum += d[lag];
        cm[lag] = if sum > 0.0 { d[lag] * lag as f64 / sum } else { 1.0 };
    }
    let mut lag = (min_lag.max(2)..max_lag).find(|&l| cm[l] < 0.15)?;
    while lag + 1 < max_lag && cm[lag + 1] < cm[lag] {
        lag += 1;
    }
    // parabolic interpolation between the neighbours
    let (a, b, c) = (cm[lag - 1], cm[lag], cm[lag + 1]);
    let den = a - 2.0 * b + c;
    let shift = if den.abs() > 1e-12 { 0.5 * (a - c) / den } else { 0.0 };
    Some(rate / (lag as f64 + shift.clamp(-1.0, 1.0)))
}

/// The nearest note (MIDI number) and how far off, in cents, at reference pitch `a4`.
pub fn note_cents(hz: f64, a4: f64) -> (u8, f64) {
    let m = 69.0 + 12.0 * (hz / a4).log2();
    let n = m.round();
    (n.clamp(0.0, 127.0) as u8, (m - n) * 100.0)
}

// ---------------------------------------------------------------------------------------------
// Quick Help: what the area under the pointer is (Studio's own words)

/// Areas the window registers each frame (App::frame, tracks.rs): where, which.
pub fn area(app: &mut App, r: Rect, which: &'static str) {
    app.nav.help_areas.push((r, which));
}

fn help_text(which: &str) -> (&'static str, &'static str) {
    match which {
        "control-bar" => ("Control Bar", "The transport (Stop, Play, Record, Cycle), the display with the position, tempo, signature and key, the mode buttons and the buttons that show or hide the window's panes. Right-click the Stop button for its options."),
        "toolbar" => ("Toolbar", "Buttons for editing commands. Right-click it, or choose View ▸ Customize Toolbar…, to pick the buttons."),
        "browser" => ("Browser", "Patches, loops and plug-in presets. Double-click a patch to load it on the selected track, or search for one by name."),
        "inspector" => ("Inspector", "The selected region's and track's parameters, and the track's channel strip with its output."),
        "ruler" => ("Ruler", "Click to move the playhead; drag in the upper part to set the cycle area. The numbers are bars."),
        "headers" => ("Track Headers", "Each track's name, icon and buttons (Mute, Solo, Record, Input Monitoring), its volume and pan. Drag a header to move the track."),
        "lanes" => ("Tracks Area", "The project's regions on a time grid. Drag regions to move them, their edges to resize, Option-drag to copy. Scroll sideways to move in time; pinch or ⌘-scroll to zoom."),
        "editor" => ("Editors", "The selected region's editor: Piano Roll, Score, Step Sequencer, the audio editors or the Session Player's settings."),
        "mixer" => ("Mixer", "A channel strip for every track, aux and output: inserts, sends, pan, the fader and meters."),
        "right-pane" => ("Lists and Browsers", "List editors (events, markers, tempo, signature), the note pad, the loop browser and the project's files."),
        _ => ("Bramble Studio", "Point at a part of the window to see what it does."),
    }
}

// ---------------------------------------------------------------------------------------------
// the windows

pub fn show(app: &mut App, ctx: &egui::Context) {
    crate::toolbar::sheets(app, ctx);
    ask(app, ctx);
    screenset_sheet(app, ctx);
    tuner(app, ctx);
    about(app, ctx);
    quick_help(app, ctx);
    app.nav.help_areas.clear();
}

fn ask(app: &mut App, ctx: &egui::Context) {
    let Some(a) = app.nav.ask.clone() else { return };
    let (title, label, mut text) = match a.clone() {
        Ask::GoToPosition(t) => ("Go to Position", "Position (bar beat division tick, or minutes:seconds)", t),
        Ask::LeftLocator(t) => ("Set Left Locator", "Left locator (bar beat division tick)", t),
        Ask::RightLocator(t) => ("Set Right Locator", "Right locator (bar beat division tick)", t),
        Ask::MarkerNumber(t) => ("Go to Marker Number", "Marker number", t),
    };
    let mut ok = false;
    let mut cancel = false;
    egui::Window::new(title).collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, vec2(0.0, 140.0)).show(ctx, |ui| {
        ui.label(label);
        let te = ui.add(egui::TextEdit::singleline(&mut text).desired_width(260.0));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            ok = true;
        }
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
            if ui.button("OK").clicked() {
                ok = true;
            }
        });
    });
    app.nav.ask = Some(match a {
        Ask::GoToPosition(_) => Ask::GoToPosition(text.clone()),
        Ask::LeftLocator(_) => Ask::LeftLocator(text.clone()),
        Ask::RightLocator(_) => Ask::RightLocator(text.clone()),
        Ask::MarkerNumber(_) => Ask::MarkerNumber(text.clone()),
    });
    if cancel {
        app.nav.ask = None;
        return;
    }
    if !ok {
        return;
    }
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let div = app.nav.division;
    let tick = |app: &App, t: &str| -> Option<u32> {
        if let Some(secs) = crate::nav::parse_time(t) {
            // the tick at that time (the tempo map: search)
            let (mut lo, mut hi) = (0.0f64, 1e9f64);
            for _ in 0..80 {
                let mid = (lo + hi) / 2.0;
                if app.seconds_at(mid) < secs { lo = mid } else { hi = mid }
            }
            return Some(lo.round() as u32);
        }
        crate::nav::parse_position(t, sig, div)
    };
    match app.nav.ask.take() {
        Some(Ask::GoToPosition(t)) => match tick(app, &t) {
            Some(at) => app.locate(at as f64),
            None => app.tell("Type a position such as 9 1 1 1"),
        },
        Some(Ask::LeftLocator(t)) | Some(Ask::RightLocator(t)) => {
            let left = matches!(a, Ask::LeftLocator(_));
            match tick(app, &t) {
                Some(at) => {
                    let (l, r) = app.cycle;
                    let c = if left { (at, r.max(at + 1)) } else { (l.min(at.saturating_sub(1)), at.max(1)) };
                    app.set_cycle(c);
                }
                None => app.tell("Type a position such as 5 1 1 1"),
            }
        }
        Some(Ask::MarkerNumber(t)) => {
            let n: usize = t.trim().parse().unwrap_or(0);
            match n.checked_sub(1).and_then(|i| app.markers.get(i)).map(|m| m.0) {
                Some(at) => app.locate(at as f64),
                None => app.tell(format!("There's no marker {n}")),
            }
        }
        None => {}
    }
}

fn screenset_sheet(app: &mut App, ctx: &egui::Context) {
    use crate::screensets::Sheet;
    let Some(sheet) = app.screens.sheet.clone() else { return };
    let mut ok = false;
    let mut cancel = false;
    let mut next = sheet.clone();
    let title = if matches!(sheet, Sheet::Rename(_)) { "Rename Screenset" } else { "Duplicate Screenset" };
    egui::Window::new(title).collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, vec2(0.0, 140.0)).show(ctx, |ui| {
        match &mut next {
            Sheet::Rename(name) => {
                ui.label("Name");
                let te = ui.add(egui::TextEdit::singleline(name).desired_width(240.0));
                crate::widgets::focus_once(ui, &te);
            }
            Sheet::Duplicate(n, name) => {
                egui::Grid::new("dup-ss").num_columns(2).show(ui, |ui| {
                    ui.label("Number");
                    ui.add(egui::TextEdit::singleline(n).desired_width(60.0));
                    ui.end_row();
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(name).desired_width(200.0));
                    ui.end_row();
                });
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            ok = true;
        }
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
            if ui.button("OK").clicked() {
                ok = true;
            }
        });
    });
    app.screens.sheet = Some(next.clone());
    if cancel {
        app.screens.sheet = None;
    } else if ok {
        match crate::screensets::finish_sheet(app, next) {
            Ok(()) => app.screens.sheet = None,
            Err(e) => app.tell(e),
        }
    }
}

pub const COMMANDS: &[&str] = &["Show/Hide Tuner", "Tuner…"];

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Show/Hide Tuner" | "Tuner…" => app.show_tuner = !app.show_tuner,
        _ => return false,
    }
    true
}

/// The Tuner (control bar's tuning fork): the input's note, how many cents off, and the Hz,
/// on Logic's arc of ±50 cents.
fn tuner(app: &mut App, ctx: &egui::Context) {
    let open_now = app.show_tuner;
    if let Ok(mut e) = app.engine.lock() {
        if e.rec.tuner != open_now {
            e.rec.tuner = open_now;
            e.rec.tap.clear();
        }
    }
    if !open_now {
        return;
    }
    let has_input = crate::recording::ensure_feed(app);
    let tap: Vec<f32> = app.engine.lock().map(|e| e.rec.tap.iter().copied().collect()).unwrap_or_default();
    let a4: f64 = app.logic_settings.0.get("tuner.a4").and_then(|v| v.parse().ok()).unwrap_or(440.0);
    let hz = pitch(&tap, crate::audio::RATE as f64);
    let mut open = true;
    egui::Window::new("Tuner").open(&mut open).collapsible(false).resizable(false).default_pos(ctx.content_rect().center() - vec2(160.0, 120.0)).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(320.0, 170.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(r, 8.0, look::lcd_bg());
        let c = pos2(r.center().x, r.max.y - 30.0);
        let rad = 115.0;
        // the scale: -50..+50 cents across 120°
        for k in -10..=10 {
            let a = (k as f32 / 10.0) * 60f32.to_radians();
            let dir = vec2(a.sin(), -a.cos());
            let len = if k % 5 == 0 { 14.0 } else { 7.0 };
            let col = if k == 0 { Color32::from_rgb(0x4c, 0xd9, 0x64) } else { look::lcd_dim() };
            p.line_segment([c + dir * (rad - len), c + dir * rad], Stroke::new(if k == 0 { 2.0 } else { 1.0 }, col));
        }
        p.text(c + vec2(-rad * 0.87 - 4.0, -rad * 0.5 - 12.0), Align2::CENTER_CENTER, "-50", look::regular(10.0), look::lcd_dim());
        p.text(c + vec2(rad * 0.87 + 4.0, -rad * 0.5 - 12.0), Align2::CENTER_CENTER, "+50", look::regular(10.0), look::lcd_dim());
        match hz {
            Some(f) => {
                let (n, cents) = note_cents(f, a4);
                let a = (cents.clamp(-50.0, 50.0) as f32 / 50.0) * 60f32.to_radians();
                let dir = vec2(a.sin(), -a.cos());
                let in_tune = cents.abs() < 3.0;
                let col = if in_tune { Color32::from_rgb(0x4c, 0xd9, 0x64) } else { Color32::from_rgb(0xff, 0x9f, 0x0a) };
                p.line_segment([c, c + dir * (rad - 4.0)], Stroke::new(2.0, col));
                p.circle_filled(c, 4.0, col);
                p.text(pos2(r.center().x, r.min.y + 58.0), Align2::CENTER_CENTER, crate::app::note_name(n), look::bold(30.0), look::lcd_text());
                p.text(pos2(r.min.x + 12.0, r.max.y - 12.0), Align2::LEFT_CENTER, format!("{cents:+.1} cents"), look::regular(11.0), look::lcd_dim());
                p.text(pos2(r.max.x - 12.0, r.max.y - 12.0), Align2::RIGHT_CENTER, format!("{f:.1} Hz"), look::regular(11.0), look::lcd_dim());
            }
            None => {
                let msg = if has_input { "Play a note" } else { "No audio input" };
                p.text(pos2(r.center().x, r.min.y + 58.0), Align2::CENTER_CENTER, msg, look::regular(15.0), look::lcd_dim());
            }
        }
        ui.horizontal(|ui| {
            ui.label("Reference");
            let mut v = a4;
            if ui.add(egui::DragValue::new(&mut v).range(415.0..=466.0).speed(0.1).suffix(" Hz")).changed() {
                app.logic_settings.0.insert("tuner.a4".into(), format!("{v:.1}"));
            }
        });
    });
    if !open {
        app.show_tuner = false;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(50));
}

/// About Bramble Studio: the gold record, the version, the credits.
fn about(app: &mut App, ctx: &egui::Context) {
    if !app.show_about {
        return;
    }
    let mut open = true;
    egui::Window::new("About Bramble Studio").title_bar(false).collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, vec2(0.0, 120.0)).show(ctx, |ui| {
        ui.set_width(300.0);
        ui.vertical_centered(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(96.0, 96.0), Sense::hover());
            let p = ui.painter();
            let c = r.center();
            p.circle_filled(c, 46.0, Color32::from_rgb(0xc9, 0x9a, 0x2e));
            for k in 0..6 {
                p.circle_stroke(c, 40.0 - k as f32 * 5.0, Stroke::new(0.8, Color32::from_rgb(0x9c, 0x73, 0x1c)));
            }
            p.circle_filled(c, 12.0, Color32::from_rgb(0x2a, 0x22, 0x18));
            p.circle_filled(c, 2.5, Color32::from_rgb(0xe8, 0xd5, 0xa0));
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Bramble Studio").font(look::bold(20.0)));
            ui.label(egui::RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(look::text_dim()));
            ui.add_space(8.0);
            ui.label("A music studio that opens Logic Pro and GarageBand projects, plays them with its own instruments and effects, and edits them.");
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Part of Bramble Office. Free and open source (GPL-3.0).").color(look::text_dim()));
            ui.label(egui::RichText::new("Music symbols: Bravura © Steinberg Media Technologies GmbH (SIL OFL 1.1).").font(look::regular(10.5)).color(look::text_dim()));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.link("bramble-apps.github.io").clicked() {
                    kn_ui::open_url("https://bramble-apps.github.io");
                }
                ui.add_space(20.0);
                if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)) {
                    open = false;
                }
            });
        });
    });
    app.show_about = open;
}

/// Quick Help (⇧/): what the area under the pointer does, in the Inspector's top, a floating
/// window or a bubble by the pointer (Help ▸ Quick Help appears as).
fn quick_help(app: &mut App, ctx: &egui::Context) {
    if !app.quick_help {
        return;
    }
    let at = ctx.input(|i| i.pointer.hover_pos());
    // the last registered area under the pointer is the innermost
    let which = at.and_then(|p| app.nav.help_areas.iter().rev().find(|(r, _)| r.contains(p)).map(|x| x.1)).unwrap_or("");
    let (title, text) = help_text(which);
    let body = |ui: &mut egui::Ui| {
        ui.set_max_width(260.0);
        ui.label(egui::RichText::new(title).font(look::bold(12.5)));
        ui.label(egui::RichText::new(text).font(look::regular(11.5)));
    };
    match app.nav.quick_help_as {
        2 => {
            let Some(p) = at else { return };
            egui::Area::new(egui::Id::unique("quick-help-bubble")).order(egui::Order::Tooltip).fixed_pos(p + vec2(18.0, 20.0)).interactable(false).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, body);
            });
        }
        0 if app.show_inspector => {
            let r = app.nav.inspector_rect;
            egui::Area::new(egui::Id::unique("quick-help-pane")).order(egui::Order::Foreground).fixed_pos(r.min + vec2(6.0, 6.0)).interactable(false).show(ctx, |ui| {
                egui::Frame::new().fill(look::pane_bg()).stroke(Stroke::new(1.0, look::line())).corner_radius(6.0).inner_margin(8).show(ui, |ui| {
                    ui.set_width(r.width() - 28.0);
                    body(ui)
                });
            });
        }
        _ => {
            let mut open = true;
            egui::Window::new("Quick Help").open(&mut open).collapsible(false).resizable(false).default_pos(ctx.content_rect().right_top() + vec2(-320.0, 120.0)).show(ctx, body);
            if !open {
                app.quick_help = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f64, n: usize) -> Vec<f32> {
        (0..n).map(|i| (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / 48000.0).sin()) as f32).collect()
    }

    #[test]
    fn finds_the_pitch() {
        for hz in [82.41, 110.0, 196.0, 440.0, 1046.5] {
            let f = pitch(&sine(hz, 8192), 48000.0).unwrap();
            assert!((f - hz).abs() / hz < 0.003, "{hz}: {f}");
        }
        // a sawtooth-like tone with harmonics still reads its fundamental
        let x: Vec<f32> = (0..8192).map(|i| { let t = i as f64 / 48000.0; (0.4 * (2.0 * std::f64::consts::PI * 220.0 * t).sin() + 0.3 * (2.0 * std::f64::consts::PI * 440.0 * t).sin() + 0.2 * (2.0 * std::f64::consts::PI * 660.0 * t).sin()) as f32 }).collect();
        let f = pitch(&x, 48000.0).unwrap();
        assert!((f - 220.0).abs() < 1.0, "{f}");
        assert_eq!(pitch(&vec![0.0; 8192], 48000.0), None);
    }

    #[test]
    fn notes_and_cents() {
        assert_eq!(note_cents(440.0, 440.0).0, 69);
        let (n, c) = note_cents(446.0, 440.0);
        assert_eq!(n, 69);
        assert!((c - 23.4).abs() < 0.2, "{c}");
        let (n, c) = note_cents(261.0, 440.0);
        assert_eq!(n, 60);
        assert!(c < 0.0);
        // A = 442 Hz reference
        assert!(note_cents(442.0, 442.0).1.abs() < 1e-9);
    }
}
