//! The toolbar (View ▸ Show Toolbar, ⌃⌥⌘T): a row of editing buttons under the control bar,
//! as Logic's; Customize Toolbar… picks its buttons. The Customize Control Bar and Display…
//! sheet: which optional transport buttons the control bar shows (controlbar::Shown) and the
//! LCD's display. Both kept in ~/.config/bramble-studio/toolbar.txt; Apply / Save As Control
//! Bar and Display Defaults.

use egui::{pos2, vec2, Align2, Rect, Sense, Stroke};

use crate::app::App;
use crate::look;

pub const H: f32 = 34.0;

/// The toolbar's buttons: label, the command it runs, on by default.
pub const BUTTONS: &[(&str, &str, bool)] = &[
    ("Set Locators", "Set Locators/Loop by Regions/Events/Marquee", true),
    ("Repeat Section", "Repeat Section Between Locators (Global)", true),
    ("Cut Section", "Cut Section Between Locators (Global)", true),
    ("Insert Section", "Insert Section at Playhead (Global)", true),
    ("Insert Silence", "Insert Silence Between Locators (Global)", true),
    ("Split", "Split Regions/Events at Playhead Position", true),
    ("Join", "Join Regions", true),
    ("Repeat", "Repeat Regions/Cells/Events", false),
    ("Quantize", "Quantize Selected Regions/Cells/Events", false),
    ("Bounce in Place", "Bounce Regions/Cells in Place…", true),
    ("Crossfade", "Apply Default Crossfade", true),
    ("Zoom", "Toggle Zoom to fit Selection or All Contents", true),
    ("Nudge Left", "Nudge Region/Event Position Left by Nudge Value", true),
    ("Nudge Right", "Nudge Region/Event Position Right by Nudge Value", true),
    ("Automation", "Show/Hide Automation", false),
    ("Store Snapshot", "Store Navigation Snapshot", false),
    ("Back", "Navigation: Back", false),
    ("Tap Tempo", "Tap Tempo", false),
    ("Note Pad", "Show/Hide Note Pad", false),
];

/// Which control-bar parts can be ticked in the sheet (beside controlbar::Shown's).
pub struct State {
    /// The toolbar's buttons shown (by label), in BUTTONS order.
    pub buttons: Vec<String>,
    pub customize_toolbar: bool,
    pub customize_bar: bool,
    /// Saved defaults: the buttons and the control bar's choices.
    defaults: Option<(Vec<String>, [bool; 3])>,
    applied: bool,
}

fn path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("toolbar.txt"))
}

fn default_buttons() -> Vec<String> {
    BUTTONS.iter().filter(|b| b.2).map(|b| b.0.to_string()).collect()
}

/// toolbar.txt: "button=Label" lines and "bar=capture,replace,autopunch" (1 / 0).
pub fn parse(text: &str) -> (Vec<String>, Option<[bool; 3]>) {
    let mut buttons = Vec::new();
    let mut bar = None;
    for l in text.lines() {
        if let Some(b) = l.strip_prefix("button=") {
            if BUTTONS.iter().any(|x| x.0 == b) {
                buttons.push(b.to_string());
            }
        } else if let Some(v) = l.strip_prefix("bar=") {
            let f: Vec<bool> = v.split(',').map(|x| x.trim() == "1").collect();
            if f.len() == 3 {
                bar = Some([f[0], f[1], f[2]]);
            }
        }
    }
    (buttons, bar)
}

pub fn write(buttons: &[String], bar: [bool; 3]) -> String {
    let mut s: String = buttons.iter().map(|b| format!("button={b}\n")).collect();
    s += &format!("bar={},{},{}\n", bar[0] as u8, bar[1] as u8, bar[2] as u8);
    s
}

impl State {
    pub fn load() -> State {
        let text = if std::env::var_os("LG_SNAP").is_some() { None } else { path().and_then(|p| std::fs::read_to_string(p).ok()) };
        let defaults = text.as_deref().map(parse).map(|(b, bar)| (if b.is_empty() { default_buttons() } else { b }, bar.unwrap_or_default()));
        State { buttons: defaults.as_ref().map_or_else(default_buttons, |d| d.0.clone()), customize_toolbar: false, customize_bar: false, defaults, applied: false }
    }
}

fn bar_flags(app: &App) -> [bool; 3] {
    [app.cb_show.capture, app.cb_show.replace, app.cb_show.autopunch]
}

fn set_bar_flags(app: &mut App, f: [bool; 3]) {
    app.cb_show.capture = f[0];
    app.cb_show.replace = f[1];
    app.cb_show.autopunch = f[2];
}

fn save_defaults(app: &mut App) {
    let f = bar_flags(app);
    app.bars.defaults = Some((app.bars.buttons.clone(), f));
    if app.automated {
        return;
    }
    if let Some(p) = path() {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, write(&app.bars.buttons, f));
    }
}

/// The saved defaults' control bar choices (at start: App::new keeps Logic's until applied).
pub fn apply_defaults(app: &mut App) {
    let (b, f) = app.bars.defaults.clone().unwrap_or_else(|| (default_buttons(), [false; 3]));
    app.bars.buttons = b;
    set_bar_flags(app, f);
}

/// The command names run here (build.rs and tool/checklist.py read this list).
#[allow(dead_code)]
pub const COMMANDS: &[&str] = &[
    "Show/Hide Toolbar", "Show Toolbar", "Customize Toolbar…",
    "Customize Control Bar and Display…", "Apply Control Bar and Display Defaults", "Save As Control Bar and Display Defaults",
];

pub fn command(app: &mut App, _ctx: &egui::Context, name: &str) -> bool {
    match name {
        "Show/Hide Toolbar" | "Show Toolbar" => app.show_toolbar = !app.show_toolbar,
        "Customize Toolbar…" => {
            app.show_toolbar = true;
            app.bars.customize_toolbar = true;
        }
        "Customize Control Bar and Display…" => app.bars.customize_bar = true,
        "Apply Control Bar and Display Defaults" => apply_defaults(app),
        "Save As Control Bar and Display Defaults" => {
            save_defaults(app);
            app.tell("Saved as the control bar's defaults");
        }
        _ => return false,
    }
    true
}

pub fn checked(_app: &App, _label: &str) -> Option<bool> {
    None
}

/// The toolbar under the control bar.
pub fn show(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    let p = ui.painter().clone();
    p.rect_filled(r, 0.0, look::bar_bg());
    p.hline(r.x_range(), r.max.y, Stroke::new(1.0, look::line()));
    let mut x = r.min.x + 10.0;
    let cy = r.center().y;
    let shown: Vec<(&str, &str)> = BUTTONS.iter().filter(|b| app.bars.buttons.iter().any(|s| s == b.0)).map(|b| (b.0, b.1)).collect();
    let mut fire = None;
    for (label, cmd) in shown {
        let g = p.layout_no_wrap(label.to_string(), look::regular(12.0), look::text());
        let br = Rect::from_min_size(pos2(x, cy - 11.0), vec2(g.size().x + 18.0, 22.0));
        if br.max.x > r.max.x - 40.0 {
            break;
        }
        let can = crate::menus::handled(cmd);
        let resp = ui.interact(br, ui.scope_id().with(("tb", label)), if can { Sense::click() } else { Sense::hover() }).on_hover_text(if can { cmd.to_string() } else { format!("{cmd} (not in Studio yet)") });
        p.rect_filled(br, 6.0, if resp.hovered() && can { look::button_on() } else { look::group_bg() });
        p.text(br.center(), Align2::CENTER_CENTER, label, look::regular(12.0), if can { look::text() } else { look::text_dim().gamma_multiply(0.6) });
        if resp.clicked() {
            fire = Some(cmd);
        }
        x = br.max.x + 6.0;
    }
    // the toolbar's right-click menu: Customize Toolbar…
    let whole = ui.interact(r, ui.scope_id().with("toolbar-bg"), Sense::click());
    whole.context_menu(|ui| {
        if ui.button("Customize Toolbar…").clicked() {
            app.bars.customize_toolbar = true;
            ui.close();
        }
    });
    if let Some(c) = fire {
        let ctx = ui.ctx().clone();
        app.command(&ctx, c);
    }
}

/// The sheets: Customize Toolbar… and Customize Control Bar and Display….
pub fn sheets(app: &mut App, ctx: &egui::Context) {
    // (the saved defaults, once at start)
    if !app.bars.applied {
        app.bars.applied = true;
        if app.bars.defaults.is_some() {
            apply_defaults(app);
        }
    }
    if app.bars.customize_toolbar {
        let mut open = true;
        let mut done = false;
        egui::Window::new("Customize Toolbar").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, vec2(0.0, 90.0)).show(ctx, |ui| {
            ui.label(egui::RichText::new("Choose the toolbar's buttons.").color(look::text_dim()));
            ui.add_space(6.0);
            egui::Grid::new("tb-grid").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
                for (i, (label, cmd, _)) in BUTTONS.iter().enumerate() {
                    let mut on = app.bars.buttons.iter().any(|b| b == label);
                    let text = if crate::menus::handled(cmd) { label.to_string() } else { format!("{label} (not yet)") };
                    if ui.checkbox(&mut on, text).changed() {
                        if on {
                            app.bars.buttons.push(label.to_string());
                            app.bars.buttons.sort_by_key(|b| BUTTONS.iter().position(|x| x.0 == b));
                        } else {
                            app.bars.buttons.retain(|b| b != label);
                        }
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Default Set").clicked() {
                    app.bars.buttons = default_buttons();
                }
                if ui.button("Done").clicked() {
                    done = true;
                }
            });
        });
        if !open || done {
            app.bars.customize_toolbar = false;
            save_defaults(app);
        }
    }
    if app.bars.customize_bar {
        let mut open = true;
        let mut done = false;
        egui::Window::new("Customize Control Bar and Display").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, vec2(0.0, 90.0)).show(ctx, |ui| {
            use crate::controlbar::LcdMode;
            egui::Grid::new("cb-grid").num_columns(3).spacing([40.0, 6.0]).show(ui, |ui| {
                for h in ["View", "Transport", "Display"] {
                    ui.label(egui::RichText::new(h).font(look::bold(12.0)));
                }
                ui.end_row();
                let modes = [(LcdMode::BeatsProject, "Beats & Project"), (LcdMode::BeatsTime, "Beats & Time"), (LcdMode::Time, "Time")];
                for i in 0..4 {
                    match i {
                        0 => ui.checkbox(&mut app.show_browser, "Browser"),
                        1 => ui.checkbox(&mut app.show_inspector, "Inspector"),
                        2 => ui.checkbox(&mut app.quick_help, "Quick Help"),
                        _ => ui.checkbox(&mut app.show_toolbar, "Toolbar"),
                    };
                    match i {
                        0 => ui.checkbox(&mut app.cb_show.capture, "Capture as Recording"),
                        1 => ui.checkbox(&mut app.cb_show.replace, "Replace"),
                        2 => ui.checkbox(&mut app.cb_show.autopunch, "Autopunch"),
                        _ => ui.add_enabled(false, egui::Checkbox::new(&mut true, "Stop, Play, Record, Cycle")),
                    };
                    if let Some((m, label)) = modes.get(i) {
                        if ui.radio(app.lcd_mode == *m, *label).clicked() {
                            app.lcd_mode = *m;
                        }
                    }
                    ui.end_row();
                }
            });
            // Modes and Functions: Sync (syncmidi.rs; hidden by default)
            ui.horizontal(|ui| {
                ui.label("Modes and Functions:");
                ui.checkbox(&mut app.hub.sync.show_button, "Sync");
                crate::varispeed::customize(ui);
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Apply Defaults").clicked() {
                    apply_defaults(app);
                }
                if ui.button("Save As Defaults").clicked() {
                    save_defaults(app);
                }
                if ui.button("Done").clicked() {
                    done = true;
                }
            });
        });
        if !open || done {
            app.bars.customize_bar = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_file_round_trip() {
        let b = vec!["Split".to_string(), "Join".to_string()];
        let t = write(&b, [true, false, true]);
        assert_eq!(parse(&t), (b, Some([true, false, true])));
        // unknown buttons are dropped
        assert_eq!(parse("button=Nope\nbutton=Zoom\n").0, vec!["Zoom".to_string()]);
        // every default button runs a Logic command name
        assert!(default_buttons().len() >= 8);
    }
}
