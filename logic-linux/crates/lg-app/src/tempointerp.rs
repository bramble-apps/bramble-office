//! The Tempo Interpreter window (Edit ▸ Tempo ▸ Tempo Interpreter…, Open Tempo Interpreter…):
//! Tap Step, Window, Tempo Response, Max. Tempo Change, Tap Count-In, Smoothing, Tempo
//! Recording and the Pre / Post tap display (lg-core tempointerp). While the window is open,
//! or the Sync Mode is Manual, Tap Tempo goes through it: the taps count in, then steer the
//! tempo; in record mode with Tempo Recording on they write tempo changes at the playhead.
//! A tap flashes yellow when in range, red when not. With Window 0 playback stops when the
//! taps stop.

use egui::{pos2, vec2, Color32, Rect, Sense, Stroke};
use lg_core::tempointerp::{Interpreter, Params, Tap, STEPS};

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &["Tempo Interpreter…", "Open Tempo Interpreter…"];

#[derive(Clone, Debug, Default)]
pub struct State {
    pub open: bool,
    pub params: Params,
    pub interp: Option<Interpreter>,
    /// The last tap: (when, in range) for the flash.
    pub flash: Option<(f64, bool)>,
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// Whether Tap Tempo goes through the interpreter.
pub fn active(app: &App) -> bool {
    app.ti.open || app.proj.get("Synchronization / General / Sync Mode:") == "Manual"
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Tempo Interpreter…" | "Open Tempo Interpreter…" => {
            app.ti.open = true;
            true
        }
        "Tap Tempo" if active(app) => {
            tap(app, now());
            true
        }
        _ => false,
    }
}

/// One tap at `t` seconds.
pub fn tap(app: &mut App, t: f64) {
    let tempo = app.tempo();
    let params = app.ti.params;
    let i = app.ti.interp.get_or_insert_with(|| Interpreter::new(tempo));
    let r = i.tap(&params, t);
    app.ti.flash = Some((t, !matches!(r, Tap::Rejected)));
    let bpm = match r {
        Tap::CountIn(_, Some(b)) | Tap::Accepted(b) => b,
        _ => return,
    };
    if params.recording && app.recording() {
        let at = app.position() as u32;
        if let Some(p) = app.project.as_mut() {
            lg_core::timeline::set_point(p, at, (bpm * 100.0).round() / 100.0);
        }
        app.after_edit();
    } else {
        app.set_tempo(bpm);
    }
}

/// Each frame: the window; Window 0 halts playback when the taps stop.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    if let Some(i) = &app.ti.interp {
        if app.playing() && i.stalled(&app.ti.params, now()) {
            app.stop();
            app.ti.interp = None;
        }
    }
    if !app.ti.open {
        return;
    }
    let mut open = true;
    let mut p = app.ti.params;
    let mut reset = false;
    egui::Window::new("Tempo Interpreter").id(egui::Id::unique("tempo-interpreter")).open(&mut open).resizable(false).default_width(300.0).show(ctx, |ui| {
        egui::Grid::new("ti-grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label("Tap Step:");
            let cur = STEPS.iter().find(|s| s.1 == p.tap_step).map_or("1/4", |s| s.0);
            egui::ComboBox::from_id_salt("ti-step").selected_text(cur).width(110.0).show_ui(ui, |ui| {
                for (n, v) in STEPS {
                    ui.selectable_value(&mut p.tap_step, v, n);
                }
            });
            ui.end_row();
            ui.label("Window:");
            ui.add(egui::DragValue::new(&mut p.window).range(0..=3840).suffix(" Ticks"));
            ui.end_row();
            ui.label("Tempo Response:");
            ui.add(egui::DragValue::new(&mut p.response).range(1..=8));
            ui.end_row();
            ui.label("Max. Tempo Change:");
            ui.add(egui::DragValue::new(&mut p.max_change).range(0.5..=100.0).speed(0.5).suffix(" bpm"));
            ui.end_row();
            ui.label("Tap Count-In:");
            ui.add(egui::DragValue::new(&mut p.count_in).range(1..=16));
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.checkbox(&mut p.smoothing, "Smoothing");
        ui.checkbox(&mut p.recording, "Tempo Recording");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::selectable(p.pre, "Pre")).on_hover_text("Show every incoming tap").clicked() {
                p.pre = true;
            }
            if ui.add(egui::Button::selectable(!p.pre, "Post")).on_hover_text("Show only the taps used").clicked() {
                p.pre = false;
            }
            if ui.button("Reset").on_hover_text("Count in again").clicked() {
                reset = true;
            }
        });
        // the tap display: the latest taps, yellow in range, red out of range
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().max(260.0), 26.0), Sense::hover());
        let pa = ui.painter();
        pa.rect_filled(r, 4.0, Color32::from_gray(0x1c));
        let t_now = now();
        if let Some(i) = &app.ti.interp {
            for (t, ok) in &i.shown {
                let age = (t_now - t) as f32;
                if age > 6.0 {
                    continue;
                }
                let x = r.max.x - 6.0 - age / 6.0 * (r.width() - 12.0);
                let c = if *ok { Color32::from_rgb(0xf2, 0xcd, 0x3c) } else { Color32::from_rgb(0xe5, 0x48, 0x3c) };
                pa.vline(x, r.shrink(4.0).y_range(), Stroke::new(2.0, c.gamma_multiply((1.0 - age / 6.0).max(0.2))));
            }
            let text = if i.following(&p) { format!("{:.2} bpm", i.tempo) } else { "Counting in…".to_string() };
            pa.text(pos2(r.min.x + 8.0, r.center().y), egui::Align2::LEFT_CENTER, text, look::regular(11.0), look::text_dim());
        }
        if let Some((t, ok)) = app.ti.flash {
            if t_now - t < 0.15 {
                let c = if ok { Color32::from_rgb(0xf2, 0xcd, 0x3c) } else { Color32::from_rgb(0xe5, 0x48, 0x3c) };
                ui.painter().circle_filled(Rect::from_min_size(r.right_top() + vec2(-14.0, 7.0), vec2(8.0, 8.0)).center(), 5.0, c);
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    });
    app.ti.params = p;
    if reset {
        app.ti.interp = None;
    }
    if !open {
        app.ti.open = false;
        app.ti.interp = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_steer_the_tempo_while_open() {
        let ctx = egui::Context::default();
        let mut app = App::new(&eframe::CreationContext::_new_kittest(ctx), None);
        app.set_project(crate::demo::song(), true);
        assert!(!active(&app));
        assert!(command(&mut app, "Open Tempo Interpreter…"));
        assert!(active(&app));
        for n in 0..4 {
            tap(&mut app, n as f64 * 0.6);
        }
        assert!((app.tempo() - 100.0).abs() < 0.05, "{}", app.tempo());
        tap(&mut app, 1.8 + 0.58);
        assert!(app.tempo() > 100.0);
        assert_eq!(app.ti.flash.map(|f| f.1), Some(true));
        tap(&mut app, 2.38 + 0.95);
        assert_eq!(app.ti.flash.map(|f| f.1), Some(false));
    }
}
