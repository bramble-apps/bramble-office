//! Logic 13's Stop Button Options (the Stop button's right-click menu, also in the Transport
//! menu). Playing, Stop stops (and optionally goes to the left locator or the last locate
//! position). Stopped, Stop jumps between a start point (the marquee's, the cycle's, the
//! selected region's or the last locate position, as ticked) and the project start; the button
//! then shows Logic's Go to Beginning glyph.

use crate::app::App;

pub const COMMANDS: &[&str] = &[
    "Stop",
    "Stop and Go to Left Locator",
    "Stop and Go to Last Locate Position",
    "Jump between Marquee- and Project Start if stopped",
    "Jump between Cycle- and Project Start if stopped",
    "Jump between Selected Region- and Project Start if stopped",
    "Jump between Last Locate Position and Project Start if stopped",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    Stop,
    LeftLocator,
    LastLocate,
}

#[derive(Clone, Debug)]
pub struct Opts {
    pub mode: Mode,
    pub marquee: bool,
    pub cycle: bool,
    pub region: bool,
    pub last_locate: bool,
    /// Where playback last started (Logic's last locate position), ticks.
    pub last_pos: f64,
}

impl Default for Opts {
    /// Logic's defaults: Stop; jump between marquee / cycle and project start.
    fn default() -> Self {
        Opts { mode: Mode::Stop, marquee: true, cycle: true, region: false, last_locate: false, last_pos: 0.0 }
    }
}

/// The ticked start points, in Logic's order, that exist right now.
fn start_point(o: &Opts, marquee: Option<u32>, cycle: Option<u32>, region: Option<u32>) -> Option<f64> {
    [(o.marquee, marquee.map(f64::from)), (o.cycle, cycle.map(f64::from)), (o.region, region.map(f64::from)), (o.last_locate, Some(o.last_pos))]
        .into_iter()
        .find_map(|(on, at)| if on { at } else { None })
}

/// Where Stop sends the playhead while stopped: to the start point, or from there to the
/// project start.
pub fn stopped_target(o: &Opts, pos: f64, marquee: Option<u32>, cycle: Option<u32>, region: Option<u32>) -> f64 {
    match start_point(o, marquee, cycle, region) {
        Some(s) if (pos - s).abs() > 0.5 => s,
        _ => 0.0,
    }
}

/// The Stop button / Stop command.
pub fn press(app: &mut App) {
    let playing = app.playing();
    let o = app.stopbtn.clone();
    if playing {
        app.halt();
        match o.mode {
            Mode::Stop => {}
            Mode::LeftLocator => {
                let c = app.cycle.0 as f64;
                app.locate(c);
            }
            Mode::LastLocate => app.locate(o.last_pos),
        }
        return;
    }
    let marquee = app.ta.regions.marquee.as_ref().map(|m| m.from);
    let cycle = if app.cycle_on() { Some(app.cycle.0) } else { None };
    let region = app.selected_region.and_then(|(t, r)| app.project.as_ref()?.tracks.get(t)?.regions.get(r).map(|g| g.start));
    let to = stopped_target(&o, app.position(), marquee, cycle, region);
    app.locate(to);
}

/// The menu's commands; true when `name` was one.
pub fn command(app: &mut App, name: &str) -> bool {
    let o = &mut app.stopbtn;
    match name {
        "Stop and Go to Left Locator" => o.mode = Mode::LeftLocator,
        "Stop and Go to Last Locate Position" => o.mode = Mode::LastLocate,
        "Jump between Marquee- and Project Start if stopped" => o.marquee = !o.marquee,
        "Jump between Cycle- and Project Start if stopped" => o.cycle = !o.cycle,
        "Jump between Selected Region- and Project Start if stopped" => o.region = !o.region,
        "Jump between Last Locate Position and Project Start if stopped" => o.last_locate = !o.last_locate,
        _ => return false,
    }
    true
}

/// The menu's check marks ("Stop" is also the Transport command: its mark is drawn only in the
/// Stop button's own menu).
pub fn checked(app: &App, label: &str) -> Option<bool> {
    let o = &app.stopbtn;
    Some(match label {
        "Stop and Go to Left Locator" => o.mode == Mode::LeftLocator,
        "Stop and Go to Last Locate Position" => o.mode == Mode::LastLocate,
        "Jump between Marquee- and Project Start if stopped" => o.marquee,
        "Jump between Cycle- and Project Start if stopped" => o.cycle,
        "Jump between Selected Region- and Project Start if stopped" => o.region,
        "Jump between Last Locate Position and Project Start if stopped" => o.last_locate,
        _ => return None,
    })
}

/// The control bar's right-click menu on the Stop button.
pub fn context_menu(app: &mut App, ui: &mut egui::Ui) {
    for (i, label) in COMMANDS.iter().enumerate() {
        if i == 3 {
            ui.separator();
        }
        let on = if *label == "Stop" { app.stopbtn.mode == Mode::Stop } else { checked(app, label).unwrap_or(false) };
        if ui.add(egui::Button::new(*label).selected(on)).clicked() {
            if *label == "Stop" {
                app.stopbtn.mode = Mode::Stop;
            } else {
                command(app, label);
            }
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_jumps_between_cycle_and_project_start() {
        let o = Opts::default();
        // cycle on at bar 5: first press goes there, the next to the project start, then back
        assert_eq!(stopped_target(&o, 9000.0, None, Some(3840), None), 3840.0);
        assert_eq!(stopped_target(&o, 3840.0, None, Some(3840), None), 0.0);
        assert_eq!(stopped_target(&o, 0.0, None, Some(3840), None), 3840.0);
        // nothing ticked exists: the project start
        assert_eq!(stopped_target(&o, 9000.0, None, None, Some(960)), 0.0);
    }

    #[test]
    fn marquee_comes_before_cycle() {
        let o = Opts::default();
        assert_eq!(stopped_target(&o, 9000.0, Some(1920), Some(3840), None), 1920.0);
        let o = Opts { region: true, cycle: false, marquee: false, ..Opts::default() };
        assert_eq!(stopped_target(&o, 9000.0, Some(1920), Some(3840), Some(960)), 960.0);
    }
}
