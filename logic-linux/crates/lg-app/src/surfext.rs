//! More of Logic's controller assignment classes (surfaces.rs calls in): the rest of the Global
//! class parameters (punch locators, marker position and length, nudging, Nudge Value, Scrub
//! Status, Shuttle Speed, Group Clutch, Active Sense, Alert, Dummy, Waveform Zoom, Quantize
//! Value, Division), the control surface group parameters beyond surfaces::Group (fader banks
//! per view and strip type, Filter for All View, surround / EQ / send / plug-in pages …),
//! automation group assignments, feedback for learned assignments in their Feedback Format,
//! and Key Repeat (with the re-learn that turning it on starts).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use lg_core::ctrlsurf::{self as cs, Assignment, Class, ValueMode};

use crate::app::App;

#[derive(Default)]
pub struct Ext {
    /// Per control surface group: the parameters beyond surfaces::Group (by GROUP_PARAMS name).
    pub gx: Vec<BTreeMap<String, i32>>,
    /// Key Repeat: the assignment held, when it repeats next.
    repeat: Option<(usize, Instant)>,
    /// Feedback sent per assignment (sent again on change).
    sent: BTreeMap<usize, Vec<u8>>,
    /// When each assignment last moved (Local Feedback: none while touched).
    touched: BTreeMap<usize, Instant>,
    /// Key Repeat turned on: re-learning this assignment (press, then release).
    pub relearn: Option<(usize, Option<i32>)>,
    /// Scrub Status: Set Clock, Audio Scrubbing, Shuttle.
    pub scrub: u8,
    /// Shuttle Speed: ticks per second (0: stopped).
    pub shuttle: f64,
    shuttle_at: Option<Instant>,
    /// Active Sense: the last Active Sensing message.
    pub sensed: Option<Instant>,
    /// Alert mode (plug-ins): the text shown.
    pub alert: Option<String>,
}

pub const COMMANDS: &[&str] = &[];

/// A group parameter of control surface group `g` (defaults as Logic's).
pub fn gx(app: &App, g: usize, key: &str) -> i32 {
    app.hub.surf.ext.gx.get(g).and_then(|m| m.get(key).copied()).unwrap_or(match key {
        "Filter for All View" => 0xff,
        "Send Slot" | "Insert Slot" | "Insert Slot (Split Lower)" | "EQ Band" => 1,
        "Send Parameter" | "EQ Parameter" => 1,
        _ => 0,
    })
}

pub fn set_gx(app: &mut App, g: usize, key: &str, v: i32) {
    let ext = &mut app.hub.surf.ext;
    while ext.gx.len() <= g {
        ext.gx.push(BTreeMap::new());
    }
    ext.gx[g].insert(key.to_string(), v);
}

/// The range a group parameter steps through (min, max).
fn gx_range(key: &str) -> (i32, i32) {
    match key {
        "Surround Parameter" => (0, cs::SURROUND_PARAMS.len() as i32 - 1),
        "EQ Band" => (1, 8),
        "EQ Parameter" => (0, cs::EQ_PARAMS.len() as i32 - 1),
        "Send Parameter" => (0, cs::SEND_PARAMS.len() as i32 - 1),
        "Send Slot" | "Insert Slot" | "Insert Slot (Split Lower)" => (1, 15),
        "Insert Type" | "Insert Type (Split Lower)" | "Open/Close Plug-in Window" | "Follows Focused View" | "Parameter Page Shift Mode" => (0, 1),
        "Filter for All View" => (0, 255),
        "Automation Group" => (0, 32),
        _ => (0, 9999),
    }
}

/// The group parameters surfaces.rs doesn't carry. True when handled.
pub fn group(app: &mut App, param: &str, a: &Assignment, v: i32) -> bool {
    let key = param.split('|').next().unwrap_or(param).to_string();
    if !cs::GROUP_PARAMS.contains(&key.as_str()) {
        return false;
    }
    let g = 0;
    let cur = gx(app, g, &key);
    let d = (v as f32 * a.multiply).round() as i32;
    let (lo, hi) = gx_range(&key);
    let nv = if key == "Filter for All View" {
        // the assignment's checkboxes: Direct shows those types, X-OR toggles them
        let mask = param.split('|').nth(1).and_then(|x| x.parse::<i32>().ok()).unwrap_or(0xff);
        if v == 0 && a.value_mode != ValueMode::Direct {
            return true;
        }
        if a.value_mode == ValueMode::XOr { cur ^ mask } else { mask }
    } else if key.starts_with("Fader Bank") || key.contains("Page") || key == "Live Loops Column Bank" {
        let by_one = gx(app, g, "Parameter Page Shift Mode") == 1 || app.hub.surf.groups[0].page_by_one;
        match a.value_mode {
            ValueMode::Relative | ValueMode::Rotate => cur + cs::bank_step(a.bank_type, by_one, 8, d),
            _ => d,
        }
    } else {
        match a.value_mode {
            ValueMode::Toggle if v != 0 => if cur == lo { hi.min(lo + 1) } else { lo },
            ValueMode::Toggle => return true,
            ValueMode::Relative => cur + d,
            ValueMode::Rotate => if cur + d > hi { lo } else if cur + d < lo { hi } else { cur + d },
            _ => d,
        }
    };
    let nv = nv.clamp(lo, hi);
    set_gx(app, g, &key, nv);
    // (the bank of the current view is the one surfaces.rs banks with)
    let view = app.hub.surf.groups[0].view as usize;
    let view_key = format!("Fader Bank for {} View", crate::surfaces::VIEWS[view.min(3)]);
    if key == view_key || (key.starts_with("Fader Bank for ") && view == 1 && gx(app, g, "Filter for All View").count_ones() == 1 && key.ends_with(cs::STRIP_TYPES[gx(app, g, "Filter for All View").trailing_zeros().min(7) as usize])) {
        app.hub.surf.groups[0].fader_bank = nv;
    }
    if key == "Send Slot" {
        app.hub.surf.groups[0].send_slot = nv;
    }
    if key == "Channel Strip Track" {
        app.selected_track = Some(nv.max(0) as usize);
    }
    if key == "Open/Close Plug-in Window" && nv == 1 {
        if let Some(t) = app.selected_track {
            let _ = t;
            app.tell("Plug-in windows follow the control surface");
        }
    }
    true
}

/// Fader Bank for Current View (Bank Type aware). Returns the new bank.
pub fn current_bank(app: &App, a: &Assignment, cur: i32, d: i32) -> i32 {
    let by_one = gx(app, 0, "Parameter Page Shift Mode") == 1 || app.hub.surf.groups[0].page_by_one;
    match a.value_mode {
        ValueMode::Relative | ValueMode::Rotate => (cur + cs::bank_step(a.bank_type, by_one, 8, d)).max(0),
        _ => d.max(0),
    }
}

fn marker_now(app: &App) -> Option<usize> {
    let pos = app.position() as u32;
    app.markers.iter().enumerate().filter(|(_, m)| m.0 <= pos).max_by_key(|(_, m)| m.0).map(|x| x.0)
}

/// The Global class parameters surfaces.rs doesn't carry out. True when handled.
pub fn global(app: &mut App, ctx: &egui::Context, param: &str, number: i32, v: i32, a: &Assignment) -> bool {
    let d = (v as f32 * a.multiply) as i64;
    let bar = app.bar_ticks() as i64;
    let ppq = lg_core::events::PPQ as f64;
    match param {
        "Move Punch Locators" | "Left Punch Locator" | "Right Punch Locator" => {
            let (l, r) = app.rec.punch;
            let (l, r) = (l as i64, r as i64);
            let delta = d * bar;
            let (nl, nr) = match param {
                "Move Punch Locators" => ((l + delta).max(0), (l + delta).max(0) + (r - l)),
                "Left Punch Locator" => ((l + delta).clamp(0, r - 1), r),
                _ => (l, (r + delta).max(l + 1)),
            };
            crate::recording::set_punch(app, (nl as u32, nr as u32));
        }
        "Marker Position" | "Marker Length" => {
            let Some(k) = marker_now(app) else { return true };
            let div = app.nav.division.max(1) as i64;
            let m = &mut app.markers[k];
            if param == "Marker Position" {
                m.0 = (m.0 as i64 + d * div).max(0) as u32;
            } else {
                m.2.len = (m.2.len.max(div as u32) as i64 + d * div).max(div) as u32;
            }
            app.markers.sort_by_key(|m| m.0);
            app.dirty = true;
        }
        "Nudge selected Regions/Events" => {
            if d != 0 {
                let c = if d > 0 { "Nudge Region/Event Position Right by Nudge Value" } else { "Nudge Region/Event Position Left by Nudge Value" };
                for _ in 0..d.unsigned_abs().min(64) {
                    let _ = app.key_command(ctx, c);
                }
            }
        }
        // (feedback only: lit while any track solos)
        "Any Solo" => {}
        "Nudge Value" => {
            if v != 0 {
                let (_, fps, _) = crate::projsettings::smpte(&app.proj);
                let t = cs::nudge_ticks(number.clamp(0, 5) as usize, ppq, bar as f64, app.nav.division as f64, fps, app.tempo());
                app.nudge = t.round().max(1.0) as u32;
            }
        }
        "Scrub Status" => {
            if v != 0 {
                app.hub.surf.ext.scrub = number.clamp(0, 2) as u8;
            }
        }
        "Playhead (Beats, Scrubbing)" => {
            // the controller value sets the speed: faster turns move further
            let p = app.position();
            let step = ppq / 16.0 * (d.unsigned_abs() as f64).max(1.0);
            app.locate((p + d.signum() as f64 * step * d.unsigned_abs() as f64).max(0.0));
        }
        "Shuttle Speed" => {
            // absolute values: 64 is stopped, above forwards, below backwards
            let s = if a.value_mode == ValueMode::Relative { app.hub.surf.ext.shuttle / ppq + d as f64 } else { (v - 64) as f64 / 8.0 };
            app.hub.surf.ext.shuttle = s * ppq;
            app.hub.surf.ext.shuttle_at = Some(Instant::now());
        }
        "Group Clutch" => {
            // a button: 1 while pressed (groups off), 0 released
            app.mix.groups_enabled = v == 0;
        }
        "Active Sense" => app.hub.surf.ext.sensed = Some(Instant::now()),
        "Alert Text" | "Alert Button" | "Alert Icon" => {
            if v != 0 {
                let t = if a.label.is_empty() { "Alert".to_string() } else { a.label.clone() };
                app.hub.surf.ext.alert = Some(t.clone());
                app.tell(t);
            } else {
                app.hub.surf.ext.alert = None;
            }
        }
        // (with Exclusive, an active Dummy disables the modeless assignment of the same control)
        "Dummy" => {}
        "Waveform Zoom" => {
            let z = &mut app.audio_edit.wave_zoom;
            *z = (*z * if d > 0 { 1.25 } else { 0.8 }).clamp(1.0, 64.0);
        }
        "Quantize Value" => {
            const Q: [u32; 7] = [3840, 1920, 960, 480, 240, 120, 60];
            let k = Q.iter().position(|q| *q == app.quantize_grid).unwrap_or(3) as i64;
            app.quantize_grid = Q[(k + d.signum()).clamp(0, 6) as usize];
        }
        "Division" => {
            let _ = app.key_command(ctx, if d > 0 { "Set Next Higher Division" } else { "Set Next Lower Division" });
        }
        _ => return false,
    }
    true
}

/// Automation group assignments: a group parameter (Volume, Pan, Mute, Solo, Automation …)
/// of every member of the group (0: the group the control surface group selects).
pub fn automation_group(app: &mut App, a: &Assignment, param: &str, group: i32, v: i32) {
    let g = if group == 0 { gx(app, 0, "Automation Group") } else { group };
    if g <= 0 {
        return;
    }
    let gi = (g - 1) as usize;
    let members: Vec<usize> = app.mix.group_of.iter().filter(|(_, x)| **x == gi).map(|(s, _)| *s).filter(|s| *s < crate::mixer::AUX).collect();
    let p = lg_core::ctrlsurf::StripParam::parse(param);
    for t in members {
        crate::surfaces::run_on_track(app, t, &p, a, v);
    }
}

/// A message into assignment `i`: remembered as touched; Key Repeat starts on the press
/// (Maximum) and stops on the release (Minimum).
pub fn input(app: &mut App, i: usize, a: &Assignment, v: i32) {
    app.hub.surf.ext.touched.insert(i, Instant::now());
    if a.key_repeat {
        app.hub.surf.ext.repeat = if v == a.max { Some((i, Instant::now() + Duration::from_millis(500))) } else { None };
    }
}

/// Key Repeat turned on in the inspector: the message is cleared and learned again (press,
/// then release: the release value goes to Minimum, the press value to Maximum).
pub fn start_relearn(app: &mut App, i: usize) {
    if let Some(a) = app.hub.surf.assign.list.get_mut(i) {
        a.msg.clear();
    }
    app.hub.surf.ext.relearn = Some((i, None));
    app.hub.surf.learn = true;
    app.tell("Key Repeat: press and release the button to learn its message");
}

/// Learn while re-learning for Key Repeat. True when taken.
pub fn relearn(app: &mut App, bytes: &[u8]) -> bool {
    let Some((i, press)) = app.hub.surf.ext.relearn else { return false };
    let pat = cs::learn_pattern(bytes);
    let val = cs::match_msg(&pat, bytes).map_or(0, |x| x.0);
    match press {
        None => {
            if let Some(a) = app.hub.surf.assign.list.get_mut(i) {
                a.msg = pat;
            }
            app.hub.surf.ext.relearn = Some((i, Some(val)));
        }
        Some(p) => {
            if let Some(a) = app.hub.surf.assign.list.get_mut(i) {
                a.max = p;
                a.min = val;
            }
            app.hub.surf.ext.relearn = None;
            app.hub.surf.learn = false;
            if p == val {
                app.tell("The press and release values are the same: the key command assignment won't work");
            } else {
                app.tell("Key Repeat learned");
            }
        }
    }
    true
}

/// Each frame: Key Repeat, shuttling, and feedback for learned assignments.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    // Key Repeat: after the delay, at the key repeat rate until released
    if let Some((i, at)) = app.hub.surf.ext.repeat {
        if Instant::now() >= at {
            if let Some(a) = app.hub.surf.assign.list.get(i).cloned() {
                crate::surfaces::run(app, ctx, &a, a.max);
            }
            app.hub.surf.ext.repeat = Some((i, Instant::now() + Duration::from_millis(60)));
        }
        ctx.request_repaint_after(Duration::from_millis(30));
    }
    // Shuttle: the playhead moves at the shuttle speed while stopped
    let sh = app.hub.surf.ext.shuttle;
    if sh != 0.0 && !app.playing() {
        let now = Instant::now();
        let dt = app.hub.surf.ext.shuttle_at.map_or(0.0, |t| now.duration_since(t).as_secs_f64());
        app.hub.surf.ext.shuttle_at = Some(now);
        let p = app.position();
        app.locate((p + sh * dt).max(0.0));
        ctx.request_repaint_after(Duration::from_millis(30));
    }
    // feedback: each assignment's value in its Feedback Format, to the surface's output
    let n = app.hub.surf.assign.list.len();
    let outs = app.hub.outs.ports();
    for i in 0..n {
        if !app.hub.surf.assign.is_active(i) {
            continue;
        }
        let a = app.hub.surf.assign.list[i].clone();
        if a.local_feedback && app.hub.surf.ext.touched.get(&i).is_some_and(|t| t.elapsed() < Duration::from_millis(400)) {
            continue;
        }
        let Some(v) = value01(app, &a) else { continue };
        let Some(msg) = crate::surfpref::feedback_bytes(&a, v) else { continue };
        let port = app.hub.surf.devices.iter().find(|d| d.in_port == a.port).map(|d| d.out_port.clone()).or_else(|| outs.iter().find(|o| **o == a.port).cloned());
        let Some(port) = port.filter(|p| !p.is_empty()) else { continue };
        if app.hub.surf.ext.sent.get(&i) != Some(&msg) {
            app.hub.outs.send(&port, &msg);
            app.hub.surf.ext.sent.insert(i, msg);
        }
    }
}

/// An assignment's current value 0–1 (for feedback), when it has one.
pub fn value01(app: &App, a: &Assignment) -> Option<f32> {
    match &a.class {
        Class::Strip { strip, param } => crate::surfaces::strip_value(app, strip, param),
        Class::Global { param, .. } => Some(match param.as_str() {
            "Cycle" => app.cycle_on() as u8 as f32,
            "Autopunch" => app.autopunch as u8 as f32,
            "Any Solo" => app.project.as_ref().is_some_and(|p| p.tracks.iter().any(|t| t.solo)) as u8 as f32,
            "Group Clutch" => (!app.mix.groups_enabled) as u8 as f32,
            _ => return None,
        }),
        Class::KeyCommand(c) => Some(crate::menus::checked(app, c) as u8 as f32),
        Class::Group { param } => {
            let (lo, hi) = gx_range(param);
            cs::GROUP_PARAMS.contains(&param.as_str()).then(|| (gx(app, 0, param) - lo) as f32 / (hi - lo).max(1) as f32)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lg_core::ctrlsurf::parse_msg;

    #[test]
    fn group_params_globals_and_relearn() {
        let mut app = crate::tm::test_app();
        let ctx = egui::Context::default();
        // Filter for All View: Direct sets the types, X-OR toggles them
        let mut a = Assignment::learned("P", parse_msg("B0 10 Lo7"), Class::Group { param: "Filter for All View|12".into() });
        a.value_mode = ValueMode::Direct;
        assert!(group(&mut app, "Filter for All View|12", &a, 127));
        assert_eq!(gx(&app, 0, "Filter for All View"), 12);
        a.value_mode = ValueMode::XOr;
        group(&mut app, "Filter for All View|4", &a, 127);
        assert_eq!(gx(&app, 0, "Filter for All View"), 8);
        // a relative EQ band stays in 1–8
        a.value_mode = ValueMode::Relative;
        group(&mut app, "EQ Band", &a, 20);
        assert_eq!(gx(&app, 0, "EQ Band"), 8);
        // By Bank banking moves eight strips
        a.bank_type = 2;
        group(&mut app, "Fader Bank for Tracks View", &a, 1);
        assert_eq!(gx(&app, 0, "Fader Bank for Tracks View"), 8);
        // punch locators by bars; Group Clutch while pressed
        let bar = app.bar_ticks();
        crate::recording::set_punch(&mut app, (bar, 3 * bar));
        let g = Assignment::learned("P", parse_msg("B0 11 Lo7"), Class::Global { param: "Move Punch Locators".into(), number: 0 });
        assert!(global(&mut app, &ctx, "Move Punch Locators", 0, 1, &g));
        assert_eq!(app.rec.punch, (2 * bar, 4 * bar));
        global(&mut app, &ctx, "Group Clutch", 0, 1, &g);
        assert!(!app.mix.groups_enabled);
        global(&mut app, &ctx, "Group Clutch", 0, 0, &g);
        assert!(app.mix.groups_enabled);
        global(&mut app, &ctx, "Nudge Value", 3, 1, &g);
        assert_eq!(app.nudge, bar);
        // Key Repeat's re-learn: press then release
        app.hub.surf.assign.list.push(Assignment::learned("P", parse_msg("90 3C Lo7"), Class::KeyCommand("Zoom Horizontal In".into())));
        start_relearn(&mut app, 0);
        assert!(app.hub.surf.assign.list[0].msg.is_empty());
        assert!(relearn(&mut app, &[0x90, 0x3C, 127]));
        assert!(relearn(&mut app, &[0x90, 0x3C, 0]));
        let r = &app.hub.surf.assign.list[0];
        assert_eq!((r.min, r.max), (0, 127));
        assert!(app.hub.surf.ext.relearn.is_none());
    }
}
