//! The plug-in window's header and footer, shared by every instrument and effect window, as
//! Logic's: two rows — the large blue power (bypass) button, the wide Settings pop-up, then
//! ‹ | › (previous / next setting), Compare, Copy | Paste, Undo | Redo; at the right the Side
//! Chain pop-up (plug-ins with a side chain input), the View pop-up (Editor / Controls and the
//! window size, kept per plug-in) and the Link button (Off / Single / Multi, project-wide). A
//! narrow window drops what doesn't fit as Logic does (Copy | Paste and Undo | Redo first). The
//! plug-in's name sits in the footer. Sizes are Logic's, measured on its screenshots.

use egui::{Align2, Color32, Pos2, Rect, Sense, pos2, vec2};

use crate::glyphs;
use crate::look;

/// What a header button asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    Bypass,
    Prev,
    Next,
    Compare,
    Copy,
    Paste,
    Undo,
    Redo,
    /// View ▸ Editor (true) or Controls (false).
    Editor(bool),
}

/// The header's state, from the window.
pub struct Head<'a> {
    pub id: egui::Id,
    /// The plug-in's name: the View size is kept per plug-in.
    pub plugin: &'a str,
    pub bypassed: bool,
    pub setting: &'a str,
    pub comparing: bool,
    pub can_compare: bool,
    pub can_paste: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub side_chain: bool,
    /// Some(editor shown) when the plug-in has an Editor view besides Controls.
    pub editor: Option<bool>,
}

/// What the header drew: the clicked button, the Settings pop-up's response (the window opens
/// its menu from it) and where the Side Chain pop-up goes.
pub struct Out {
    pub act: Option<Act>,
    pub setting: egui::Response,
    pub side_chain: Option<Rect>,
}

/// Logic's header height (points), its footer's.
pub const HEIGHT: f32 = 74.0;
pub const FOOTER: f32 = 39.0;
const LEFT: f32 = 51.0;
const RIGHT: f32 = 19.0;
const GAP: f32 = 8.0;
const ROW1: f32 = 10.0;
const ROW2: f32 = 43.0;

/// The View pop-up's window sizes.
pub const SIZES: &[f32] = &[0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

fn col(dark: u32, light: u32) -> Color32 {
    crate::synthui::rgb(if kn_ui::theme::is_light() { light } else { dark })
}
/// The window's and header's background.
pub fn bg() -> Color32 {
    col(0x1f1f1f, 0xececec)
}
fn pop_bg() -> Color32 {
    col(0x313131, 0xdcdcdc)
}
fn btn_bg() -> Color32 {
    col(0x272727, 0xe0e0e0)
}
fn btn_hot() -> Color32 {
    col(0x3a3a3a, 0xcfcfcf)
}
fn divider() -> Color32 {
    col(0x4a4a4a, 0xb8b8b8)
}
fn label() -> Color32 {
    col(0x9a9a9a, 0x6a6a6a)
}
fn faded() -> Color32 {
    col(0x5c5c5c, 0xa8a8a8)
}
/// The power button's blue (Logic's, on) and its bypassed grey.
fn power_on() -> Color32 {
    Color32::from_rgb(47, 104, 197)
}

/// The Link mode (project-wide): 0 Off, 1 Single (purple), 2 Multi (yellow).
pub fn link_mode(ctx: &egui::Context) -> u8 {
    ctx.data(|d| d.get_temp::<u8>(egui::Id::unique("plugin-link-mode"))).unwrap_or(0)
}
pub fn set_link_mode(ctx: &egui::Context, m: u8) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("plugin-link-mode"), m.min(2)));
}

/// The View size of a plug-in (1.0 = 100%).
pub fn scale(ctx: &egui::Context, plugin: &str) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(egui::Id::unique(("plugin-view-scale", plugin)))).unwrap_or(1.0)
}
pub fn set_scale(ctx: &egui::Context, plugin: &str, s: f32) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("plugin-view-scale", plugin)), s.clamp(0.5, 2.0)));
}

/// The View pop-up's text: the size, or "Controls" when the Controls view shows.
pub fn view_text(scale: f32, editor: Option<bool>) -> String {
    if editor == Some(false) { "Controls".to_string() } else { format!("{}%", (scale * 100.0).round() as i32) }
}

/// Which optional parts fit a header `w` points wide: (Copy | Paste, Undo | Redo, "View:"
/// label, Side Chain label, Side Chain pop-up).
pub fn fits(w: f32, side_chain: bool) -> (bool, bool, bool, bool, bool) {
    let right = w - RIGHT - 31.0 - GAP - 82.0;
    let mut x = LEFT + 66.0 + GAP + 73.0;
    let copy = x + GAP + 96.0 + GAP <= right;
    if copy {
        x += GAP + 96.0;
    }
    let undo = copy && x + GAP + 65.0 + GAP <= right;
    if undo {
        x += GAP + 65.0;
    }
    let view_label = undo && x + GAP + 36.0 <= right;
    let preset_end = LEFT + preset_w(w);
    let sc_label = side_chain && preset_end + GAP + 76.0 + 121.0 + RIGHT <= w;
    let sc = side_chain && preset_end + GAP + 121.0 + RIGHT <= w;
    (copy, undo, view_label, sc_label, sc)
}

fn preset_w(w: f32) -> f32 {
    276.0f32.min(w - LEFT - RIGHT).max(80.0)
}

/// A rounded pill split in two halves (‹ | ›, Copy | Paste, Undo | Redo); the clicked half.
fn split(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, id: egui::Id, parts: [(&str, bool, bool, &str); 2]) -> Option<usize> {
    p.rect_filled(r, 6.0, btn_bg());
    let mid = r.center().x;
    p.line_segment([pos2(mid, r.min.y + 6.0), pos2(mid, r.max.y - 6.0)], egui::Stroke::new(1.0, divider()));
    let mut hit = None;
    for (i, (text, icon, enabled, tip)) in parts.into_iter().enumerate() {
        let h = if i == 0 { Rect::from_min_max(r.min, pos2(mid, r.max.y)) } else { Rect::from_min_max(pos2(mid, r.min.y), r.max) };
        let resp = ui.interact(h, id.with(i), Sense::click()).on_hover_text(tip);
        if resp.hovered() && enabled {
            p.rect_filled(h.shrink(1.0), 6.0, btn_hot());
        }
        let c = if enabled { look::text() } else { faded() };
        if icon {
            glyphs::paint(p, Rect::from_center_size(h.center(), vec2(14.0, 14.0)), text, 14.0, c);
        } else {
            p.text(h.center(), Align2::CENTER_CENTER, text, look::regular(12.5), c);
        }
        if resp.clicked() && enabled {
            hit = Some(i);
        }
    }
    hit
}

/// A pop-up button: text at the left, the chevron at the right.
fn popup_button(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, id: egui::Id, text: &str, updown: bool, size: f32) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click());
    p.rect_filled(r, 6.0, if resp.hovered() { btn_hot() } else { pop_bg() });
    p.with_clip_rect(r.shrink2(vec2(24.0, 0.0)).with_min_x(r.min.x + 2.0)).text(pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, text, look::regular(size), look::text());
    let icon = if updown { glyphs::ph::CARET_UP_DOWN } else { glyphs::ph::CARET_DOWN };
    glyphs::paint(p, Rect::from_center_size(pos2(r.max.x - 14.0, r.center().y), vec2(10.0, 10.0)), icon, 10.0, look::text());
    resp
}

/// Draws the header at the top of `ui` (full width) and returns what was clicked.
pub fn header(ui: &mut egui::Ui, h: &Head) -> Out {
    let margin = ui.spacing().window_margin;
    let (hr, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT - margin.top as f32), Sense::hover());
    // (Logic's measures are from the window's edges: the header spans the window margins)
    let full = Rect::from_min_max(pos2(hr.min.x - margin.left as f32, hr.min.y - margin.top as f32), pos2(hr.max.x + margin.right as f32, hr.max.y));
    let w = full.width();
    let p = ui.painter().with_clip_rect(full.expand(1.0));
    p.rect_filled(full, 0.0, bg());
    p.line_segment([pos2(full.min.x, full.max.y - 0.5), pos2(full.max.x, full.max.y - 0.5)], egui::Stroke::new(1.0, Color32::BLACK));
    let p = ui.painter().clone();
    let at = |x: f32, y: f32| -> Pos2 { pos2(full.min.x + x, full.min.y + y) };
    let mut act = None;
    // power (bypass)
    let pr = Rect::from_min_size(at(10.0, ROW1 + 1.0), vec2(31.0, 31.0));
    let presp = ui.interact(pr, h.id.with("power"), Sense::click()).on_hover_text("Bypass");
    let fill = if h.bypassed { col(0x4a4a4a, 0xbdbdbd) } else { power_on() };
    p.circle_filled(pr.center(), 15.5, if presp.hovered() { look::mix(fill, Color32::WHITE, 0.08) } else { fill });
    glyphs::paint(&p, Rect::from_center_size(pr.center(), vec2(20.0, 20.0)), glyphs::ph::POWER, 20.0, if h.bypassed { col(0x9a9a9a, 0x505050) } else { Color32::WHITE });
    if presp.clicked() {
        act = Some(Act::Bypass);
    }
    // the Settings pop-up
    let sr = Rect::from_min_size(at(LEFT, ROW1), vec2(preset_w(w), 24.0));
    let setting = popup_button(ui, &p, sr, h.id.with("setting"), h.setting, false, 13.0);
    let (copy, undo, view_label, sc_label, sc) = fits(w, h.side_chain);
    // row 2
    let mut x = LEFT;
    let row = |x: f32, bw: f32| Rect::from_min_size(at(x, ROW2), vec2(bw, 23.0));
    if let Some(i) = split(ui, &p, row(x, 66.0), h.id.with("step"), [(glyphs::ph::CARET_LEFT, true, true, "Previous Setting"), (glyphs::ph::CARET_RIGHT, true, true, "Next Setting")]) {
        act = Some(if i == 0 { Act::Prev } else { Act::Next });
    }
    x += 66.0 + GAP;
    let cr = row(x, 73.0);
    let cresp = ui.interact(cr, h.id.with("compare"), Sense::click()).on_hover_text("Compare the edited setting with the saved one");
    let lit = h.comparing || (cresp.hovered() && h.can_compare);
    p.rect_filled(cr, 6.0, if lit { btn_hot() } else { btn_bg() });
    p.text(cr.center(), Align2::CENTER_CENTER, "Compare", look::regular(12.5), if h.can_compare || h.comparing { look::text() } else { faded() });
    if cresp.clicked() && (h.can_compare || h.comparing) {
        act = Some(Act::Compare);
    }
    x += 73.0 + GAP;
    if copy {
        if let Some(i) = split(ui, &p, row(x, 96.0), h.id.with("copy"), [("Copy", false, true, "Copy Setting"), ("Paste", false, h.can_paste, "Paste Setting")]) {
            act = Some(if i == 0 { Act::Copy } else { Act::Paste });
        }
        x += 96.0 + GAP;
    }
    if undo {
        if let Some(i) = split(ui, &p, row(x, 65.0), h.id.with("undo"), [(glyphs::ph::ARROW_U_UP_LEFT, true, h.can_undo, "Undo"), (glyphs::ph::ARROW_U_UP_RIGHT, true, h.can_redo, "Redo")]) {
            act = Some(if i == 0 { Act::Undo } else { Act::Redo });
        }
    }
    // right: Link, View
    let lr = Rect::from_min_size(at(w - RIGHT - 31.0, ROW2), vec2(31.0, 23.0));
    let lresp = ui.interact(lr, h.id.with("link"), Sense::click()).on_hover_text("Link: Off / Single (purple) / Multi (yellow); Control-click for the menu");
    let mode = link_mode(ui.ctx());
    p.rect_filled(lr, 6.0, if lresp.hovered() { btn_hot() } else { btn_bg() });
    let lcol = match mode {
        1 => Color32::from_rgb(0xb4, 0x8c, 0xf5),
        2 => Color32::from_rgb(0xf2, 0xc9, 0x4c),
        _ => look::text(),
    };
    glyphs::paint(&p, Rect::from_center_size(lr.center(), vec2(15.0, 15.0)), glyphs::ph::LINK, 15.0, lcol);
    if lresp.clicked() {
        set_link_mode(ui.ctx(), if mode == 0 { 1 } else { 0 });
    }
    egui::Popup::context_menu(&lresp).show(|ui| {
        for (i, n) in ["Off", "Single", "Multi"].iter().enumerate() {
            if ui.add(egui::Button::selectable(mode == i as u8, *n)).clicked() {
                set_link_mode(ui.ctx(), i as u8);
                ui.close();
            }
        }
    });
    let vr = Rect::from_min_size(pos2(lr.min.x - GAP - 82.0, lr.min.y), vec2(82.0, 23.0));
    let s = scale(ui.ctx(), h.plugin);
    let vresp = popup_button(ui, &p, vr, h.id.with("view"), &view_text(s, h.editor), false, 13.0);
    if view_label {
        p.text(pos2(vr.min.x - 6.0, vr.center().y), Align2::RIGHT_CENTER, "View:", look::regular(13.0), label());
    }
    egui::Popup::menu(&vresp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(140.0);
        if let Some(ed) = h.editor {
            if ui.add(egui::Button::selectable(ed, "Editor")).clicked() {
                act = Some(Act::Editor(true));
                ui.close();
            }
            if ui.add(egui::Button::selectable(!ed, "Controls")).clicked() {
                act = Some(Act::Editor(false));
                ui.close();
            }
        } else {
            let _ = ui.add(egui::Button::selectable(true, "Controls"));
        }
        ui.separator();
        for z in SIZES {
            if ui.add(egui::Button::selectable((s - z).abs() < 0.01, format!("{}%", (z * 100.0) as i32))).clicked() {
                set_scale(ui.ctx(), h.plugin, *z);
                ui.close();
            }
        }
    });
    // the Side Chain pop-up's place (the window draws it)
    let side_chain = sc.then(|| {
        let r = Rect::from_min_size(at(w - RIGHT - 121.0, ROW1), vec2(121.0, 24.0));
        if sc_label {
            p.text(pos2(r.min.x - 6.0, r.center().y), Align2::RIGHT_CENTER, "Side Chain:", look::regular(13.0), label());
        }
        r
    });
    Out { act, setting, side_chain }
}

/// A pop-up button for the Side Chain (its current choice, up/down chevrons).
pub fn side_chain_button(ui: &mut egui::Ui, r: Rect, id: egui::Id, text: &str) -> egui::Response {
    let p = ui.painter().clone();
    popup_button(ui, &p, r, id, text, true, 13.0)
}

/// The footer with the plug-in's name, at the bottom of `ui`; `more` is the extended
/// parameters' disclosure chevron at its left (Some(open) when the plug-in has them).
pub fn footer(ui: &mut egui::Ui, name: &str, more: Option<&mut bool>) {
    let margin = ui.spacing().window_margin;
    let (fr, _) = ui.allocate_exact_size(vec2(ui.available_width(), FOOTER - margin.bottom as f32), Sense::hover());
    let full = Rect::from_min_max(pos2(fr.min.x - margin.left as f32, fr.min.y), pos2(fr.max.x + margin.right as f32, fr.max.y + margin.bottom as f32));
    let p = ui.painter().with_clip_rect(full.expand(1.0));
    p.rect_filled(full, 0.0, bg());
    p.line_segment([pos2(full.min.x, full.min.y + 0.5), pos2(full.max.x, full.min.y + 0.5)], egui::Stroke::new(1.0, Color32::BLACK));
    p.text(full.center(), Align2::CENTER_CENTER, name, look::regular(18.0), col(0xe0e0e0, 0x303030));
    if let Some(open) = more {
        let r = Rect::from_center_size(pos2(full.min.x + 20.0, full.center().y), vec2(24.0, 24.0));
        let resp = ui.interact(r, ui.id().with(("plug-foot-more", name)), Sense::click()).on_hover_text("Extended Parameters");
        glyphs::paint(&p, r, if *open { glyphs::ph::CARET_DOWN } else { glyphs::ph::CARET_RIGHT }, 14.0, if resp.hovered() { look::text() } else { label() });
        if resp.clicked() {
            *open = !*open;
        }
    }
}

/// Applies a plug-in's View size to its window's layer (scaled about the window's top left).
pub fn apply_scale(ctx: &egui::Context, layer: egui::LayerId, top_left: Pos2, s: f32) {
    use egui::emath::TSTransform;
    let t = TSTransform::from_translation(top_left.to_vec2()) * TSTransform::from_scaling(s) * TSTransform::from_translation(-top_left.to_vec2());
    ctx.set_transform_layer(layer, t);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_windows_drop_parts_as_logic() {
        // Chorus (388 pt): no Copy | Paste, no Undo | Redo, no "View:"
        assert_eq!(fits(388.0, false), (false, false, false, false, false));
        // Compressor (1028 pt): everything
        assert_eq!(fits(1028.0, true), (true, true, true, true, true));
        // Limiter (729 pt): everything on row 2
        let f = fits(729.0, true);
        assert!(f.0 && f.1 && f.2);
    }

    #[test]
    fn view_text_shows_size_or_controls() {
        assert_eq!(view_text(0.75, None), "75%");
        assert_eq!(view_text(1.0, Some(true)), "100%");
        assert_eq!(view_text(1.0, Some(false)), "Controls");
    }
}
