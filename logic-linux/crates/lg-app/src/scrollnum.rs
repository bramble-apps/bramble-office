//! Settings ▸ View ▸ Tracks ▸ Show track or bar number while scrolling: while the Tracks area
//! scrolls, a centred overlay shows a number with "Track" or "Bar" under it — scrolling down
//! the highest visible track number, up the lowest; right the highest visible bar, left the
//! lowest.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use egui::{pos2, vec2, Align2, Color32, Rect};

use crate::app::App;

pub const COMMANDS: &[&str] = &[];

pub const KEY: &str = "View / Tracks / Show track or bar number while scrolling";
const SHOW_FOR: Duration = Duration::from_millis(900);

/// The number shown for a scroll by (`dx`, `dy`) points (egui's: negative y scrolls down,
/// negative x right) with tracks `first..=last` and bars `first_bar..=last_bar` in view (1-based).
pub fn number(dx: f32, dy: f32, tracks: (usize, usize), bars: (usize, usize)) -> Option<(usize, &'static str)> {
    if dy.abs() >= dx.abs() && dy != 0.0 {
        Some((if dy < 0.0 { tracks.1 } else { tracks.0 }, "Track"))
    } else if dx != 0.0 {
        Some((if dx < 0.0 { bars.1 } else { bars.0 }, "Bar"))
    } else {
        None
    }
}

thread_local! {
    static SHOWN: RefCell<Option<(usize, &'static str, Instant)>> = const { RefCell::new(None) };
}

/// A scroll in the Tracks area (`lanes`: the lanes' rect, after the scroll was applied).
pub fn scrolled(app: &App, lanes: Rect, dx: f32, dy: f32) {
    if !app.logic_settings.on(KEY) || (dx == 0.0 && dy == 0.0) {
        return;
    }
    let rows = crate::trackarea::rows(app);
    let top = app.scroll_y;
    let bottom = top + lanes.height();
    let vis: Vec<usize> = rows.iter().filter(|r| r.y + r.h > top && r.y < bottom).map(|r| r.t + 1).collect();
    let tracks = (vis.iter().copied().min().unwrap_or(1), vis.iter().copied().max().unwrap_or(1));
    let first_bar = app.scroll_x.max(0.0).floor() as usize + 1;
    let last_bar = (app.scroll_x.max(0.0) + lanes.width() / app.bar_w.max(1.0)).floor() as usize + 1;
    if let Some((n, what)) = number(dx, dy, tracks, (first_bar, last_bar)) {
        SHOWN.with(|s| *s.borrow_mut() = Some((n, what, Instant::now())));
    }
}

/// Draws the overlay (centred on the lanes) while it lasts.
pub fn draw(ui: &egui::Ui, lanes: Rect) {
    let Some((n, what, at)) = SHOWN.with(|s| *s.borrow()) else { return };
    if at.elapsed() > SHOW_FOR {
        SHOWN.with(|s| *s.borrow_mut() = None);
        return;
    }
    ui.ctx().request_repaint_after(Duration::from_millis(100));
    let p = ui.painter_at(lanes);
    let r = Rect::from_center_size(lanes.center(), vec2(120.0, 84.0));
    p.rect_filled(r, 12.0, Color32::from_black_alpha(170));
    p.text(pos2(r.center().x, r.min.y + 36.0), Align2::CENTER_CENTER, n.to_string(), egui::FontId::proportional(40.0), Color32::WHITE);
    p.text(pos2(r.center().x, r.max.y - 14.0), Align2::CENTER_CENTER, what, egui::FontId::proportional(13.0), Color32::from_gray(210));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_number() {
        // down: the highest visible track; up: the lowest
        assert_eq!(number(0.0, -30.0, (4, 12), (1, 9)), Some((12, "Track")));
        assert_eq!(number(0.0, 30.0, (4, 12), (1, 9)), Some((4, "Track")));
        // right: the highest visible bar; left: the lowest
        assert_eq!(number(-30.0, 0.0, (4, 12), (5, 9)), Some((9, "Bar")));
        assert_eq!(number(30.0, 0.0, (4, 12), (5, 9)), Some((5, "Bar")));
        assert_eq!(number(0.0, 0.0, (4, 12), (5, 9)), None);
    }
}
