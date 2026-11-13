//! Transport extras, as Logic's:
//! - the control bar's optional transport buttons (Customize Control Bar and Display ▸
//!   Transport): Rewind and Forward (a click: a bar; held: fast; dragged: scrubbing; ⌘-click:
//!   the previous / next marker), Go to Beginning, Go to Position, Go to Left / Right Locator,
//!   Go to Selection Start, Play from Beginning / Left Window Edge / Left Locator / Right
//!   Locator / Selection;
//! - Shuttle Rewind / Shuttle Forward (each press winds faster; the other way slows, then
//!   reverses; shuttling turns Cycle off and Stop halts it);
//! - Send discrete Note Offs (Panic) and Send Maximum Volume;
//! - the LCD menu's Open Giant Beats Display / Open Giant Time Display (floating, resizable, as
//!   many as wanted) and Use SMPTE View Offset;
//! - Project Information… (counts and memory, Reorganize Memory).

use egui::{vec2, Align2, Color32, Rect, Sense};

use crate::app::App;
use crate::glyphs::{ph, ph_fill};
use crate::look;

/// The commands this file runs (build.rs and tool/checklist.py read the list).
pub const COMMANDS: &[&str] = &["Shuttle Rewind", "Shuttle Forward", "Send discrete Note Offs (Panic)", "Send Maximum Volume", "Project Information…", "Open Giant Beats Display", "Open Giant Time Display"];

/// The optional transport buttons: (name, icon, the command a click runs).
pub const BUTTONS: [(&str, &str, &str); 12] = [
    ("Rewind", ph_fill::REWIND, "Rewind"),
    ("Forward", ph_fill::FAST_FORWARD, "Forward"),
    ("Go to Beginning", ph_fill::SKIP_BACK, "Go to Beginning"),
    ("Go to Position", ph::CROSSHAIR, "Go to Position…"),
    ("Go to Left Locator", ph::ARROW_LINE_LEFT, "Go to Left Locator"),
    ("Go to Right Locator", ph::ARROW_LINE_RIGHT, "Go to Right Locator"),
    ("Go to Selection Start", ph::CARET_LINE_LEFT, "Go to Selection Start"),
    ("Play from Beginning", ph::PLAY_CIRCLE, "Play from Beginning"),
    ("Play from Left Window Edge", ph::PLAY_PAUSE, "Play from Left Window Edge"),
    ("Play from Left Locator", ph_fill::ARROW_LINE_LEFT, "Play from Left Locator"),
    ("Play from Right Locator", ph_fill::ARROW_LINE_RIGHT, "Play from Right Locator"),
    ("Play from Selection", ph_fill::CARET_LINE_LEFT, "Play from Selection"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Giant {
    Beats,
    Time,
}

#[derive(Default)]
pub struct State {
    loaded: bool,
    /// The optional buttons shown (BUTTONS' names).
    pub shown: Vec<String>,
    /// Shuttling: the speed step (− rewinding, + forward; 0 = not shuttling).
    pub shuttle: i32,
    last: Option<f64>,
    /// A Rewind / Forward button held: when, and when it last moved.
    held: Option<(String, f64, f64)>,
    pub giants: Vec<(u64, Giant)>,
    next_id: u64,
    pub info_open: bool,
    info_report: Option<String>,
    /// Tap Tempo's first use: the alert asking to turn it on.
    pub tap_alert: bool,
}

pub const TAP_ON: &str = "Studio / Tap Tempo on";

fn path() -> Option<std::path::PathBuf> {
    Some(dirs_config()?.join("bramble-studio").join("transport-buttons.txt"))
}

fn dirs_config() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
}

fn ensure(app: &mut App) {
    if app.tx.loaded {
        return;
    }
    app.tx.loaded = true;
    if std::env::var_os("LG_SNAP").is_some() {
        // (checks: LG_TRANSPORT="Rewind,Forward" shows those)
        app.tx.shown = std::env::var("LG_TRANSPORT").unwrap_or_default().split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
        return;
    }
    if let Some(t) = path().and_then(|p| std::fs::read_to_string(p).ok()) {
        app.tx.shown = t.lines().filter(|l| BUTTONS.iter().any(|b| b.0 == *l)).map(str::to_string).collect();
    }
}

fn save(app: &App) {
    if std::env::var_os("LG_SNAP").is_some() {
        return;
    }
    if let Some(p) = path() {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, app.tx.shown.join("\n"));
    }
}

/// The optional buttons shown, in Logic's order.
pub fn shown(app: &mut App) -> Vec<(&'static str, &'static str, &'static str)> {
    ensure(app);
    BUTTONS.iter().copied().filter(|b| app.tx.shown.iter().any(|s| s == b.0)).collect()
}

/// Customize Control Bar and Display ▸ Transport: a checkbox per optional button.
pub fn customize(app: &mut App, ui: &mut egui::Ui) {
    ensure(app);
    for (name, _, _) in BUTTONS {
        let mut on = app.tx.shown.iter().any(|s| s == name);
        if ui.checkbox(&mut on, name).changed() {
            if on {
                app.tx.shown.push(name.to_string());
            } else {
                app.tx.shown.retain(|s| s != name);
            }
            save(app);
        }
    }
}

/// An optional button's behaviour (its response from the control bar).
pub fn button(app: &mut App, ctx: &egui::Context, name: &str, cmd: &str, resp: &egui::Response) {
    let now = ctx.input(|i| i.time);
    let wind = matches!(name, "Rewind" | "Forward");
    if wind {
        let dir = if name == "Rewind" { -1.0 } else { 1.0 };
        // dragged: scrubbing (a bar per 20 points)
        if resp.dragged() {
            let dx = resp.drag_delta().x;
            if dx != 0.0 {
                let bar = app.bar_ticks() as f64;
                let p = app.position();
                app.locate((p + dx as f64 / 20.0 * bar).max(0.0));
            }
            app.tx.held = None;
            return;
        }
        if resp.is_pointer_button_down_on() {
            match app.tx.held.clone() {
                None => app.tx.held = Some((name.to_string(), now, now)),
                // held: fast winding, four bars every tenth of a second
                Some((n, t0, last)) if n == name && now - t0 > 0.45 && now - last > 0.1 => {
                    let _ = app.key_command(ctx, if dir < 0.0 { "Fast Rewind" } else { "Fast Forward" });
                    app.tx.held = Some((n, t0, now));
                }
                _ => {}
            }
            ctx.request_repaint();
        }
        if resp.clicked() {
            let fast = app.tx.held.as_ref().is_some_and(|h| now - h.1 > 0.45);
            app.tx.held = None;
            if fast {
                return;
            }
            let marker = ctx.input(|i| i.modifiers.command);
            let c = if marker { if dir < 0.0 { "Go to Previous Marker" } else { "Go to Next Marker" } } else { cmd };
            let _ = app.key_command(ctx, c);
        } else if !resp.is_pointer_button_down_on() {
            if app.tx.held.as_ref().is_some_and(|h| h.0 == name) {
                app.tx.held = None;
            }
        }
        return;
    }
    if resp.clicked() {
        let _ = app.key_command(ctx, cmd);
    }
}

/// The next shuttle speed step after a press (`dir` −1 rewind, +1 forward): the same way
/// winds faster (up to 4), the other way slows, then reverses.
pub fn shuttle_step(cur: i32, dir: i32) -> i32 {
    (cur + dir).clamp(-4, 4)
}

/// Bars a second at a shuttle step.
pub fn shuttle_bars_per_sec(step: i32) -> f64 {
    if step == 0 {
        return 0.0;
    }
    step.signum() as f64 * 2f64.powi(step.abs() - 1)
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    match name {
        "Shuttle Rewind" | "Shuttle Forward" => {
            let dir = if name == "Shuttle Rewind" { -1 } else { 1 };
            if app.tx.shuttle == 0 && app.cycle_on() {
                app.toggle_cycle();
            }
            app.tx.shuttle = shuttle_step(app.tx.shuttle, dir);
            app.tx.last = None;
            true
        }
        // (Stop halts shuttling; the stop itself runs as usual)
        "Stop" | "Play or Stop" | "Pause" => {
            app.tx.shuttle = 0;
            false
        }
        // the first Tap Tempo asks to turn it on (Manual sync, as Logic's)
        "Tap Tempo" if app.logic_settings.get(TAP_ON) != "1" && !app.automated => {
            app.tx.tap_alert = true;
            true
        }
        "Send discrete Note Offs (Panic)" => {
            panic(app);
            true
        }
        "Send Maximum Volume" => {
            max_volume(app);
            true
        }
        "Project Information…" | "Project Information" => {
            app.tx.info_open = true;
            app.tx.info_report = None;
            true
        }
        "Open Giant Beats Display" | "Open Giant Time Display" => {
            app.tx.next_id += 1;
            let id = app.tx.next_id;
            app.tx.giants.push((id, if name.contains("Beats") { Giant::Beats } else { Giant::Time }));
            let _ = ctx;
            true
        }
        _ => false,
    }
}

/// Panic: a Note Off for every note on all 16 channels of every MIDI output, and every
/// software instrument silenced.
pub fn panic_bytes() -> Vec<u8> {
    let mut out = Vec::with_capacity(16 * 128 * 3);
    for ch in 0..16u8 {
        for n in 0..128u8 {
            out.extend([0x80 | ch, n, 0]);
        }
    }
    out
}

fn panic(app: &mut App) {
    if let Ok(mut e) = app.engine.lock() {
        e.all_off();
    }
    let bytes = panic_bytes();
    let outs = app.hub.outs.clone();
    for port in outs.ports() {
        outs.send(&port, &bytes);
    }
    app.tell("Panic: Note Offs sent");
}

/// Send Maximum Volume: CC 7 at 127 on all channels of every output and every instrument.
pub fn max_volume_bytes() -> Vec<u8> {
    (0..16u8).flat_map(|ch| [0xB0 | ch, 7, 127]).collect()
}

fn max_volume(app: &mut App) {
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    if let Ok(mut e) = app.engine.lock() {
        for t in 0..n {
            e.live_control(t, 7, 127);
        }
    }
    let bytes = max_volume_bytes();
    let outs = app.hub.outs.clone();
    for port in outs.ports() {
        outs.send(&port, &bytes);
    }
}

/// Each frame: shuttling and the windows.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    if app.tx.shuttle != 0 {
        let now = ctx.input(|i| i.time);
        let dt = app.tx.last.map_or(0.0, |l| now - l).min(0.1);
        app.tx.last = Some(now);
        let bar = app.bar_ticks() as f64;
        let p = app.position() + shuttle_bars_per_sec(app.tx.shuttle) * dt * bar;
        if p <= 0.0 {
            app.tx.shuttle = 0;
        }
        app.locate(p.max(0.0));
        ctx.request_repaint();
    }
    giants(app, ctx);
    info_window(app, ctx);
    tap_alert(app, ctx);
}

fn tap_alert(app: &mut App, ctx: &egui::Context) {
    if !app.tx.tap_alert {
        return;
    }
    let mut answer: Option<bool> = None;
    egui::Window::new("Tap Tempo").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        ui.label("Tap Tempo sets the project tempo from your taps. Turn on Tap Tempo?");
        ui.label(egui::RichText::new("(Synchronization goes to Manual, as Logic's.)").color(look::text_dim()));
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
            if ui.button("Turn On").clicked() {
                answer = Some(true);
            }
        });
    });
    if let Some(on) = answer {
        app.tx.tap_alert = false;
        if on {
            app.logic_settings.0.insert(TAP_ON.into(), "1".into());
            app.logic_settings.save();
            app.proj.0.insert("Synchronization / General / Sync Mode:".into(), "Manual".into());
            app.dirty = true;
        }
    }
}

/// The LCD's ⌄ menu items (after the display modes).
pub fn lcd_menu(app: &mut App, ui: &mut egui::Ui) {
    if ui.button("Open Giant Beats Display").clicked() {
        let ctx = ui.ctx().clone();
        command(app, &ctx, "Open Giant Beats Display");
        ui.close();
    }
    if ui.button("Open Giant Time Display").clicked() {
        let ctx = ui.ctx().clone();
        command(app, &ctx, "Open Giant Time Display");
        ui.close();
    }
    let k = "Synchronization / General / Enable separate SMPTE view offset";
    let on = app.proj.on(k);
    if ui.add(egui::Button::selectable(on, "Use SMPTE View Offset")).clicked() {
        app.proj.0.insert(k.to_string(), if on { "0" } else { "1" }.to_string());
        app.dirty = true;
        ui.close();
    }
    ui.separator();
}

fn giants(app: &mut App, ctx: &egui::Context) {
    let list = app.tx.giants.clone();
    let mut closed = Vec::new();
    for (id, kind) in list {
        let mut open = true;
        let text = match kind {
            Giant::Beats => {
                let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
                crate::lcdcustom::bars_text(app.position(), sig, 240)
            }
            Giant::Time => {
                let secs = app.seconds_at(app.position());
                crate::prefs::time_text(&app.logic_settings, &app.proj, secs)
            }
        };
        let title = if kind == Giant::Beats { "Giant Beats Display" } else { "Giant Time Display" };
        egui::Window::new(title).id(egui::Id::unique(("lg-giant", id))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(520.0, 140.0)).frame(egui::Frame::window(&ctx.global_style()).fill(look::lcd_bg())).show(ctx, |ui| {
            let size = ui.available_size().max(vec2(200.0, 60.0));
            let (r, _) = ui.allocate_exact_size(size, Sense::hover());
            // (the digits fill the window: their size follows its height and width)
            let fs = (r.height() * 0.7).min(r.width() / (text.chars().count().max(4) as f32 * 0.62));
            ui.painter().text(r.center(), Align2::CENTER_CENTER, &text, egui::FontId::monospace(fs), look::lcd_text());
        });
        if !open {
            closed.push(id);
        }
    }
    app.tx.giants.retain(|g| !closed.contains(&g.0));
}

/// What Project Information counts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub tracks: usize,
    pub regions: usize,
    pub audio_regions: usize,
    pub midi_regions: usize,
    pub notes: usize,
    pub other_events: usize,
    pub markers: usize,
    pub automation_points: usize,
    pub bytes: usize,
}

pub fn count(p: &lg_core::Project) -> Info {
    let mut i = Info { tracks: p.tracks.len(), markers: p.markers.len(), ..Default::default() };
    for t in &p.tracks {
        for r in &t.regions {
            i.regions += 1;
            if r.audio.is_some() {
                i.audio_regions += 1;
            } else {
                i.midi_regions += 1;
            }
            i.notes += r.notes.len();
            i.other_events += r.ext.events.len();
        }
        i.automation_points += t.automation.iter().map(|a| a.points.len()).sum::<usize>();
    }
    // (memory: the events and regions as Studio keeps them)
    i.bytes = i.notes * std::mem::size_of::<lg_core::events::Note>() + i.other_events * 16 + i.regions * std::mem::size_of::<lg_core::Region>() + i.automation_points * 8 + i.tracks * std::mem::size_of::<lg_core::Track>();
    i
}

/// Reorganize Memory's check: damaged objects (regions with no length, notes of no length or
/// outside 0–127) and unused blocks (empty automation lanes). Returns what it found.
pub fn check(p: &lg_core::Project) -> (usize, usize) {
    let mut damaged = 0;
    let mut unused = 0;
    for t in &p.tracks {
        for r in &t.regions {
            if r.length == 0 {
                damaged += 1;
            }
            damaged += r.notes.iter().filter(|n| n.len == 0 || n.pitch > 127).count();
        }
        unused += t.automation.iter().filter(|a| a.points.is_empty()).count();
    }
    (damaged, unused)
}

/// Removes what `check` found.
pub fn reorganize(p: &mut lg_core::Project) -> (usize, usize) {
    let found = check(p);
    for t in p.tracks.iter_mut() {
        t.regions.retain(|r| r.length > 0);
        for r in t.regions.iter_mut() {
            r.notes.retain(|n| n.len > 0 && n.pitch <= 127);
            r.notes.shrink_to_fit();
        }
        t.automation.retain(|a| !a.points.is_empty());
    }
    found
}

fn info_window(app: &mut App, ctx: &egui::Context) {
    if !app.tx.info_open {
        return;
    }
    let Some(p) = app.project.as_ref() else { return };
    let i = count(p);
    let name = p.name.clone();
    let mut open = true;
    let mut reorganize_now = false;
    egui::Window::new("Project Information").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.label(egui::RichText::new(&name).font(look::semibold(13.0)));
        ui.add_space(4.0);
        egui::Grid::new("pinfo").num_columns(2).spacing([24.0, 3.0]).show(ui, |ui| {
            for (k, v) in [("Tracks", i.tracks), ("Regions", i.regions), ("  Audio Regions", i.audio_regions), ("  MIDI Regions", i.midi_regions), ("MIDI Events (notes)", i.notes), ("Other MIDI Events", i.other_events), ("Markers", i.markers), ("Automation Points", i.automation_points)] {
                ui.label(egui::RichText::new(k).color(look::text_dim()));
                ui.label(v.to_string());
                ui.end_row();
            }
            ui.label(egui::RichText::new("Memory Used").color(look::text_dim()));
            ui.label(format!("{:.1} KB", i.bytes as f64 / 1024.0));
            ui.end_row();
        });
        ui.add_space(6.0);
        if let Some(r) = &app.tx.info_report {
            ui.label(egui::RichText::new(r).color(look::text_dim()));
        }
        ui.horizontal(|ui| {
            if ui.button("Reorganize Memory").clicked() {
                reorganize_now = true;
            }
        });
    });
    if reorganize_now {
        let found = app.project.as_ref().map(check).unwrap_or_default();
        app.tx.info_report = Some(if found == (0, 0) {
            "No damaged or unused blocks found".to_string()
        } else {
            app.checkpoint("Reorganize Memory");
            if let Some(p) = app.project.as_mut() {
                reorganize(p);
            }
            app.load_engine();
            format!("Removed {} damaged and {} unused blocks", found.0, found.1)
        });
    }
    if !open {
        app.tx.info_open = false;
    }
}

/// After opening a project: a warning when its data is damaged, and memory tidied.
pub fn after_open(app: &mut App) {
    let Some(p) = app.project.as_mut() else { return };
    let (damaged, _) = check(p);
    for t in p.tracks.iter_mut() {
        t.automation.retain(|a| !a.points.is_empty());
    }
    if damaged > 0 {
        app.tell(format!("This project has {damaged} damaged objects: Project Information… ▸ Reorganize Memory removes them"));
    }
}

#[allow(dead_code)]
fn _unused(_: Rect, _: Color32) {}

pub const DISPLAY_TEMPO: &str = "View / General / Display Tempo As:";

/// The tempo as Settings ▸ View ▸ Display Tempo As writes it: BPM (decimals only when it has
/// them), or frames per click at the project's frame rate, with eighths or four decimals.
pub fn tempo_text(v: &crate::settings::Values, fps: f64, bpm: f64) -> String {
    let mode = v.get(DISPLAY_TEMPO);
    tempo_text_as(&mode, fps, bpm)
}

pub fn tempo_text_as(mode: &str, fps: f64, bpm: f64) -> String {
    let fpc = fps * 60.0 / bpm.max(1.0);
    if mode.starts_with("Frames per Click with Eighths") {
        let whole = fpc.floor();
        let eighths = ((fpc - whole) * 8.0).round();
        let (whole, eighths) = if eighths >= 8.0 { (whole + 1.0, 0.0) } else { (whole, eighths) };
        format!("{whole:.0} {eighths:.0}/8 fpc")
    } else if mode.starts_with("Frames per Click with Decimals") {
        format!("{fpc:.4} fpc")
    } else if (bpm - bpm.round()).abs() < 0.005 {
        format!("{bpm:.0}")
    } else {
        format!("{bpm:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuttle_speeds_up_slows_and_reverses() {
        let mut s = 0;
        s = shuttle_step(s, 1);
        assert_eq!(s, 1);
        s = shuttle_step(s, 1);
        assert_eq!(shuttle_bars_per_sec(s), 2.0);
        s = shuttle_step(s, -1);
        assert_eq!(s, 1);
        s = shuttle_step(s, -1);
        s = shuttle_step(s, -1);
        assert_eq!(s, -1);
        assert!(shuttle_bars_per_sec(s) < 0.0);
        assert_eq!(shuttle_step(4, 1), 4);
    }

    #[test]
    fn tempo_display_formats() {
        assert_eq!(tempo_text_as("Beats per Minute (BPM, Maelzel)", 25.0, 120.0), "120");
        assert_eq!(tempo_text_as("Beats per Minute (BPM, Maelzel)", 25.0, 120.5), "120.50");
        // 24 fps at 120 bpm: 12 frames a click
        assert_eq!(tempo_text_as("Frames per Click with Eighths (Film)", 24.0, 120.0), "12 0/8 fpc");
        assert_eq!(tempo_text_as("Frames per Click with Eighths (Film)", 24.0, 128.0), "11 2/8 fpc");
        assert_eq!(tempo_text_as("Frames per Click with Decimals (Film)", 24.0, 128.0), "11.2500 fpc");
    }

    #[test]
    fn panic_and_max_volume_messages() {
        let p = panic_bytes();
        assert_eq!(p.len(), 16 * 128 * 3);
        assert_eq!(&p[..3], &[0x80, 0, 0]);
        assert_eq!(&p[p.len() - 3..], &[0x8F, 127, 0]);
        let v = max_volume_bytes();
        assert_eq!(v.len(), 48);
        assert_eq!(&v[..3], &[0xB0, 7, 127]);
    }

    #[test]
    fn project_information_counts_and_reorganizes() {
        let mut p = crate::demo::song();
        let i = count(&p);
        assert_eq!(i.tracks, p.tracks.len());
        assert!(i.regions > 0 && i.notes > 0 && i.bytes > 0);
        p.tracks[0].automation.push(lg_core::project::AutoLane { param: 7, ..Default::default() });
        let n = p.tracks[0].regions.len();
        p.tracks[0].regions[0].length = 0;
        let (d, u) = check(&p);
        assert_eq!((d >= 1, u), (true, 1));
        reorganize(&mut p);
        assert_eq!(check(&p), (0, 0));
        assert_eq!(p.tracks[0].regions.len(), n - 1);
    }
}
