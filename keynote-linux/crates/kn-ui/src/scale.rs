//! Settings > Scaling (all three apps): the size of everything on screen. Default follows the
//! system; a percentage is points per screen pixel (100 % = one pixel a point, 200 % = a
//! HiDPI screen's usual size).

/// The choices after Default, in percent.
pub const CHOICES: [u32; 10] = [50, 75, 90, 100, 110, 125, 150, 175, 200, 250];

fn label(pct: u32) -> String {
    if pct == 0 { "Default".into() } else { format!("{pct}%") }
}

/// The Scaling drop-down; true when the choice changed. `pct` 0 is Default.
pub fn combo(ui: &mut egui::Ui, id: &str, pct: &mut u32) -> bool {
    let before = *pct;
    crate::theme::combo(id).selected_text(label(*pct)).show_ui(ui, |ui| {
        ui.selectable_value(pct, 0, label(0));
        for p in CHOICES {
            ui.selectable_value(pct, p, label(p));
        }
    });
    *pct != before
}

/// Keeps the window at the chosen scale (cheap: only sets it when it differs).
pub fn apply(ctx: &egui::Context, pct: u32) {
    let native = ctx.native_pixels_per_point().unwrap_or(1.0).max(0.1);
    let want = if pct == 0 { 1.0 } else { pct as f32 / 100.0 / native };
    if (ctx.zoom_factor() - want).abs() > 1e-3 {
        ctx.set_zoom_factor(want);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn labels() {
        assert_eq!(super::label(0), "Default");
        assert_eq!(super::label(125), "125%");
    }
}
