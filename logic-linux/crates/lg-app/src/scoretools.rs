//! The Score Editor's menu bar controls right of its local menus (Logic: the Link button, the
//! left-click and Command-click Tool menus, the zoom slider), as in Logic's Score.

use crate::glyphs;
use crate::look;
use crate::score::{self, State, View, TOOLS};
use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

/// The tools' icons, in `TOOLS` order.
pub const ICONS: [&str; 9] = [glyphs::ph_fill::CURSOR, glyphs::ph::PENCIL_SIMPLE, glyphs::ph::ERASER, glyphs::ph::TEXT_T, glyphs::ph::LAYOUT, glyphs::ph::MAGNIFYING_GLASS, glyphs::ph::ARROWS_SPLIT, glyphs::ph::HEADPHONES, glyphs::ph::CAMERA];

const H: f32 = 24.0;

/// Draws the Link button and the Tool menus from `x`, and the zoom slider at the right edge.
pub fn bar(ui: &mut egui::Ui, bar: Rect, x: f32, st: &mut State) {
    let p = ui.painter().clone();
    let y = bar.center().y;
    // Link: purple Same Level, yellow Content (double-click), grey off
    let lr = Rect::from_center_size(pos2(x + 24.0, y), vec2(48.0, H));
    let resp = ui.interact(lr, ui.scope_id().with("sc-link"), Sense::click()).on_hover_text("Link: click for Same Level, double-click for Content");
    if resp.double_clicked() {
        st.link = 2;
    } else if resp.clicked() {
        st.link = if st.link == 0 { 1 } else { 0 };
    }
    let (fill, ink) = match st.link {
        1 => (Color32::from_rgb(0x8e, 0x4f, 0xc8), Color32::WHITE),
        2 => (Color32::from_rgb(0xc9, 0xa2, 0x16), Color32::from_rgb(0x1e, 0x1e, 0x1e)),
        _ => (look::group_bg(), look::text()),
    };
    p.rect_filled(lr, 6.0, fill);
    glyphs::paint(&p, lr, glyphs::ph::LINK, 17.0, ink);
    // the Tool menus: one pill, left-click tool | Command-click tool, each with its ⌄
    let tr = Rect::from_min_size(pos2(lr.max.x + 14.0, y - H / 2.0), vec2(116.0, H));
    p.rect_filled(tr, 6.0, look::group_bg());
    p.vline(tr.center().x, tr.min.y + 5.0..=tr.max.y - 5.0, Stroke::new(1.0, look::line()));
    for (k, half) in [Rect::from_min_max(tr.min, pos2(tr.center().x, tr.max.y)), Rect::from_min_max(pos2(tr.center().x, tr.min.y), tr.max)].into_iter().enumerate() {
        let cur = if k == 0 { st.tool } else { st.tool2 };
        let r = ui.interact(half, ui.scope_id().with(("sc-tool", k)), Sense::click()).on_hover_text(if k == 0 { "Left-click Tool" } else { "Command-click Tool" });
        glyphs::paint(&p, Rect::from_center_size(pos2(half.min.x + 20.0, y), vec2(20.0, 20.0)), ICONS[cur], 16.0, look::text());
        glyphs::paint(&p, Rect::from_center_size(pos2(half.max.x - 14.0, y), vec2(12.0, 12.0)), glyphs::ph::CARET_DOWN, 10.0, look::text());
        egui::Popup::menu(&r).show(|ui| {
            for (t, n) in TOOLS.iter().enumerate() {
                // the Camera works in Page view
                let ok = t != 8 || st.view == View::Page;
                if ui.add_enabled(ok, egui::Button::selectable(cur == t, format!("{}  {n}", ICONS[t]))).clicked() {
                    if k == 0 {
                        st.tool = t;
                    } else {
                        st.tool2 = t;
                    }
                    ui.close();
                }
            }
        });
    }
    // the zoom slider (vertical-zoom icon, track, knob)
    let zr = Rect::from_min_size(pos2(bar.max.x - 112.0, y - H / 2.0), vec2(104.0, H));
    glyphs::paint(&p, Rect::from_center_size(pos2(zr.min.x + 8.0, y), vec2(14.0, 14.0)), glyphs::ph::ARROWS_VERTICAL, 13.0, look::text());
    let track = Rect::from_min_max(pos2(zr.min.x + 22.0, y - 2.0), pos2(zr.max.x - 4.0, y + 2.0));
    let resp = ui.interact(track.expand2(vec2(6.0, 8.0)), ui.scope_id().with("sc-zoom"), Sense::click_and_drag());
    let lo = 3.0f32.ln();
    let hi = 24.0f32.ln();
    if let Some(pp) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        let f = ((pp.x - track.min.x) / track.width()).clamp(0.0, 1.0);
        let new = (lo + f * (hi - lo)).exp();
        st.bar_w = (st.bar_w * new / st.space).clamp(80.0, 2400.0);
        st.space = new;
    }
    let f = ((st.space.ln() - lo) / (hi - lo)).clamp(0.0, 1.0);
    let kx = track.min.x + f * track.width();
    p.rect_filled(track, 2.0, look::mix(look::group_bg(), look::text_dim(), 0.3));
    p.rect_filled(Rect::from_min_max(track.min, pos2(kx, track.max.y)), 2.0, look::accent());
    p.circle_filled(pos2(kx, y), 7.0, Color32::from_gray(0xe8));
    p.circle_stroke(pos2(kx, y), 7.0, Stroke::new(0.5, Color32::from_black_alpha(60)));
    let _ = (Align2::CENTER_CENTER, score::TOOLS.len());
}
