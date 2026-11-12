//! Editing on the Tempo and Signature tracks beyond dragging (lg-core tempoclip): Copy / Paste
//! of tempo points and signatures (pasted at the playhead, the first copied one there; kept
//! across projects), Select All Inside Locators and Pickup Clock for tempo points, Delete,
//! the Eraser, Set Time For Last Selected Tempo (Logic's Set Time dialog for a video hit
//! point), ⌃⌥⌘-click to type a tempo, Shift-double-click for the Tempo / Signature List
//! (⌥⇧: the Tempo List as its own window), and on the Signature track: click / Shift-click
//! selection, Option-drag copies, the Scissors split a bar's signature, the Join tool merges.

use egui::{pos2, vec2, Rect, Sense};
use lg_core::tempoclip as tc;
use lg_core::timeline::{self, Map};

use crate::app::{App, RightPane};
use crate::edit::Tool;

pub const COMMANDS: &[&str] = &["Tempo Track: Set Time For Last Selected Tempo"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    None,
    Tempo,
    Signature,
}

#[derive(Clone, Debug)]
pub struct SetTime {
    /// Playhead Position (true) or Manual Position.
    pub playhead: bool,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub focus: Focus,
    pub tempo_clip: Vec<(u32, f64)>,
    pub sig_clip: Vec<(u32, u8, u8)>,
    /// Selected signatures (bars).
    pub sig_sel: Vec<u32>,
    pub set_time: Option<SetTime>,
    /// ⌃⌥⌘-click: the tempo typed for a new point (tick, text).
    pub typing: Option<(u32, String)>,
    /// An Option-drag copy of a signature: its bar and value.
    sig_copy: Option<(u32, (u8, u8))>,
}

/// Each global lane reports presses so Copy / Paste / Delete know which track they're for.
pub fn lane_focus(app: &mut App, ui: &mut egui::Ui, lane: Rect, kind: Focus, tick_of: &dyn Fn(f32) -> f32) {
    let (pressed, hover, m) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.hover_pos(), i.modifiers));
    let inside = hover.is_some_and(|h| lane.contains(h));
    if pressed {
        if inside {
            app.gc.focus = kind;
        } else if app.gc.focus == kind {
            app.gc.focus = Focus::None;
        }
    }
    if !inside {
        return;
    }
    let dbl = ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary));
    if dbl && m.shift {
        match kind {
            Focus::Tempo if m.alt => app.arr.tempo_list = true,
            Focus::Tempo => {
                app.right_pane = Some(RightPane::Lists);
                app.list_tab = 2;
            }
            Focus::Signature => {
                app.right_pane = Some(RightPane::Lists);
                app.list_tab = 3;
            }
            Focus::None => {}
        }
    }
    let clicked = ui.input(|i| i.pointer.primary_clicked());
    let Some(h) = hover else { return };
    let tick = tick_of(h.x).max(0.0);
    if kind == Focus::Tempo && clicked {
        if m.ctrl && m.alt && m.command {
            let bpm = app.project.as_ref().map_or(120.0, |p| Map::of(p).bpm_at(tick as f64));
            app.gc.typing = Some((tick as u32, format!("{bpm:.2}")));
        } else if app.tool == Tool::Eraser {
            // the point nearest the click (within a few points)
            let ppt = (tick_of(h.x + 5.0) - tick).abs();
            let near = app.project.as_ref().and_then(|p| p.tempo_map.iter().map(|x| x.0).filter(|&t| t > 0).min_by_key(|&t| (t as f32 - tick).abs() as u32)).filter(|&t| (t as f32 - tick).abs() <= ppt);
            if let Some(t) = near {
                app.checkpoint("Delete Tempo Point");
                if let Some(p) = app.project.as_mut() {
                    timeline::delete_point(p, t);
                }
                app.arr.tempo_sel.retain(|&s| s != t);
                app.after_edit();
            }
        }
    }
    if kind == Focus::Signature && clicked && app.tool == Tool::Scissors {
        if let Some(p) = &app.project {
            let m = Map::of(p);
            let (bar, start, sig) = m.bar_of(tick as u32);
            let beat_len = 3840 / sig.1.max(1) as u32;
            let beats = ((tick as u32 - start) / beat_len.max(1)) as u8;
            app.checkpoint("Split Signature");
            let ok = app.project.as_mut().is_some_and(|p| tc::split_signature(p, bar.max(1), beats));
            if ok {
                app.after_edit();
            } else {
                app.undo.pop();
            }
        }
    }
}

/// A signature on the Signature track: click / Shift-click selects, the Eraser deletes, the
/// Join tool merges the selection, Option-drag copies. True when the press was used here.
pub fn sig_item(app: &mut App, ui: &egui::Ui, resp: &egui::Response, bar: u32, key: bool, painter: &egui::Painter, r: Rect) -> bool {
    if app.gc.sig_sel.contains(&bar) && !key {
        painter.rect_stroke(r, 3.0, egui::Stroke::new(1.5, crate::look::accent()), egui::StrokeKind::Inside);
    }
    if key {
        return false;
    }
    let m = ui.input(|i| i.modifiers);
    if resp.clicked() {
        match app.tool {
            Tool::Eraser => {
                app.checkpoint("Delete Signature");
                if let Some(p) = app.project.as_mut() {
                    timeline::delete_signature(p, bar);
                }
                app.gc.sig_sel.retain(|&b| b != bar);
                app.after_edit();
                return true;
            }
            Tool::Glue => {
                if !app.gc.sig_sel.contains(&bar) {
                    app.gc.sig_sel.push(bar);
                }
                if m.shift && app.gc.sig_sel.len() >= 2 {
                    let sel = app.gc.sig_sel.clone();
                    app.checkpoint("Join Signatures");
                    if app.project.as_mut().is_some_and(|p| tc::join_signatures(p, &sel)) {
                        app.gc.sig_sel = vec![sel.iter().copied().min().unwrap_or(bar)];
                        app.after_edit();
                    } else {
                        app.undo.pop();
                    }
                }
                return true;
            }
            _ => {
                if m.shift {
                    if !app.gc.sig_sel.contains(&bar) {
                        app.gc.sig_sel.push(bar);
                    }
                } else {
                    app.gc.sig_sel = vec![bar];
                }
                app.gc.focus = Focus::Signature;
            }
        }
    }
    if resp.drag_started() && m.alt {
        let sig = app.project.as_ref().map_or((4, 4), |p| Map::of(p).sig_of_bar(bar));
        app.gc.sig_copy = Some((bar, sig));
    }
    if resp.drag_stopped() {
        if let Some((b, s)) = app.gc.sig_copy.take() {
            if let Some(p) = app.project.as_mut() {
                if !p.global.signatures.iter().any(|x| x.0 == b) {
                    timeline::set_signature(p, b, s);
                }
            }
        }
    }
    false
}

pub fn command(app: &mut App, name: &str) -> bool {
    let pos = app.position() as u32;
    match (app.gc.focus, name) {
        (_, "Tempo Track: Set Time For Last Selected Tempo" | "Set Time For Last Selected Tempo Event…") => {
            app.gc.set_time = Some(SetTime { playhead: true, text: String::new() });
            true
        }
        (Focus::Tempo, "Copy") if !app.arr.tempo_sel.is_empty() => {
            if let Some(p) = &app.project {
                app.gc.tempo_clip = tc::copy_points(p, &app.arr.tempo_sel);
            }
            true
        }
        (Focus::Tempo, "Cut") if !app.arr.tempo_sel.is_empty() => {
            command(app, "Copy");
            command(app, "Delete")
        }
        (Focus::Tempo, "Paste") if !app.gc.tempo_clip.is_empty() => {
            app.checkpoint("Paste Tempo Points");
            let clip = app.gc.tempo_clip.clone();
            if let Some(p) = app.project.as_mut() {
                app.arr.tempo_sel = tc::paste_points(p, &clip, pos);
            }
            app.after_edit();
            true
        }
        (Focus::Tempo, "Delete") if !app.arr.tempo_sel.is_empty() => {
            app.checkpoint("Delete Tempo Points");
            let sel = std::mem::take(&mut app.arr.tempo_sel);
            if let Some(p) = app.project.as_mut() {
                for t in sel {
                    timeline::delete_point(p, t);
                }
            }
            app.after_edit();
            true
        }
        (Focus::Tempo, "Select All Inside Locators" | "Inside Locators") => {
            let (a, b) = app.cycle;
            if let Some(p) = &app.project {
                app.arr.tempo_sel = tc::points_inside(p, a, b);
            }
            true
        }
        (Focus::Tempo, "Move Region/Event to Playhead Position (Pickup Clock)") if app.arr.tempo_sel.len() == 1 => {
            let from = app.arr.tempo_sel[0];
            app.checkpoint("Pickup Clock");
            if app.project.as_mut().is_some_and(|p| timeline::move_point(p, from, pos)) {
                app.arr.tempo_sel = vec![pos];
            }
            app.after_edit();
            true
        }
        (Focus::Signature, "Copy") if !app.gc.sig_sel.is_empty() => {
            if let Some(p) = &app.project {
                app.gc.sig_clip = tc::copy_signatures(p, &app.gc.sig_sel);
            }
            true
        }
        (Focus::Signature, "Paste") if !app.gc.sig_clip.is_empty() => {
            app.checkpoint("Paste Signatures");
            let clip = app.gc.sig_clip.clone();
            if let Some(p) = app.project.as_mut() {
                let bar = Map::of(p).bar_of(pos).0;
                app.gc.sig_sel = tc::paste_signatures(p, &clip, bar);
            }
            app.after_edit();
            true
        }
        (Focus::Signature, "Delete") if !app.gc.sig_sel.is_empty() => {
            app.checkpoint("Delete Signatures");
            let sel = std::mem::take(&mut app.gc.sig_sel);
            if let Some(p) = app.project.as_mut() {
                for b in sel {
                    timeline::delete_signature(p, b);
                }
            }
            app.after_edit();
            true
        }
        _ => false,
    }
}

/// A time typed in the Set Time dialog: seconds, m:ss.mmm, h:mm:ss.mmm, or SMPTE hh:mm:ss:ff
/// (as pasted from the LCD; `fps` for the frames), measured from the project start.
pub fn parse_time(s: &str, fps: f64) -> Option<f64> {
    let s = s.trim();
    let parts: Vec<&str> = s.split(':').collect();
    let num = |x: &str| x.trim().parse::<f64>().ok();
    match parts.len() {
        1 => num(parts[0]),
        2 => Some(num(parts[0])? * 60.0 + num(parts[1])?),
        3 => Some(num(parts[0])? * 3600.0 + num(parts[1])? * 60.0 + num(parts[2])?),
        4 => {
            // SMPTE: frames may carry .subframes
            Some(num(parts[0])? * 3600.0 + num(parts[1])? * 60.0 + num(parts[2])? + num(parts[3])? / fps)
        }
        _ => None,
    }
}

/// Each frame: the Set Time dialog and the typed-tempo field.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    if let Some(mut d) = app.gc.set_time.clone() {
        let mut open = true;
        let mut go = false;
        let mut cancel = false;
        egui::Window::new("Set Time For Last Selected Tempo").id(egui::Id::unique("set-time")).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            ui.label("Set the hit point the last selected tempo event moves to:");
            ui.radio_value(&mut d.playhead, true, "Playhead Position");
            ui.radio_value(&mut d.playhead, false, "Manual Position");
            ui.horizontal(|ui| {
                ui.label("Time Position:");
                ui.add_enabled(!d.playhead, egui::TextEdit::singleline(&mut d.text).hint_text("01:00:12:10 or 12.5").desired_width(160.0));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                if ui.button("Set").clicked() {
                    go = true;
                }
            });
        });
        app.gc.set_time = Some(d.clone());
        if go {
            let secs = if d.playhead { app.project.as_ref().map(|p| Map::of(p).secs_at(app.position())) } else { parse_time(&d.text, 25.0) };
            let sel = app.arr.tempo_sel.clone();
            match secs {
                Some(s) => {
                    app.checkpoint("Set Time");
                    if app.project.as_mut().is_some_and(|p| tc::set_time_for_last(p, &sel, s)) {
                        app.after_edit();
                    } else {
                        app.undo.pop();
                        app.tell("Select two or more tempo points; the time must be after the first one");
                    }
                }
                None => app.tell("That time couldn't be read"),
            }
            app.gc.set_time = None;
        }
        if cancel || !open {
            app.gc.set_time = None;
        }
    }
    if let Some((tick, mut text)) = app.gc.typing.clone() {
        let mut done = None;
        egui::Area::new(egui::Id::unique("tempo-type")).order(egui::Order::Foreground).fixed_pos(ctx.input(|i| i.pointer.latest_pos()).unwrap_or(pos2(200.0, 200.0)) + vec2(8.0, 8.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let te = ui.add(egui::TextEdit::singleline(&mut text).desired_width(70.0));
                crate::widgets::focus_once(ui, &te);
                if te.lost_focus() {
                    done = Some(ui.input(|i| i.key_pressed(egui::Key::Escape)));
                }
                let _ = ui.allocate_response(vec2(0.0, 0.0), Sense::hover());
            });
        });
        match done {
            Some(esc) => {
                app.gc.typing = None;
                if let (false, Ok(bpm)) = (esc, text.trim().parse::<f64>()) {
                    app.checkpoint("Create Tempo Point");
                    if let Some(p) = app.project.as_mut() {
                        timeline::set_point(p, tick, bpm);
                    }
                    app.arr.tempo_sel = vec![tick];
                    app.after_edit();
                }
            }
            None => app.gc.typing = Some((tick, text)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_typed_or_pasted() {
        assert_eq!(parse_time("12.5", 25.0), Some(12.5));
        assert_eq!(parse_time("1:02.5", 25.0), Some(62.5));
        assert_eq!(parse_time("00:00:10:05", 25.0), Some(10.2));
        assert_eq!(parse_time("x", 25.0), None);
    }

    #[test]
    fn copy_paste_tempo_points_at_the_playhead() {
        let ctx = egui::Context::default();
        let mut app = App::new(&eframe::CreationContext::_new_kittest(ctx), None);
        app.set_project(crate::demo::song(), true);
        if let Some(p) = app.project.as_mut() {
            timeline::set_point(p, 3840, 100.0);
        }
        app.gc.focus = Focus::Tempo;
        app.arr.tempo_sel = vec![3840];
        assert!(command(&mut app, "Copy"));
        app.locate(7680.0);
        assert!(command(&mut app, "Paste"));
        let at = app.position() as u32;
        assert!(app.project.as_ref().unwrap().tempo_map.iter().any(|x| x.0 == at && (x.1 - 100.0).abs() < 1e-9));
        assert_eq!(app.arr.tempo_sel, vec![at]);
        app.undo();
        assert!(!app.project.as_ref().unwrap().tempo_map.iter().any(|x| x.0 == at && at != 3840));
    }
}
