//! Notes 4.13's Markup palette, measured on the NO3 captures (2x, light): a band across the
//! bottom of the editor (a #E5E5E5 hairline 56.5 pt above the bottom) holding a capsule — the
//! selection tool, the ink / eraser with its picker (⌄), the colour dot | Shapes, Text,
//! Signature. The tool picker has five sizes and the inks (Pen, Monoline, Marker, Pencil, Crayon,
//! Fountain Pen, Reed, Watercolor, Eraser — pixel or object); the colour dot opens the 12-colour
//! strip and the 12 × 10 grid with Show Colors…; a selected object gets a mini toolbar under it
//! (fill, stroke with its width, Aa, opacity; a text box: colour, Aa). Also the text field over a
//! text box being typed in and Create Signature's sheet. Undo and Done are the menus' and a click
//! outside the drawing (Notes has no buttons for them in the palette).

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke as Line, pos2, vec2};

use super::draw::{self, ColorTarget, Pop, Signing, Tool};
use super::ink::{self, Builder, InkKind, ShapeKind};
use super::layout;
use super::shapes::{self, c32};
use super::Editor;

/// The bottom band and the palette in it.
const BAND_H: f32 = 56.5;
const PAL_W: f32 = 238.5;
const PAL_H: f32 = 38.5;
/// Item centres from the palette's left edge.
const X_SELECT: f32 = 23.0;
const X_TOOL: f32 = 61.75;
const X_DOT: f32 = 99.25;
const X_SEP: f32 = 121.25;
const X_SHAPES: f32 = 145.75;
const X_TEXT: f32 = 181.0;
const X_SIGN: f32 = 215.75;
/// A popover's arrow and its gap to what it points at.
const ARROW_W: f32 = 26.0;
const ARROW_H: f32 = 9.0;

/// The colour dot's strip (sampled from popover-colour-grid-light.png).
pub const STRIP: [u32; 12] = [0xeb4125, 0xf09937, 0xfefb52, 0x72f54a, 0x73f9fd, 0x1331f5, 0xeb52f7, 0x882a8d, 0xa37c4b, 0xffffff, 0x919191, 0x000000];
/// Its grid: greys, then nine rows dark to light, twelve hues (the first grey is "none").
pub const GRID: [[u32; 12]; 10] = [
    [0x000000, 0xffffff, 0xebebea, 0xd7d6d7, 0xc0c0c1, 0xaaaaab, 0x939392, 0x7a797a, 0x60605f, 0x434444, 0x222222, 0x000000],
    [0x143549, 0x071d54, 0x0f0639, 0x2a093b, 0x360c1c, 0x541007, 0x522009, 0x52350d, 0x523e10, 0x65611b, 0x505518, 0x2b3c16],
    [0x1e4c63, 0x0f2e75, 0x190a50, 0x3f1156, 0x4e1729, 0x781e0d, 0x723011, 0x734c16, 0x73591a, 0x8c8728, 0x717625, 0x3f5623],
    [0x2e6b8b, 0x1841a3, 0x281371, 0x591d78, 0x6f223d, 0xa72c17, 0xa0451b, 0xa06b24, 0xa07d28, 0xc2bc3c, 0x9ca536, 0x597934],
    [0x3d89b0, 0x2355cf, 0x331b8f, 0x702899, 0x8e2f4f, 0xd03a20, 0xc95a23, 0xc9882e, 0xcb9f35, 0xf4ec4f, 0xc6d047, 0x719b45],
    [0x48a1d3, 0x2960f6, 0x4825ad, 0x8c32b6, 0xaa395d, 0xea522e, 0xed742f, 0xf3ae3c, 0xf5ca44, 0xfffc67, 0xddeb5c, 0x85b953],
    [0x59c3f7, 0x4e86f6, 0x5732e2, 0xaf42eb, 0xd44a7a, 0xec6c59, 0xf08c56, 0xf5b757, 0xf6cc5b, 0xfdf881, 0xe5ef7a, 0xa2d16f],
    [0x79d3f9, 0x7fa5f9, 0x7e51f5, 0xc35ef6, 0xdd789d, 0xf09287, 0xf2a984, 0xf6c982, 0xf9da85, 0xfef9a2, 0xecf39b, 0xb9db95],
    [0xa6e2fa, 0xadc5fa, 0xab8df6, 0xd895f8, 0xe8a7c0, 0xf3b8b1, 0xf6c7af, 0xf9daae, 0xfbe5af, 0xfefbc0, 0xf2f6bd, 0xd2e7ba],
    [0xd3effd, 0xd6e2fc, 0xd6cafb, 0xebcafb, 0xf2d4e0, 0xf9dbd9, 0xfae3d9, 0xfbedd7, 0xfef2d9, 0xfefce0, 0xf8fadf, 0xe3ecd6],
];
/// A selected object's swatches (fill, stroke, text: popover-shape-fill-light.png).
pub const SWATCHES: [u32; 12] = [0xffffff, 0xbfbfbf, 0x000000, 0x8ce1cf, 0xda5e9d, 0x6f31ed, 0xd85946, 0xe19133, 0xf0cf60, 0x7cc270, 0x76c2f1, 0x375eeb];
/// The inks in the picker's order.
pub const INKS: [(InkKind, &str); 8] = [
    (InkKind::Pen, "Pen"),
    (InkKind::Monoline, "Monoline"),
    (InkKind::Marker, "Marker"),
    (InkKind::Pencil, "Pencil"),
    (InkKind::Crayon, "Crayon"),
    (InkKind::Fountain, "Fountain Pen"),
    (InkKind::Reed, "Reed"),
    (InkKind::Watercolor, "Watercolor"),
];
const TEXT_SIZES: [f32; 14] = [9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 18.0, 24.0, 36.0, 48.0, 64.0, 72.0, 96.0, 144.0];

pub fn rgb(u: u32) -> [f32; 4] {
    [((u >> 16) & 255) as f32 / 255.0, ((u >> 8) & 255) as f32 / 255.0, (u & 255) as f32 / 255.0, 1.0]
}

fn dark() -> bool {
    layout::dark()
}

/// Black in light mode (the palette's glyphs are black).
fn ink() -> Color32 {
    if dark() { Color32::from_gray(235) } else { Color32::BLACK }
}

fn font(size: f32) -> FontId {
    FontId::new(size, layout::family("Sfera-Regular"))
}

/// The palette's slide (no recording of Notes' shows it: the usual sheet timing).
pub const SLIDE_IN: f64 = 0.3;
pub const SLIDE_OUT: f64 = 0.25;
/// How far it travels: the band and its shadow, out of sight below the editor.
const SLIDE_DIST: f32 = BAND_H + 14.0;

/// How much of the palette is still out of sight while it slides in (1 hidden .. 0 in place):
/// ease-out cubic.
pub fn hidden_share(shown_at: Option<f64>, now: f64) -> f32 {
    let Some(t0) = shown_at else { return 1.0 };
    let p = ((now - t0) / SLIDE_IN).clamp(0.0, 1.0) as f32;
    (1.0 - p).powi(3)
}

/// The same while it slides out: ease-in cubic.
pub fn exit_hidden(t0: f64, now: f64) -> f32 {
    (((now - t0) / SLIDE_OUT).clamp(0.0, 1.0) as f32).powi(3)
}

/// When a slide out must have begun to be `hidden` out now (Done during the slide in carries on
/// from where the palette is).
pub fn exit_start(hidden: f32, now: f64) -> f64 {
    now - hidden.clamp(0.0, 1.0).cbrt() as f64 * SLIDE_OUT
}

/// Where the palette is: how far below its place (points), while drawing or sliding out.
#[cfg(test)]
pub fn palette_offset(ed: &Editor) -> Option<f32> {
    match (&ed.draw, &ed.markup_exit) {
        (Some(st), _) => Some(hidden_share(st.shown_at, ed.now) * SLIDE_DIST),
        (None, Some((_, t0))) => Some(exit_hidden(*t0, ed.now) * SLIDE_DIST),
        _ => None,
    }
}

/// The band across the bottom of the editor while drawing.
pub fn band_rect(ed: &Editor) -> Option<Rect> {
    let v = ed.view.rect?;
    Some(Rect::from_min_max(pos2(v.min.x, v.max.y - BAND_H), v.max))
}

pub fn palette_rect(ed: &Editor) -> Option<Rect> {
    let b = band_rect(ed)?;
    Some(Rect::from_center_size(b.center(), vec2(PAL_W, PAL_H)))
}

/// A popover's panel (and its arrow, pointing at `tip` from above or below).
fn panel(p: &egui::Painter, r: Rect, tip: Pos2, below: bool, glass: bool) {
    let fill = if dark() { Color32::from_gray(if glass { 58 } else { 44 }) } else if glass { Color32::from_gray(232) } else { Color32::from_gray(254) };
    let rad = CornerRadius::same(16);
    soft_shadow(p, r, 16.0);
    // the arrow: a triangle from the panel's edge to the tip
    let (y0, sgn) = if below { (r.min.y, -1.0) } else { (r.max.y, 1.0) };
    let x = tip.x.clamp(r.min.x + 20.0, r.max.x - 20.0);
    let tri = vec![pos2(x - ARROW_W / 2.0, y0 - sgn), pos2(x + ARROW_W / 2.0, y0 - sgn), pos2(x, y0 + sgn * ARROW_H)];
    p.add(egui::Shape::convex_polygon(tri.clone(), fill, Line::NONE));
    p.rect_filled(r, rad, fill);
    p.rect_stroke(r, rad, Line::new(0.5, Color32::from_black_alpha(10)), egui::StrokeKind::Outside);
    p.add(egui::Shape::convex_polygon(tri, fill, Line::NONE));
}

/// A shadow that fades out over 12 pt (Notes' popovers and toolbars show no edge to it).
fn soft_shadow(p: &egui::Painter, r: Rect, rad: f32) {
    for k in 1..=12 {
        let e = k as f32;
        p.rect_filled(r.translate(vec2(0.0, 2.0)).expand(e), CornerRadius::same((rad + e) as u8), Color32::from_black_alpha(2));
    }
}

/// Where a popover of `size` goes: over (or under) `anchor`, its arrow at the anchor's middle,
/// inside the view.
fn place_pop(ed: &Editor, anchor: Rect, size: egui::Vec2, prefer_below: bool) -> (Rect, Pos2, bool) {
    let v = ed.view.rect.unwrap_or(Rect::EVERYTHING);
    let band_top = band_rect(ed).map_or(v.max.y, |b| b.min.y);
    // (NO3: the arrow's tip meets the button's edge)
    let below = prefer_below && anchor.max.y + ARROW_H + size.y < band_top || !prefer_below && anchor.min.y - ARROW_H - size.y < v.min.y;
    let x = (anchor.center().x - size.x / 2.0).clamp(v.min.x + 8.0, (v.max.x - 8.0 - size.x).max(v.min.x + 8.0));
    let (y, tip) = if below { (anchor.max.y + ARROW_H, pos2(anchor.center().x, anchor.max.y)) } else { (anchor.min.y - ARROW_H - size.y, pos2(anchor.center().x, anchor.min.y)) };
    (Rect::from_min_size(pos2(x, y), size), tip, below)
}

type Acts = Vec<Box<dyn FnOnce(&mut Editor)>>;

/// The palette, the selection's mini toolbar and whatever is open (while drawing).
pub fn toolbar(ed: &mut Editor, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    // sliding: in when the drawing opens, out (as it was, its popovers closed) when it closes
    let ghost = ed.draw.is_none();
    let off = if ghost {
        let Some((g, t0)) = ed.markup_exit.clone() else { return };
        let h = exit_hidden(t0, now);
        if h >= 1.0 {
            ed.markup_exit = None;
            return;
        }
        ed.draw = Some(g);
        h * SLIDE_DIST
    } else {
        ed.markup_exit = None;
        let st = ed.draw.as_mut().unwrap();
        let t0 = *st.shown_at.get_or_insert(now);
        hidden_share(Some(t0), now) * SLIDE_DIST
    };
    if off > 0.0 {
        ctx.request_repaint();
    }
    let (Some(band), Some(pal)) = (band_rect(ed), palette_rect(ed)) else {
        if ghost {
            ed.draw = None;
        }
        return;
    };
    let view = ed.view.rect.unwrap_or(band);
    let (band, pal) = (band.translate(vec2(0.0, off)), pal.translate(vec2(0.0, off)));
    let Some(st) = ed.draw.clone() else { return };
    let mut rects = vec![band];
    let mut act: Acts = Vec::new();
    let mut anchors: Vec<(Pop, Rect)> = Vec::new();
    egui::Area::new(egui::Id::unique("nt-markup-band")).order(egui::Order::Foreground).fade_in(false).interactable(!ghost).fixed_pos(band.min).show(&ctx, |ui| {
        // (below the editor's edge it doesn't show)
        ui.set_clip_rect(view);
        let (r, _) = ui.allocate_exact_size(band.size(), Sense::click());
        let p = ui.painter().clone();
        p.rect_filled(r, CornerRadius { nw: 0, ne: 0, sw: ed.corners.sw, se: ed.corners.se }, layout::bg_color());
        p.hline(r.x_range(), r.min.y + 0.25, Line::new(0.5, if dark() { Color32::from_gray(60) } else { Color32::from_rgb(229, 229, 229) }));
        // the capsule
        let pr = pal;
        let rad = CornerRadius::same((PAL_H / 2.0) as u8);
        soft_shadow(&p, pr, PAL_H / 2.0);
        p.rect_filled(pr, rad, if dark() { Color32::from_gray(40) } else { Color32::from_gray(253) });
        p.rect_stroke(pr, rad, Line::new(0.5, if dark() { Color32::from_gray(70) } else { Color32::from_gray(232) }), egui::StrokeKind::Inside);
        let cy = pr.center().y;
        let at = |x: f32| pos2(pr.min.x + x, cy);
        let fg = ink();
        let on_fg = if dark() { Color32::BLACK } else { Color32::WHITE };
        let on_bg = if dark() { Color32::from_gray(235) } else { Color32::BLACK };
        // selection
        let sel_on = matches!(st.tool, Tool::Select | Tool::Lasso);
        let sr = Rect::from_center_size(at(X_SELECT), vec2(29.0, 29.0));
        let resp = ui.interact(sr, egui::Id::unique("nt-mk-select"), Sense::click()).on_hover_text("Selection");
        if sel_on {
            p.circle_filled(sr.center(), 14.5, on_bg);
        } else if resp.hovered() {
            p.circle_filled(sr.center(), 14.5, Color32::from_black_alpha(14));
        }
        select_glyph(&p, sr.center(), if sel_on { on_fg } else { fg });
        if resp.clicked() {
            act.push(Box::new(|e: &mut Editor| {
                if let Some(s) = e.draw.as_mut() {
                    if !matches!(s.tool, Tool::Select | Tool::Lasso) {
                        s.pen_tool = s.tool;
                    }
                    s.tool = Tool::Select;
                    s.pop = None;
                }
            }));
        }
        // the ink or eraser, and its picker
        let pen_on = !sel_on;
        let shown = if pen_on { st.tool } else { st.pen_tool };
        let tr = Rect::from_center_size(at(X_TOOL), vec2(37.5, 29.0));
        let resp = ui.interact(tr, egui::Id::unique("nt-mk-tool"), Sense::click()).on_hover_text("Tools");
        if pen_on {
            p.rect_filled(tr, CornerRadius::same(15), on_bg);
        } else if resp.hovered() {
            p.rect_filled(tr, CornerRadius::same(15), Color32::from_black_alpha(14));
        }
        let c = if pen_on { on_fg } else { fg };
        tool_glyph(&p, pos2(tr.center().x - 6.0, cy), shown, 1.25, c);
        chevron(&p, pos2(tr.center().x + 9.0, cy + 0.5), c);
        anchors.push((Pop::Tools, tr));
        if resp.clicked() {
            act.push(Box::new(move |e: &mut Editor| {
                if let Some(s) = e.draw.as_mut() {
                    if pen_on {
                        s.pop = if s.pop == Some(Pop::Tools) { None } else { Some(Pop::Tools) };
                    } else {
                        s.tool = s.pen_tool;
                        s.pop = None;
                    }
                }
            }));
        }
        // the colour (not for the eraser)
        let erasing = matches!(st.tool, Tool::PixelEraser | Tool::ObjectEraser);
        let dr = Rect::from_center_size(at(X_DOT), vec2(24.0, 24.0));
        let resp = ui.interact(dr, egui::Id::unique("nt-mk-dot"), Sense::click()).on_hover_text("Color");
        let dc = c32(st.color);
        p.circle_filled(dr.center(), 7.5, if erasing { dc.gamma_multiply(0.35) } else { dc });
        p.circle_stroke(dr.center(), 7.5, Line::new(0.75, Color32::from_black_alpha(if erasing { 20 } else { 50 })));
        anchors.push((Pop::Color, dr));
        if resp.clicked() && !erasing {
            act.push(Box::new(|e: &mut Editor| {
                if let Some(s) = e.draw.as_mut() {
                    s.pop = if s.pop == Some(Pop::Color) { None } else { Some(Pop::Color) };
                }
            }));
        }
        p.vline(pr.min.x + X_SEP, cy - 11.0..=cy + 11.0, Line::new(0.5, Color32::from_black_alpha(36)));
        // shapes, text, signature
        for (x, name, k) in [(X_SHAPES, "Shapes", 0), (X_TEXT, "Text", 1), (X_SIGN, "Signature", 2)] {
            let br = Rect::from_center_size(at(x), vec2(30.0, 29.0));
            let resp = ui.interact(br, egui::Id::unique(("nt-mk-item", k)), Sense::click()).on_hover_text(name);
            let open = (k == 0 && st.pop == Some(Pop::Shapes)) || (k == 2 && st.pop == Some(Pop::Sign));
            if open || resp.hovered() {
                p.rect_filled(br, CornerRadius::same(8), Color32::from_black_alpha(if open { 22 } else { 12 }));
            }
            match k {
                0 => shapes_glyph(&p, br.center(), fg),
                1 => text_glyph(&p, br.center(), fg),
                _ => sign_glyph(&p, br.center(), fg),
            }
            if k == 0 {
                anchors.push((Pop::Shapes, br));
            }
            if k == 2 {
                anchors.push((Pop::Sign, br));
            }
            if resp.clicked() {
                act.push(Box::new(move |e: &mut Editor| match k {
                    0 | 2 => {
                        let want = if k == 0 { Pop::Shapes } else { Pop::Sign };
                        if let Some(s) = e.draw.as_mut() {
                            s.pop = if s.pop == Some(want) { None } else { Some(want) };
                        }
                    }
                    _ => {
                        let style = e.draw.as_ref().map(|s| s.style.clone()).unwrap_or_default();
                        e.add_shape(shapes::new_shape(ShapeKind::Text, Pos2::ZERO, &style));
                    }
                }));
            }
        }
    });
    if ghost {
        ed.draw = None;
        return;
    }
    if off > 0.0 {
        // (still sliding in: no popovers or object bar yet)
        if let Some(s) = ed.draw.as_mut() {
            s.ui_rects = rects;
        }
        for f in act {
            f(ed);
        }
        return;
    }
    // the selected object's mini toolbar
    if let Some(r) = mini_toolbar(ed, &ctx, &st, &mut act, &mut anchors) {
        rects.push(r);
    }
    // the open popover
    if let Some(pop) = st.pop {
        if let Some((_, anchor)) = anchors.iter().find(|a| a.0 == pop).copied() {
            if let Some(r) = popover(ed, &ctx, pop, anchor, &st, &mut act) {
                rects.push(r);
            }
        }
    }
    text_field(ed, ui, &mut act);
    if st.signing.is_some() {
        if let Some(r) = sign_sheet(ed, ui, &mut act) {
            rects.push(r);
        }
    }
    // Show Colors… / the colour wheels: the suite's Colors window
    if let Some(target) = st.colors {
        let style = selected_style(ed);
        let mut c = c32(match target {
            ColorTarget::Ink => st.color,
            ColorTarget::Fill => style.fill,
            ColorTarget::Stroke => style.stroke,
            ColorTarget::Text => style.text_color,
        });
        let mut open = true;
        let mut panel_state = st.color_panel.clone();
        if kn_ui::colors::colors_window_for(&ctx, &mut panel_state, &mut c, &mut open) {
            let v = shapes::rgba(c);
            act.push(Box::new(move |e: &mut Editor| set_color(e, target, v)));
        }
        if let Some(r) = ctx.memory(|m| m.area_rect(egui::Id::unique("Colors"))) {
            rects.push(r);
        }
        act.push(Box::new(move |e: &mut Editor| {
            if let Some(s) = e.draw.as_mut() {
                s.color_panel = panel_state;
                if !open {
                    s.colors = None;
                }
            }
        }));
    }
    if let Some(s) = ed.draw.as_mut() {
        s.ui_rects = rects;
    }
    for f in act {
        f(ed);
    }
    // the tools as they are now, for the next drawing and the next launch
    if let Some(p) = ed.draw.as_ref().map(|s| s.prefs()) {
        if p != st.prefs() {
            #[cfg(not(test))]
            draw::save_prefs(&p);
        }
    }
}

/// The one selected object, if one is.
fn selected_one(ed: &Editor) -> Option<ink::Shape> {
    let st = ed.draw.as_ref()?;
    if st.selected_shapes.len() != 1 || !st.selected.is_empty() {
        return None;
    }
    ed.drawing()?.shapes.iter().find(|s| s.id == st.selected_shapes[0]).cloned()
}

/// The style the object menus show: the selected object's, else what new objects get.
fn selected_style(ed: &Editor) -> shapes::Style {
    let mut s = ed.draw.as_ref().map(|d| d.style.clone()).unwrap_or_default();
    if let Some(o) = selected_one(ed) {
        s.stroke = o.stroke;
        s.fill = o.fill;
        s.line_width = o.line_width;
        s.font = o.font.clone();
        s.font_size = o.font_size;
        s.text_color = o.text_color;
        s.align = o.align;
    }
    s
}

/// A colour from a swatch or the Colors window.
fn set_color(e: &mut Editor, target: ColorTarget, v: [f32; 4]) {
    match target {
        ColorTarget::Ink => {
            if let Some(s) = e.draw.as_mut() {
                s.color = v;
            }
        }
        ColorTarget::Fill => e.restyle(&move |st, o| match o {
            Some(o) => o.fill = v,
            None => st.fill = v,
        }),
        ColorTarget::Stroke => e.restyle(&move |st, o| match o {
            Some(o) => {
                o.stroke = v;
                // a signature's ink is its stroke
                for k in &mut o.ink {
                    k.color = v;
                }
            }
            None => st.stroke = v,
        }),
        ColorTarget::Text => e.restyle(&move |st, o| match o {
            Some(o) => o.text_color = v,
            None => st.text_color = v,
        }),
    }
}

/// The mini toolbar under the selected object (NO3: 14.5 pt below it, items 35 pt apart):
/// fill, stroke, Aa, opacity; a text box: its colour and Aa; a line: stroke and opacity.
fn mini_toolbar(ed: &Editor, ctx: &egui::Context, st: &draw::DrawState, act: &mut Acts, anchors: &mut Vec<(Pop, Rect)>) -> Option<Rect> {
    if st.moving.is_some() || st.shape_drag.is_some() || st.text_edit.is_some() {
        return None;
    }
    let o = selected_one(ed)?;
    let (cr, k) = draw::canvas(ed)?;
    let b = shapes::bounds(&o);
    let sb = Rect::from_min_max(cr.min + b.min.to_vec2() * k, cr.min + b.max.to_vec2() * k);
    let items: Vec<Pop> = match o.kind {
        ShapeKind::Text => vec![Pop::TextColor, Pop::TextStyle],
        ShapeKind::Line | ShapeKind::Arrow => vec![Pop::Stroke, Pop::Opacity],
        ShapeKind::Signature => vec![Pop::Stroke, Pop::Opacity],
        _ => vec![Pop::Fill, Pop::Stroke, Pop::TextStyle, Pop::Opacity],
    };
    let w = 38.0 + (items.len() as f32 - 1.0) * 35.0;
    let size = vec2(w, 37.5);
    let band_top = band_rect(ed).map_or(f32::INFINITY, |r| r.min.y);
    let v = ed.view.rect?;
    let mut top = sb.max.y + 14.5;
    if top + size.y > band_top - 4.0 {
        top = sb.min.y - 14.5 - size.y;
    }
    let x = (sb.center().x - w / 2.0).clamp(v.min.x + 8.0, v.max.x - 8.0 - w);
    let r = Rect::from_min_size(pos2(x, top.max(v.min.y + 4.0)), size);
    egui::Area::new(egui::Id::unique("nt-markup-mini")).order(egui::Order::Foreground).fade_in(false).fixed_pos(r.min).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(size, Sense::click());
        let p = ui.painter();
        let rad = CornerRadius::same(19);
        soft_shadow(p, r, 19.0);
        p.rect_filled(r, rad, if dark() { Color32::from_gray(56) } else { Color32::from_gray(247) });
        p.rect_stroke(r, rad, Line::new(0.5, Color32::from_black_alpha(26)), egui::StrokeKind::Inside);
        for (i, it) in items.iter().copied().enumerate() {
            let c = pos2(r.min.x + 19.0 + i as f32 * 35.0, r.center().y);
            let br = Rect::from_center_size(c, vec2(32.0, 30.0));
            let resp = ui.interact(br, egui::Id::unique(("nt-mk-mini", i)), Sense::click());
            // (Notes marks no item for the open popover)
            if resp.hovered() {
                p.circle_filled(c, 15.0, Color32::from_black_alpha(10));
            }
            match it {
                Pop::Fill => color_dot(p, c, 8.5, o.fill),
                Pop::TextColor => color_dot(p, c, 8.5, o.text_color),
                Pop::Stroke if o.kind == ShapeKind::Signature => color_dot(p, c, 8.5, o.stroke),
                Pop::Stroke => {
                    if o.stroke[3] < 0.01 {
                        // the stroke's slash, struck through in red
                        p.line_segment([c + vec2(-7.0, 7.0), c + vec2(7.0, -7.0)], Line::new(2.0, ink()));
                        p.line_segment([c + vec2(-7.0, -7.0), c + vec2(7.0, 7.0)], Line::new(1.6, Color32::from_rgb(230, 52, 40)));
                    } else {
                        p.line_segment([c + vec2(-7.0, 7.0), c + vec2(7.0, -7.0)], Line::new(2.4, ink()));
                    }
                }
                Pop::TextStyle => {
                    p.text(c, Align2::CENTER_CENTER, "Aa", font(17.0), ink());
                }
                _ => {
                    // opacity: a dotted ring
                    for k in 0..12 {
                        let a = k as f32 / 12.0 * std::f32::consts::TAU;
                        p.circle_filled(c + vec2(a.cos(), a.sin()) * 8.0, 0.95, ink());
                    }
                }
            }
            anchors.push((it, br));
            if resp.clicked() {
                act.push(Box::new(move |e: &mut Editor| {
                    if let Some(s) = e.draw.as_mut() {
                        s.pop = if s.pop == Some(it) { None } else { Some(it) };
                    }
                }));
            }
        }
    });
    Some(r)
}

fn color_dot(p: &egui::Painter, c: Pos2, r: f32, col: [f32; 4]) {
    if col[3] < 0.01 {
        p.circle_filled(c, r, Color32::WHITE);
        p.circle_stroke(c, r, Line::new(0.6, Color32::from_gray(170)));
        p.line_segment([c + vec2(-r * 0.7, r * 0.7), c + vec2(r * 0.7, -r * 0.7)], Line::new(1.4, Color32::from_rgb(230, 52, 40)));
    } else {
        p.circle_filled(c, r, c32(col));
        p.circle_stroke(c, r, Line::new(0.6, Color32::from_black_alpha(36)));
    }
}

/// The popover `pop`, pointing at `anchor`. Returns its rectangle.
fn popover(ed: &Editor, ctx: &egui::Context, pop: Pop, anchor: Rect, st: &draw::DrawState, act: &mut Acts) -> Option<Rect> {
    let from_palette = matches!(pop, Pop::Tools | Pop::Color | Pop::Shapes | Pop::Sign);
    let erasing = matches!(st.tool, Tool::PixelEraser | Tool::ObjectEraser);
    let sigs = if pop == Pop::Sign { draw::signatures() } else { Vec::new() };
    let style = selected_style(ed);
    let size = match pop {
        // (NO3: 18.5 pt under the last row's middle, so its highlight clears the corners)
        Pop::Tools if erasing => vec2(171.5, 373.0),
        Pop::Tools => vec2(171.5, 337.0),
        Pop::Color => vec2(241.0, 265.0),
        Pop::Shapes => vec2(70.0, 127.5),
        Pop::Sign => vec2(250.0, 44.0 + sigs.len() as f32 * 64.0),
        Pop::Fill => vec2(245.0, 133.0),
        Pop::Stroke if style.stroke[3] > 0.01 => vec2(245.0, 177.0),
        Pop::Stroke => vec2(245.0, 133.0),
        Pop::TextColor => vec2(245.0, 133.0),
        Pop::TextStyle => vec2(391.0, 128.0),
        Pop::Opacity => vec2(275.0, 67.5),
    };
    let (r, tip, below) = place_pop(ed, anchor, size, !from_palette);
    let outer = r.union(Rect::from_center_size(tip, vec2(ARROW_W, 2.0)));
    let resp = egui::Area::new(egui::Id::unique("nt-markup-pop")).order(egui::Order::Tooltip).fade_in(false).fixed_pos(outer.min).show(ctx, |ui| {
        let _ = ui.allocate_exact_size(outer.size(), Sense::click());
        let p = ui.painter().clone();
        panel(&p, r, tip, below, !from_palette);
        match pop {
            Pop::Tools => tools_pop(ui, &p, r, st, act),
            Pop::Color => color_pop(ui, &p, r, act),
            Pop::Shapes => shapes_pop(ui, &p, r, st, act),
            Pop::Sign => sign_pop(ui, &p, r, &sigs, act),
            Pop::Fill | Pop::Stroke | Pop::TextColor => swatch_pop(ui, &p, r, pop, &style, act),
            Pop::TextStyle => text_style_pop(ui, &p, r, &style, act),
            Pop::Opacity => opacity_pop(ui, &p, r, ed, act),
        }
    });
    Some(resp.response.rect)
}

fn close() -> Box<dyn FnOnce(&mut Editor)> {
    Box::new(|e: &mut Editor| {
        if let Some(s) = e.draw.as_mut() {
            s.pop = None;
        }
    })
}

/// The tool picker (NO3): five sizes on a grey strip (the chosen one black), then the inks and the
/// eraser, 32 pt rows, the current one ticked; the eraser's sizes are rings, with Pixel | Object.
fn tools_pop(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, st: &draw::DrawState, act: &mut Acts) {
    let erasing = matches!(st.tool, Tool::PixelEraser | Tool::ObjectEraser);
    let kind = if let Tool::Ink(k) = st.tool { k } else { st.last_ink };
    let strip = Rect::from_min_size(r.min + vec2(3.5, 3.5), vec2(164.0, if erasing { 70.5 } else { 39.0 }));
    p.rect_filled(strip, CornerRadius::same(12), if dark() { Color32::from_gray(70) } else { Color32::from_gray(236) });
    let cur = if erasing { st.eraser_size } else { st.widths.iter().find(|w| w.0 == kind).map_or(0, |w| w.1) };
    for i in 0..5 {
        let c = pos2(r.min.x + 23.5 + i as f32 * 31.0, strip.min.y + 19.5);
        let cell = Rect::from_center_size(c, vec2(28.0, 28.0));
        let resp = ui.interact(cell, egui::Id::unique(("nt-mk-size", i)), Sense::click());
        let on = i == cur;
        if on {
            p.rect_filled(cell, CornerRadius::same(8), ink());
        } else if resp.hovered() {
            p.rect_filled(cell, CornerRadius::same(8), Color32::from_black_alpha(14));
        }
        let col = if on { if dark() { Color32::BLACK } else { Color32::WHITE } } else { ink() };
        if erasing {
            p.circle_stroke(c, 4.5 + i as f32 * 1.0, Line::new(1.1, if on { col } else { Color32::from_gray(128) }));
        } else {
            size_glyph(p, c, kind, i, col);
        }
        if resp.clicked() {
            act.push(Box::new(move |e: &mut Editor| {
                if let Some(s) = e.draw.as_mut() {
                    if erasing {
                        s.eraser_size = i;
                    } else {
                        match s.widths.iter_mut().find(|x| x.0 == kind) {
                            Some(x) => x.1 = i,
                            None => s.widths.push((kind, i)),
                        }
                    }
                }
            }));
        }
    }
    if erasing {
        // Pixel | Object
        let seg = Rect::from_min_size(pos2(r.min.x + 11.5, r.min.y + 43.0), vec2(148.0, 28.0));
        p.rect_filled(seg, CornerRadius::same(8), if dark() { Color32::from_gray(60) } else { Color32::from_gray(222) });
        for (k, (t, label)) in [(Tool::PixelEraser, "Pixel"), (Tool::ObjectEraser, "Object")].into_iter().enumerate() {
            let half = Rect::from_min_size(pos2(seg.min.x + k as f32 * 74.0, seg.min.y), vec2(74.0, 28.0));
            let resp = ui.interact(half, egui::Id::unique(("nt-mk-erase", k)), Sense::click());
            if st.tool == t {
                p.rect_filled(half.shrink(2.0), CornerRadius::same(7), if dark() { Color32::from_gray(95) } else { Color32::from_gray(250) });
            }
            p.text(half.center(), Align2::CENTER_CENTER, label, font(14.0), if st.tool == t { ink() } else { ink().gamma_multiply(0.75) });
            if resp.clicked() {
                act.push(Box::new(move |e: &mut Editor| {
                    if let Some(s) = e.draw.as_mut() {
                        s.tool = t;
                        s.pen_tool = t;
                        s.eraser_object = t == Tool::ObjectEraser;
                    }
                }));
            }
        }
    }
    let top = r.min.y + if erasing { 98.5 } else { 62.5 };
    let rows: Vec<(Option<InkKind>, &str)> = INKS.iter().map(|(k, n)| (Some(*k), *n)).chain(std::iter::once((None, "Eraser"))).collect();
    for (i, (k, name)) in rows.into_iter().enumerate() {
        let cy = top + i as f32 * 32.0;
        // the hover highlight, inset so it clears the panel's rounded corners
        let row = Rect::from_min_max(pos2(r.min.x + 7.0, cy - 12.0), pos2(r.max.x - 7.0, cy + 12.0));
        let resp = ui.interact(row, egui::Id::unique(("nt-mk-ink", i)), Sense::click());
        if resp.hovered() {
            p.rect_filled(row, CornerRadius::same(7), Color32::from_black_alpha(10));
        }
        let ticked = match k {
            Some(k) => st.tool == Tool::Ink(k) || !matches!(st.tool, Tool::Ink(_) | Tool::PixelEraser | Tool::ObjectEraser) && st.last_ink == k && !matches!(st.pen_tool, Tool::PixelEraser | Tool::ObjectEraser),
            None => erasing,
        };
        if ticked {
            p.text(pos2(r.min.x + 7.5, cy), Align2::LEFT_CENTER, "✓", font(20.0), ink());
        }
        let t = match k {
            Some(k) => Tool::Ink(k),
            None => Tool::PixelEraser,
        };
        tool_glyph(p, pos2(r.min.x + 34.0, cy), t, 1.0, ink());
        p.text(pos2(r.min.x + 48.0, cy), Align2::LEFT_CENTER, name, font(15.0), ink());
        if resp.clicked() {
            act.push(Box::new(move |e: &mut Editor| {
                if let Some(s) = e.draw.as_mut() {
                    // the eraser comes back as the kind chosen last
                    let t = if t == Tool::PixelEraser && s.eraser_object { Tool::ObjectEraser } else { t };
                    s.tool = t;
                    s.pen_tool = t;
                    if let Tool::Ink(k) = t {
                        s.last_ink = k;
                    }
                    s.clear_selection();
                    s.pop = None;
                }
            }));
        }
    }
}

/// The colour dot's popover (NO3): the strip, the grid, Show Colors….
fn color_pop(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, act: &mut Acts) {
    let mut cell = |rect: Rect, u: Option<u32>, id: (u8, usize)| {
        let resp = ui.interact(rect, egui::Id::unique(("nt-mk-col", id.0, id.1)), Sense::click());
        if let Some(u) = u {
            // (Notes marks no cell as the current colour: the dot shows it)
            if resp.hovered() {
                p.rect_stroke(rect.shrink(0.5), CornerRadius::same(2), Line::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
            }
            if resp.clicked() {
                let v = rgb(u);
                act.push(Box::new(move |e: &mut Editor| set_color(e, ColorTarget::Ink, v)));
                act.push(close());
            }
        }
    };
    let strip = Rect::from_min_size(r.min + vec2(12.5, 12.5), vec2(216.0, 17.5));
    p.rect_filled(strip, CornerRadius::same(5), Color32::WHITE);
    for (i, u) in STRIP.iter().enumerate() {
        let cr = Rect::from_min_size(pos2(strip.min.x + i as f32 * 18.0, strip.min.y), vec2(18.0, 17.5));
        let rad = CornerRadius { nw: if i == 0 { 5 } else { 0 }, sw: if i == 0 { 5 } else { 0 }, ne: if i == 11 { 5 } else { 0 }, se: if i == 11 { 5 } else { 0 } };
        p.rect_filled(cr, rad, c32(rgb(*u)));
        cell(cr, Some(*u), (0, i));
    }
    p.rect_stroke(strip, CornerRadius::same(5), Line::new(0.5, Color32::from_black_alpha(30)), egui::StrokeKind::Outside);
    let grid = Rect::from_min_size(r.min + vec2(12.5, 44.5), vec2(216.0, 180.0));
    p.rect_filled(grid, CornerRadius::same(6), Color32::WHITE);
    for (row, line) in GRID.iter().enumerate() {
        for (col, u) in line.iter().enumerate() {
            let cr = Rect::from_min_size(pos2(grid.min.x + col as f32 * 18.0, grid.min.y + row as f32 * 18.0), vec2(18.0, 18.0));
            let rad = CornerRadius {
                nw: if row == 0 && col == 0 { 6 } else { 0 },
                ne: if row == 0 && col == 11 { 6 } else { 0 },
                sw: if row == 9 && col == 0 { 6 } else { 0 },
                se: if row == 9 && col == 11 { 6 } else { 0 },
            };
            if row == 0 && col == 0 {
                // none: a red slash (the ink can't be none)
                p.line_segment([cr.left_bottom() + vec2(4.0, -4.0), cr.right_top() + vec2(-4.0, 4.0)], Line::new(1.5, Color32::from_rgb(235, 76, 70)));
                continue;
            }
            p.rect_filled(cr, rad, c32(rgb(*u)));
            cell(cr, Some(*u), (1, row * 12 + col));
        }
    }
    p.rect_stroke(grid, CornerRadius::same(6), Line::new(0.5, Color32::from_black_alpha(30)), egui::StrokeKind::Outside);
    let b = Rect::from_center_size(pos2(r.center().x, r.min.y + 248.5), vec2(94.5, 20.0));
    if flat_button(ui, p, b, "Show Colors…", 11.5, "nt-mk-showcolors") {
        act.push(Box::new(|e: &mut Editor| {
            if let Some(s) = e.draw.as_mut() {
                s.colors = Some(ColorTarget::Ink);
                s.pop = None;
            }
        }));
    }
}

fn flat_button(ui: &mut egui::Ui, p: &egui::Painter, b: Rect, label: &str, size: f32, id: &str) -> bool {
    let resp = ui.interact(b, egui::Id::unique(id), Sense::click());
    let base = if dark() { 80 } else { 232 };
    p.rect_filled(b, CornerRadius::same(6), Color32::from_gray(if resp.hovered() { base - 12 } else { base }));
    p.text(b.center(), Align2::CENTER_CENTER, label, font(size), ink().gamma_multiply(0.85));
    resp.clicked()
}

/// Shapes (NO3): two columns — line, arrow, rectangle, rounded rectangle, oval, speech bubble,
/// star, polygon.
fn shapes_pop(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, st: &draw::DrawState, act: &mut Acts) {
    const KINDS: [(ShapeKind, &str); 8] = [
        (ShapeKind::Line, "Line"),
        (ShapeKind::Arrow, "Arrow"),
        (ShapeKind::Rect, "Rectangle"),
        (ShapeKind::RoundRect, "Rounded Rectangle"),
        (ShapeKind::Oval, "Oval"),
        (ShapeKind::Bubble, "Speech Bubble"),
        (ShapeKind::Star, "Star"),
        (ShapeKind::Polygon, "Polygon"),
    ];
    for (i, (k, name)) in KINDS.iter().enumerate() {
        let c = pos2(r.min.x + 22.5 + (i % 2) as f32 * 30.0, r.min.y + 18.5 + (i / 2) as f32 * 31.0);
        let cell = Rect::from_center_size(c, vec2(26.0, 26.0));
        let resp = ui.interact(cell, egui::Id::unique(("nt-mk-shape", i)), Sense::click()).on_hover_text(*name);
        if resp.hovered() {
            p.rect_filled(cell, CornerRadius::same(6), Color32::from_black_alpha(14));
        }
        shape_glyph(p, Rect::from_center_size(c, vec2(15.5, 15.5)), *k, if dark() { Color32::from_gray(220) } else { Color32::from_gray(64) });
        if resp.clicked() {
            let (k, s) = (*k, st.style.clone());
            act.push(Box::new(move |e: &mut Editor| {
                let mut o = shapes::new_shape(k, Pos2::ZERO, &s);
                // Notes' polygon is a pentagon
                if k == ShapeKind::Polygon {
                    o.sides = 5;
                }
                e.add_shape(o);
            }));
        }
    }
}

/// Signatures (NO3, its lower part): the saved ones, a line, Create Signature.
fn sign_pop(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, sigs: &[(String, ink::Drawing)], act: &mut Acts) {
    for (i, (name, d)) in sigs.iter().enumerate() {
        let row = Rect::from_min_size(pos2(r.min.x + 6.0, r.min.y + 6.0 + i as f32 * 64.0), vec2(r.width() - 12.0, 60.0));
        let resp = ui.interact(row, egui::Id::unique(("nt-sig", name)), Sense::click());
        if resp.hovered() {
            p.rect_filled(row, CornerRadius::same(8), Color32::from_black_alpha(10));
        }
        signature_thumb(p, row.shrink2(vec2(18.0, 8.0)), &d.strokes, ink());
        let x = Rect::from_center_size(pos2(row.max.x - 14.0, row.min.y + 14.0), vec2(16.0, 16.0));
        let mut deleted = false;
        if resp.hovered() || ui.rect_contains_pointer(x) {
            let xr = ui.interact(x, egui::Id::unique(("nt-sig-x", name)), Sense::click());
            p.circle_filled(x.center(), 7.0, Color32::from_gray(if xr.hovered() { 120 } else { 165 }));
            p.text(x.center(), Align2::CENTER_CENTER, "×", font(12.0), Color32::WHITE);
            if xr.clicked() {
                deleted = true;
                let n = name.clone();
                act.push(Box::new(move |_e: &mut Editor| draw::delete_signature(&n)));
            }
        }
        if resp.clicked() && !deleted {
            let strokes = d.strokes.clone();
            act.push(Box::new(move |e: &mut Editor| {
                if let Some(s) = shapes::new_signature(&strokes, Pos2::ZERO, 160.0, [0.0, 0.0, 0.0, 1.0]) {
                    e.add_shape(s);
                }
            }));
        }
    }
    let line_y = r.max.y - 34.5;
    p.hline(r.x_range(), line_y, Line::new(0.5, Color32::from_black_alpha(26)));
    let b = Rect::from_center_size(pos2(r.center().x, r.max.y - 17.0), vec2(125.0, 24.0));
    if flat_button(ui, p, b, "Create Signature", 13.0, "nt-mk-create-sig") {
        act.push(Box::new(|e: &mut Editor| {
            if let Some(s) = e.draw.as_mut() {
                s.signing = Some(Signing::default());
                s.pop = None;
            }
        }));
    }
}

/// Fill / Stroke / a text box's colour (NO3): two rows of six swatches, No Fill / No Stroke and
/// the colour wheel; a stroke has its width stepper.
fn swatch_pop(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, pop: Pop, style: &shapes::Style, act: &mut Acts) {
    let (target, cur) = match pop {
        Pop::Fill => (ColorTarget::Fill, style.fill),
        Pop::Stroke => (ColorTarget::Stroke, style.stroke),
        _ => (ColorTarget::Text, style.text_color),
    };
    for (i, u) in SWATCHES.iter().enumerate() {
        let c = pos2(r.min.x + 29.5 + (i % 6) as f32 * 36.0, r.min.y + 27.5 + (i / 6) as f32 * 36.0);
        let resp = ui.interact(Rect::from_center_size(c, vec2(30.0, 30.0)), egui::Id::unique(("nt-mk-sw", i)), Sense::click());
        let v = rgb(*u);
        let col = c32(v);
        let same = cur[3] > 0.01 && (0..3).all(|k| (v[k] - cur[k]).abs() < 0.005);
        if same {
            // the chosen one: a ring of its colour round a white rim
            p.circle_filled(c, 12.0, col);
            p.circle_filled(c, 9.0, Color32::WHITE);
            p.circle_filled(c, 6.5, col);
        } else {
            p.circle_filled(c, if resp.hovered() { 12.5 } else { 12.0 }, col);
        }
        if *u == 0xffffff {
            p.circle_stroke(c, 12.0, Line::new(0.6, Color32::from_gray(180)));
        }
        if resp.clicked() {
            let v = [v[0], v[1], v[2], if cur[3] > 0.01 { cur[3] } else { 1.0 }];
            act.push(Box::new(move |e: &mut Editor| set_color(e, target, v)));
        }
    }
    let y = r.min.y + 108.0;
    if target != ColorTarget::Text {
        let pill = Rect::from_min_max(pos2(r.min.x + 17.5, y - 15.0), pos2(r.min.x + 191.5, y + 15.0));
        let resp = ui.interact(pill, egui::Id::unique("nt-mk-none"), Sense::click());
        p.rect_filled(pill, CornerRadius::same(15), Color32::from_rgb(209, 209, 210).gamma_multiply(if resp.hovered() { 0.9 } else { 1.0 }));
        p.text(pill.center(), Align2::CENTER_CENTER, if target == ColorTarget::Fill { "No Fill" } else { "No Stroke" }, font(15.0), ink());
        if resp.clicked() {
            act.push(Box::new(move |e: &mut Editor| set_color(e, target, [0.0; 4])));
        }
    }
    // the colour wheel: the Colors window
    let wc = pos2(r.min.x + 216.5, y);
    let resp = ui.interact(Rect::from_center_size(wc, vec2(28.0, 28.0)), egui::Id::unique("nt-mk-wheel"), Sense::click());
    wheel(p, wc, 12.0);
    p.circle_filled(wc, 7.5, Color32::WHITE);
    p.circle_filled(wc, 5.5, if cur[3] > 0.01 { c32(cur) } else { Color32::from_gray(190) });
    if resp.clicked() {
        act.push(Box::new(move |e: &mut Editor| {
            if let Some(s) = e.draw.as_mut() {
                s.colors = Some(target);
                s.pop = None;
            }
        }));
    }
    if pop == Pop::Stroke && cur[3] > 0.01 {
        // the width: [−] n pt [+]
        let cy = r.min.y + 150.0;
        let minus = Rect::from_min_max(pos2(r.min.x + 17.5, cy - 15.0), pos2(r.min.x + 58.5, cy + 15.0));
        let field = Rect::from_min_max(pos2(r.min.x + 70.5, cy - 15.0), pos2(r.min.x + 172.5, cy + 15.0));
        let plus = Rect::from_min_max(pos2(r.min.x + 183.5, cy - 15.0), pos2(r.min.x + 225.0, cy + 15.0));
        let w = style.line_width;
        for (b, sign) in [(minus, -1.0f32), (plus, 1.0)] {
            let resp = ui.interact(b, egui::Id::unique(("nt-mk-step", sign as i32)), Sense::click());
            p.rect_filled(b, CornerRadius::same(8), Color32::from_gray(if resp.hovered() { 200 } else { 214 }));
            p.text(b.center(), Align2::CENTER_CENTER, if sign < 0.0 { "−" } else { "+" }, font(20.0), ink());
            if resp.clicked() {
                let nw = (w + sign).clamp(1.0, 50.0);
                act.push(Box::new(move |e: &mut Editor| e.restyle(&move |st, o| match o {
                    Some(o) => o.line_width = nw,
                    None => st.line_width = nw,
                })));
            }
        }
        p.rect_filled(field, CornerRadius::same(8), Color32::from_gray(214));
        p.rect_filled(field.shrink(2.5), CornerRadius::same(6), Color32::from_gray(236));
        p.text(field.center(), Align2::CENTER_CENTER, format!("{} pt", w.round() as i32), font(14.0), ink());
    }
}

/// Text style (NO3): B I U S and the colour well; [−] size ⌄ [+] and the font; the alignments.
fn text_style_pop(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, style: &shapes::Style, act: &mut Acts) {
    let seg_bg = if dark() { Color32::from_gray(80) } else { Color32::from_gray(214) };
    let fonts = layout::markup_fonts();
    let bold = fonts.iter().any(|f| f.2 == style.font);
    // B I U S (italic, underline and strike have no place in the Markup object yet: shown, off)
    let row1 = Rect::from_min_max(pos2(r.min.x + 18.0, r.min.y + 18.0), pos2(r.min.x + 281.5, r.min.y + 42.0));
    p.rect_filled(row1, CornerRadius::same(7), seg_bg);
    for (i, label) in ["B", "I", "U", "S"].into_iter().enumerate() {
        let sr = Rect::from_min_size(pos2(row1.min.x + i as f32 * row1.width() / 4.0, row1.min.y), vec2(row1.width() / 4.0, row1.height()));
        if i > 0 {
            p.vline(sr.min.x, sr.y_range().shrink(4.0), Line::new(0.5, Color32::from_black_alpha(30)));
        }
        let resp = ui.interact(sr, egui::Id::unique(("nt-mk-bius", i)), Sense::click());
        if i == 0 && bold {
            p.rect_filled(sr.shrink(2.0), CornerRadius::same(6), Color32::from_gray(250));
        }
        let fam = match i {
            0 => layout::family("Sfera-Bold"),
            1 => layout::family("Sfera-Italic"),
            _ => layout::family("Sfera-Regular"),
        };
        let col = if i == 0 { ink() } else { ink().gamma_multiply(0.55) };
        let g = p.layout_no_wrap(label.into(), FontId::new(15.0, fam), col);
        let at = sr.center() - g.size() / 2.0;
        let gw = g.size().x;
        p.galley(at, g, col);
        if i == 2 {
            p.hline(at.x..=at.x + gw, at.y + 17.0, Line::new(1.0, col));
        }
        if i == 3 {
            p.hline(at.x - 1.0..=at.x + gw + 1.0, sr.center().y, Line::new(1.0, col));
        }
        if resp.clicked() && i == 0 {
            if let Some(f) = fonts.iter().find(|f| f.1 == style.font || f.2 == style.font) {
                let ps = if bold { f.1 } else { f.2 }.to_string();
                act.push(Box::new(move |e: &mut Editor| e.restyle(&move |st, o| match o {
                    Some(o) => o.font = ps.clone(),
                    None => st.font = ps.clone(),
                })));
            }
        }
    }
    // the colour well
    let well = Rect::from_center_size(pos2(r.min.x + 338.5, r.min.y + 30.0), vec2(45.0, 18.0));
    let resp = ui.interact(well.expand(4.0), egui::Id::unique("nt-mk-twell"), Sense::click());
    p.rect_filled(well.expand(3.0), CornerRadius::same(12), Color32::from_gray(214));
    p.rect_filled(well, CornerRadius::same(9), c32(style.text_color));
    if resp.clicked() {
        act.push(Box::new(|e: &mut Editor| {
            if let Some(s) = e.draw.as_mut() {
                s.pop = Some(Pop::TextColor);
            }
        }));
    }
    // [−] size ⌄ [+] font ⌄
    let cy = r.min.y + 64.0;
    let size = style.font_size;
    let set_size = |v: f32| -> Box<dyn FnOnce(&mut Editor)> {
        Box::new(move |e: &mut Editor| e.restyle(&move |st, o| match o {
            Some(o) => o.font_size = v,
            None => st.font_size = v,
        }))
    };
    for (x0, x1, sign) in [(25.0, 92.5, -1.0f32), (165.0, 233.0, 1.0)] {
        let b = Rect::from_min_max(pos2(r.min.x + x0, cy - 12.0), pos2(r.min.x + x1, cy + 12.0));
        let resp = ui.interact(b, egui::Id::unique(("nt-mk-tsz", sign as i32)), Sense::click());
        p.rect_filled(b, CornerRadius::same(7), if resp.hovered() { seg_bg.gamma_multiply(0.92) } else { seg_bg });
        p.text(b.center(), Align2::CENTER_CENTER, if sign < 0.0 { "−" } else { "+" }, font(19.0), ink());
        if resp.clicked() {
            let next = if sign > 0.0 { TEXT_SIZES.iter().copied().find(|s| *s > size + 0.1).unwrap_or(size + 1.0) } else { TEXT_SIZES.iter().rev().copied().find(|s| *s < size - 0.1).unwrap_or((size - 1.0).max(6.0)) };
            act.push(set_size(next));
        }
    }
    let field = Rect::from_min_max(pos2(r.min.x + 97.5, cy - 12.0), pos2(r.min.x + 161.0, cy + 12.0));
    p.rect_filled(field, CornerRadius::same(7), seg_bg);
    ui.scope_builder(egui::UiBuilder::new().max_rect(field), |ui| {
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Line::NONE;
        egui::ComboBox::from_id_salt("nt-mk-size").icon(|ui, r, v, _| chevron(ui.painter(), r.center(), v.fg_stroke.color)).selected_text(egui::RichText::new(format!("{}", size.round() as i32)).size(15.0)).width(field.width() - 4.0).show_ui(ui, |ui| {
            for s in TEXT_SIZES {
                if ui.selectable_label((s - size).abs() < 0.1, format!("{}", s as i32)).clicked() {
                    act.push(set_size(s));
                }
            }
        });
    });
    let fr = Rect::from_min_max(pos2(r.min.x + 244.0, cy - 12.0), pos2(r.min.x + 379.0, cy + 12.0));
    let shown = fonts.iter().find(|f| f.1 == style.font || f.2 == style.font).map_or(style.font.as_str(), |f| f.0);
    p.rect_filled(fr, CornerRadius::same(7), seg_bg);
    ui.scope_builder(egui::UiBuilder::new().max_rect(fr), |ui| {
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Line::NONE;
        egui::ComboBox::from_id_salt("nt-mk-font").icon(|ui, r, v, _| chevron(ui.painter(), r.center(), v.fg_stroke.color)).selected_text(egui::RichText::new(shown).size(15.0)).width(fr.width() - 4.0).show_ui(ui, |ui| {
            for (name, reg, bld) in &fonts {
                let ps = if bold { *bld } else { *reg };
                if ui.selectable_label(*reg == style.font || *bld == style.font, egui::RichText::new(*name).family(layout::markup_family(reg))).clicked() {
                    let ps = ps.to_string();
                    act.push(Box::new(move |e: &mut Editor| e.restyle(&move |st, o| match o {
                        Some(o) => o.font = ps.clone(),
                        None => st.font = ps.clone(),
                    })));
                }
            }
        });
    });
    // left / centre / right / justified
    let row3 = Rect::from_min_max(pos2(r.min.x + 25.0, r.min.y + 86.0), pos2(r.min.x + 379.0, r.min.y + 110.0));
    p.rect_filled(row3, CornerRadius::same(7), seg_bg);
    for a in 0..4u8 {
        let sr = Rect::from_min_size(pos2(row3.min.x + a as f32 * row3.width() / 4.0, row3.min.y), vec2(row3.width() / 4.0, row3.height()));
        if a > 0 {
            p.vline(sr.min.x, sr.y_range().shrink(4.0), Line::new(0.5, Color32::from_black_alpha(30)));
        }
        let resp = ui.interact(sr, egui::Id::unique(("nt-mk-align", a)), Sense::click());
        if resp.hovered() {
            p.rect_filled(sr.shrink(2.0), CornerRadius::same(6), Color32::from_black_alpha(10));
        }
        align_glyph(p, Rect::from_center_size(sr.center(), vec2(14.0, 11.0)), a, ink());
        if resp.clicked() {
            act.push(Box::new(move |e: &mut Editor| e.restyle(&move |st, o| match o {
                Some(o) => o.align = a,
                None => st.align = a,
            })));
        }
    }
}

/// Opacity (NO3): one slider, the accent up to the knob.
fn opacity_pop(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, ed: &Editor, act: &mut Acts) {
    let Some(o) = selected_one(ed) else { return };
    let track = Rect::from_min_max(pos2(r.min.x + 12.5, r.center().y - 3.5), pos2(r.max.x - 12.5, r.center().y + 3.5));
    let v = shapes::opacity(&o).clamp(0.0, 1.0);
    let resp = ui.interact(track.expand2(vec2(10.0, 12.0)), egui::Id::unique("nt-mk-opacity"), Sense::click_and_drag());
    let v = match resp.interact_pointer_pos() {
        Some(q) if resp.dragged() || resp.clicked() => ((q.x - track.min.x) / track.width()).clamp(0.02, 1.0),
        _ => v,
    };
    let x = track.min.x + v * track.width();
    p.rect_filled(track, CornerRadius::same(4), Color32::from_gray(205));
    p.rect_filled(Rect::from_min_max(track.min, pos2(x, track.max.y)), CornerRadius::same(4), layout::accent());
    let knob = Rect::from_center_size(pos2(x.clamp(track.min.x + 9.5, track.max.x - 9.5), track.center().y), vec2(19.0, 14.0));
    p.rect_filled(knob.translate(vec2(0.0, 1.0)).expand(1.0), CornerRadius::same(8), Color32::from_black_alpha(30));
    p.rect_filled(knob, CornerRadius::same(7), Color32::WHITE);
    if resp.drag_started() || resp.clicked() {
        act.push(Box::new(|e: &mut Editor| e.draw_change()));
    }
    if resp.dragged() || resp.clicked() {
        let id = o.id.clone();
        act.push(Box::new(move |e: &mut Editor| {
            if let Some(d) = e.drawing_mut() {
                if let Some(s) = d.shapes.iter_mut().find(|s| s.id == id) {
                    shapes::set_opacity(s, v);
                }
            }
            e.changed = true;
        }));
    }
}

fn wheel(p: &egui::Painter, c: Pos2, r: f32) {
    let n = 36;
    let mut m = egui::Mesh::default();
    for i in 0..n {
        let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
        let rgb = kn_ui::colors::hsv_to_rgb(i as f32 / n as f32, 0.75, 1.0);
        let col = Color32::from_rgb((rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8);
        let b = m.vertices.len() as u32;
        m.colored_vertex(c, col);
        m.colored_vertex(c + vec2(a0.cos(), a0.sin()) * r, col);
        m.colored_vertex(c + vec2(a1.cos(), a1.sin()) * r, col);
        m.add_triangle(b, b + 1, b + 2);
    }
    p.add(egui::Shape::mesh(m));
}

fn ink_color() -> Color32 {
    if dark() { Color32::from_gray(225) } else { Color32::from_gray(60) }
}

fn panel_bg() -> Color32 {
    if dark() { Color32::from_gray(48) } else { Color32::from_gray(250) }
}

// ---- the text field over a text box, Create Signature

fn text_field(ed: &mut Editor, ui: &mut egui::Ui, act: &mut Acts) {
    let Some(id) = ed.draw.as_ref().and_then(|s| s.text_edit.clone()) else { return };
    let Some((rect, scale)) = draw::canvas(ed) else { return };
    let Some(s) = ed.drawing().and_then(|d| d.shapes.iter().find(|s| s.id == id)).cloned() else { return };
    let f = shapes::frame(&s);
    let r = Rect::from_min_size(rect.min + f.min.to_vec2() * scale, f.size() * scale);
    // a shape's own text sits in its middle
    let dy = if s.kind == ShapeKind::Text { shapes::TEXT_PAD * scale } else { (r.height() - shapes::text_galley(ui.ctx(), &s, scale).size().y) / 2.0 };
    let mut text = s.text.clone();
    let tid = egui::Id::unique(("nt-markup-text", &id));
    egui::Area::new(egui::Id::unique("nt-markup-textarea")).order(egui::Order::Foreground).fade_in(false).fixed_pos(r.min + vec2(shapes::TEXT_PAD * scale, dy)).show(ui.ctx(), |ui| {
        let mut safe = super::find::SafeText(&mut text);
        let te = egui::TextEdit::multiline(&mut safe)
            .id(tid)
            .font(shapes::text_font(&s, s.font_size * scale))
            .text_color(c32(s.text_color))
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_rows(1)
            .desired_width(((f.width() - 2.0 * shapes::TEXT_PAD) * scale).max(8.0))
            .align(match s.align {
                1 => Align2::CENTER_TOP,
                2 => Align2::RIGHT_TOP,
                _ => Align2::LEFT_TOP,
            });
        let out = te.show(ui);
        let resp = out.response;
        if !resp.has_focus() && ui.memory(|m| m.focused().is_none()) {
            resp.request_focus();
        }
        // a new box's "Text" starts selected, so typing replaces it
        let fresh = egui::Id::unique(("nt-markup-text-fresh", &id));
        if !ui.data(|d| d.get_temp::<bool>(fresh).unwrap_or(false)) {
            ui.data_mut(|d| d.insert_temp(fresh, true));
            let mut state = out.state;
            let n = s.text.chars().count();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
            state.store(ui.ctx(), tid);
        }
        // the box while it is typed in
        ui.painter().rect_stroke(r, 0.0, Line::new(1.0, Color32::from_rgb(10, 132, 255).gamma_multiply(0.6)), egui::StrokeKind::Outside);
    });
    if text != s.text {
        act.push(Box::new(move |e: &mut Editor| {
            let ctx = shapes::CTX.with_borrow(|c| c.clone());
            if let Some(d) = e.drawing_mut() {
                let cw = d.width;
                if let Some(o) = d.shapes.iter_mut().find(|o| o.id == id) {
                    o.text = text;
                    if let Some(ctx) = ctx {
                        // the box widens to its longest line (to the canvas' edge), then wraps
                        let natural = shapes::text_natural_width(&ctx, o) + 2.0 * shapes::TEXT_PAD + 2.0;
                        if o.kind == ShapeKind::Text && natural > o.frame[2] && o.rotation == 0.0 {
                            o.frame[2] = natural.min((cw - o.frame[0]).max(o.frame[2]));
                        }
                        if o.kind == ShapeKind::Text {
                            o.frame[3] = shapes::text_height(&ctx, o);
                        }
                    }
                }
            }
            e.changed = true;
        }));
    }
}

/// Create Signature: sign on the pad, then Done saves it with the app's signatures and puts it
/// in. Returns the sheet's rectangle.
fn sign_sheet(ed: &mut Editor, ui: &mut egui::Ui, act: &mut Acts) -> Option<Rect> {
    let v = ed.view.rect?;
    let size = vec2(460.0, 270.0);
    let sheet = Rect::from_center_size(pos2(v.center().x, v.min.y + 70.0 + size.y / 2.0), size);
    let ctx = ui.ctx().clone();
    let time = ctx.input(|i| i.time);
    let resp = egui::Area::new(egui::Id::unique("nt-markup-sign")).order(egui::Order::Foreground).fade_in(false).fixed_pos(sheet.min).show(&ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(size, Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(r.translate(vec2(0.0, 6.0)).expand(6.0), CornerRadius::same(18), Color32::from_black_alpha(26));
        p.rect_filled(r, CornerRadius::same(12), panel_bg());
        p.rect_stroke(r, CornerRadius::same(12), Line::new(0.5, Color32::from_black_alpha(40)), egui::StrokeKind::Inside);
        let ink = ink_color();
        p.text(pos2(r.center().x, r.min.y + 20.0), Align2::CENTER_CENTER, "Create Signature", FontId::new(13.0, layout::family("Sfera-Semibold")), ink);
        p.text(pos2(r.center().x, r.min.y + 40.0), Align2::CENTER_CENTER, "Sign your name with the mouse or trackpad.", FontId::new(11.5, layout::family("Sfera-Regular")), ink.gamma_multiply(0.6));
        let pad = Rect::from_min_size(pos2(r.min.x + 20.0, r.min.y + 56.0), vec2(size.x - 40.0, 150.0));
        p.rect_filled(pad, CornerRadius::same(8), Color32::WHITE);
        p.rect_stroke(pad, CornerRadius::same(8), Line::new(0.8, Color32::from_gray(190)), egui::StrokeKind::Inside);
        p.line_segment([pos2(pad.min.x + 24.0, pad.min.y + 112.0), pos2(pad.max.x - 24.0, pad.min.y + 112.0)], Line::new(1.0, Color32::from_gray(170)));
        let pr = ui.interact(pad, egui::Id::unique("nt-sign-pad"), Sense::drag());
        let st = ed.draw.as_mut().and_then(|s| s.signing.as_mut());
        if let Some(sg) = st {
            let to_pad = |q: Pos2| pos2(q.x - pad.min.x, q.y - pad.min.y);
            if let Some(q) = pr.interact_pointer_pos() {
                if pr.drag_started() {
                    sg.building = Some(Builder::new(InkKind::Pen, [0.0, 0.0, 0.0, 1.0], 2.5, to_pad(q), time, None));
                } else if pr.dragged() {
                    if let Some(b) = sg.building.as_mut() {
                        b.add(to_pad(q), time, None);
                    }
                }
            }
            if pr.drag_stopped() {
                if let Some(b) = sg.building.take() {
                    sg.strokes.push(b.finish());
                }
            }
            let clip = p.with_clip_rect(pad);
            let to_screen = |q: Pos2| pad.min + q.to_vec2();
            for s in sg.strokes.iter().chain(sg.building.as_ref().map(|b| &b.stroke)) {
                ink::paint_stroke(&clip, s, &to_screen, 1.0);
            }
            if sg.building.is_some() {
                ctx.request_repaint();
            }
        }
        // Clear, then Cancel and Done
        let by = r.max.y - 26.0;
        let empty = ed.draw.as_ref().and_then(|s| s.signing.as_ref()).is_none_or(|s| s.strokes.is_empty());
        let button = |ui: &mut egui::Ui, at: Pos2, w: f32, label: &str, primary: bool, enabled: bool| -> bool {
            let br = Rect::from_center_size(at, vec2(w, 24.0));
            let resp = ui.interact(br, egui::Id::unique(("nt-sign-btn", label.to_string())), Sense::click());
            let fill = if primary && enabled { layout::accent() } else if layout::dark() { Color32::from_gray(80) } else { Color32::from_gray(232) };
            p.rect_filled(br, CornerRadius::same(6), if resp.hovered() && enabled { fill.gamma_multiply(0.92) } else { fill });
            let tc = if primary && enabled { Color32::WHITE } else if enabled { ink } else { ink.gamma_multiply(0.4) };
            p.text(br.center(), Align2::CENTER_CENTER, label, FontId::new(13.0, layout::family("Sfera-Regular")), tc);
            resp.clicked() && enabled
        };
        if button(ui, pos2(r.min.x + 60.0, by), 80.0, "Clear", false, !empty) {
            act.push(Box::new(|e: &mut Editor| {
                if let Some(s) = e.draw.as_mut().and_then(|s| s.signing.as_mut()) {
                    s.strokes.clear();
                }
            }));
        }
        if button(ui, pos2(r.max.x - 150.0, by), 80.0, "Cancel", false, true) {
            act.push(Box::new(|e: &mut Editor| {
                if let Some(s) = e.draw.as_mut() {
                    s.signing = None;
                }
            }));
        }
        if button(ui, pos2(r.max.x - 60.0, by), 80.0, "Done", true, !empty) {
            act.push(Box::new(|e: &mut Editor| {
                let Some(sg) = e.draw.as_mut().and_then(|s| s.signing.take()) else { return };
                draw::save_signature(&sg.strokes);
                if let Some(s) = shapes::new_signature(&sg.strokes, Pos2::ZERO, 160.0, [0.0, 0.0, 0.0, 1.0]) {
                    e.add_shape(s);
                }
            }));
        }
    });
    Some(resp.response.rect)
}

// ---- symbols (drawn after the NO3 captures)

fn chevron(p: &egui::Painter, c: Pos2, col: Color32) {
    p.add(egui::Shape::line(vec![c + vec2(-3.2, -1.6), c + vec2(0.0, 1.6), c + vec2(3.2, -1.6)], Line::new(1.4, col)));
}

/// The selection tool: a dashed rounded square.
fn select_glyph(p: &egui::Painter, c: Pos2, col: Color32) {
    let r = Rect::from_center_size(c, vec2(15.0, 15.0));
    let line = Line::new(1.6, col);
    // corners whole, sides broken in the middle
    for (a, b) in [(r.left_top(), r.right_top()), (r.right_top(), r.right_bottom()), (r.right_bottom(), r.left_bottom()), (r.left_bottom(), r.left_top())] {
        let d = (b - a) / (b - a).length();
        p.line_segment([a + d * 2.0, a + d * 4.6], line);
        p.line_segment([a + (b - a) / 2.0 - d * 1.8, a + (b - a) / 2.0 + d * 1.8], line);
        p.line_segment([b - d * 4.6, b - d * 2.0], line);
    }
    for k in [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()] {
        let dx = if k.x < c.x { 1.0 } else { -1.0 };
        let dy = if k.y < c.y { 1.0 } else { -1.0 };
        p.add(egui::Shape::line(vec![k + vec2(dx * 2.4, 0.0), k + vec2(dx * 0.6, dy * 0.6), k + vec2(0.0, dy * 2.4)], line));
    }
}

/// An ink's (or the eraser's) nib, as in the picker; `k` scales it (1 = the list's 16 pt).
fn tool_glyph(p: &egui::Painter, c: Pos2, t: Tool, k: f32, col: Color32) {
    // a 10 × 16 design box, centred on c
    let q = |x: f32, y: f32| c + vec2((x - 5.0) * k * 0.85, (y - 8.0) * k * 0.85);
    let line = Line::new(1.2 * k, col);
    let fill = |pts: &[(f32, f32)]| egui::Shape::convex_polygon(pts.iter().map(|(x, y)| q(*x, *y)).collect(), col, Line::NONE);
    let outline = |pts: &[(f32, f32)]| egui::Shape::closed_line(pts.iter().map(|(x, y)| q(*x, *y)).collect(), line);
    match t {
        Tool::Ink(InkKind::Pen) => {
            p.add(egui::Shape::line(vec![q(2.0, 16.0), q(5.0, 1.0), q(8.0, 16.0)], line));
            p.line_segment([q(3.8, 10.0), q(6.2, 10.0)], line);
        }
        Tool::Ink(InkKind::Monoline) => {
            p.add(outline(&[(5.0, 1.0), (7.5, 7.0), (2.5, 7.0)]));
            p.add(outline(&[(2.0, 7.0), (8.0, 7.0), (8.0, 12.5), (2.0, 12.5)]));
            p.line_segment([q(1.0, 12.5), q(1.0, 16.0)], line);
            p.line_segment([q(9.0, 12.5), q(9.0, 16.0)], line);
        }
        Tool::Ink(InkKind::Marker) => {
            p.add(fill(&[(3.0, 1.5), (7.5, 0.5), (8.0, 9.0), (2.0, 9.0)]));
            p.add(outline(&[(2.0, 9.0), (8.0, 9.0), (8.5, 16.0), (1.5, 16.0)]));
        }
        Tool::Ink(InkKind::Pencil) => {
            p.add(outline(&[(5.0, 1.0), (8.0, 9.0), (8.0, 16.0), (2.0, 16.0), (2.0, 9.0)]));
            p.add(fill(&[(5.0, 1.0), (6.3, 4.6), (3.7, 4.6)]));
            p.add(egui::Shape::line(vec![q(2.0, 9.0), q(3.5, 10.0), q(5.0, 9.0), q(6.5, 10.0), q(8.0, 9.0)], Line::new(1.0 * k, col)));
        }
        Tool::Ink(InkKind::Crayon) => {
            p.add(fill(&[(5.0, 1.0), (7.5, 6.0), (2.5, 6.0)]));
            p.add(fill(&[(2.0, 6.5), (8.0, 6.5), (8.5, 16.0), (1.5, 16.0)]));
        }
        Tool::Ink(InkKind::Fountain) => {
            p.add(outline(&[(5.0, 1.0), (8.5, 7.0), (7.5, 16.0), (2.5, 16.0), (1.5, 7.0)]));
            p.circle_stroke(q(5.0, 9.0), 1.2 * k, Line::new(1.0 * k, col));
            p.line_segment([q(5.0, 1.0), q(5.0, 7.8)], Line::new(1.0 * k, col));
        }
        Tool::Ink(InkKind::Reed) => {
            p.add(egui::Shape::line(vec![q(1.5, 16.0), q(1.5, 2.0), q(8.5, 2.0), q(8.5, 16.0)], line));
            p.add(fill(&[(1.5, 1.0), (8.5, 1.0), (8.5, 4.0), (1.5, 4.0)]));
            p.line_segment([q(5.0, 4.0), q(5.0, 12.0)], line);
            p.line_segment([q(3.4, 6.0), q(3.4, 16.0)], line);
            p.line_segment([q(6.6, 6.0), q(6.6, 16.0)], line);
        }
        Tool::Ink(_) => {
            // watercolour: a drop
            p.circle_filled(q(5.0, 11.5), 3.6 * k * 0.85, col);
            p.add(fill(&[(5.0, 1.5), (8.3, 10.5), (1.7, 10.5)]));
        }
        Tool::PixelEraser | Tool::ObjectEraser | Tool::Select | Tool::Lasso => {
            p.rect_filled(Rect::from_min_max(q(1.5, 2.0), q(8.5, 16.0)), CornerRadius::same((2.0 * k) as u8), col);
            let gap = if col.r() > 128 { Color32::BLACK } else { Color32::WHITE };
            for x in [3.5, 6.5] {
                p.line_segment([q(x, 10.5), q(x, 16.0)], Line::new(1.0 * k, gap));
            }
        }
    }
}

/// A size in the tool picker: Notes' squiggle (up, back down, a hump, a tail), as thick as the
/// size (the marker's a broad diagonal).
fn size_glyph(p: &egui::Painter, c: Pos2, kind: InkKind, i: usize, col: Color32) {
    let w = [1.3, 2.0, 2.8, 4.2, 6.0][i.min(4)];
    if kind == InkKind::Marker {
        let d = vec2(5.5, 5.5);
        p.line_segment([c - d, c + d], Line::new(1.5 + i as f32 * 1.8, col));
        return;
    }
    let ctrl = [(-5.5, 3.0), (-2.5, -6.0), (-4.5, 1.5), (-1.0, -2.0), (1.0, 0.0), (2.5, 3.0), (5.5, 5.0)];
    let pts: Vec<ink::InkPoint> = ctrl.iter().map(|(x, y)| <ink::InkPoint as ink::PointExt>::new(c.x + x, c.y + y, 0.0, w)).collect();
    let line: Vec<Pos2> = ink::centre_line(&pts, 0.6).into_iter().map(|(q, _)| q).collect();
    p.add(egui::Shape::line(line, Line::new(w, col)));
    // round ends
    p.circle_filled(pos2(c.x + ctrl[0].0, c.y + ctrl[0].1), w / 2.0, col);
    p.circle_filled(pos2(c.x + ctrl[6].0, c.y + ctrl[6].1), w / 2.0, col);
}

/// Shapes: a circle behind a rounded square.
fn shapes_glyph(p: &egui::Painter, c: Pos2, col: Color32) {
    let line = Line::new(1.6, col);
    p.circle_stroke(c + vec2(-3.0, -3.0), 6.0, line);
    let sq = Rect::from_center_size(c + vec2(3.0, 3.0), vec2(12.5, 12.5));
    p.rect_filled(sq, CornerRadius::same(3), if dark() { Color32::from_gray(40) } else { Color32::from_gray(253) });
    p.rect_stroke(sq, CornerRadius::same(3), line, egui::StrokeKind::Middle);
}

/// Text: an "A" in a box whose sides are broken.
fn text_glyph(p: &egui::Painter, c: Pos2, col: Color32) {
    let r = Rect::from_center_size(c, vec2(21.0, 15.0));
    let line = Line::new(1.5, col);
    p.line_segment([r.left_top() + vec2(2.0, 0.0), r.right_top() - vec2(2.0, 0.0)], line);
    p.line_segment([r.left_bottom() + vec2(2.0, 0.0), r.right_bottom() - vec2(2.0, 0.0)], line);
    for x in [r.min.x, r.max.x] {
        p.line_segment([pos2(x, r.min.y + 2.0), pos2(x, r.min.y + 4.5)], line);
        p.line_segment([pos2(x, r.center().y - 1.2), pos2(x, r.center().y + 1.2)], line);
        p.line_segment([pos2(x, r.max.y - 4.5), pos2(x, r.max.y - 2.0)], line);
    }
    p.text(c + vec2(0.0, 0.5), Align2::CENTER_CENTER, "A", FontId::new(10.5, layout::family("Sfera-Semibold")), col);
}

/// Signature: "×", a scribbled name, the line.
fn sign_glyph(p: &egui::Painter, c: Pos2, col: Color32) {
    let line = Line::new(1.3, col);
    let x = c + vec2(-10.0, 3.0);
    p.line_segment([x + vec2(-1.8, -1.8), x + vec2(1.8, 1.8)], Line::new(1.1, col));
    p.line_segment([x + vec2(-1.8, 1.8), x + vec2(1.8, -1.8)], Line::new(1.1, col));
    // a written name: loops leaning right, falling away
    let pts: Vec<Pos2> = (0..=60).map(|i| {
        let t = i as f32 / 60.0;
        let a = t * std::f32::consts::TAU * 2.5;
        c + vec2(-6.5 + t * 17.0 - a.sin() * 2.2, -a.cos() * 4.2 * (1.0 - t * 0.45) + t * 2.0)
    }).collect();
    p.add(egui::Shape::line(pts, line));
    p.line_segment([c + vec2(-12.5, 7.0), c + vec2(12.5, 7.0)], Line::new(1.2, col));
}

fn align_glyph(p: &egui::Painter, r: Rect, a: u8, col: Color32) {
    for (k, w) in [(0.0, 1.0), (0.33, 0.7), (0.66, 1.0), (1.0, 0.6)] {
        let w = if a == 3 { 1.0 } else { w };
        let len = r.width() * w;
        let x0 = match a {
            1 => r.center().x - len / 2.0,
            2 => r.max.x - len,
            _ => r.min.x,
        };
        let y = r.min.y + k * r.height();
        p.line_segment([pos2(x0, y), pos2(x0 + len, y)], Line::new(1.1, col));
    }
}

/// A shape's picture in the Shapes popover.
fn shape_glyph(p: &egui::Painter, r: Rect, k: ShapeKind, col: Color32) {
    let st = shapes::Style { line_width: 1.3, stroke: shapes::rgba(col), fill: [0.0; 4], ..Default::default() };
    let mut s = shapes::new_shape(k, r.center(), &st);
    s.frame = [r.min.x, r.min.y, r.width(), r.height()];
    s.fill = [0.0; 4];
    s.stroke = shapes::rgba(col);
    s.line_width = 1.3;
    match k {
        ShapeKind::Line | ShapeKind::Arrow => {
            s.points = vec![[r.min.x, r.max.y], [r.max.x, r.min.y]];
            shapes::fit_line(&mut s);
        }
        ShapeKind::Bubble => {
            s.frame = [r.min.x, r.min.y, r.width(), r.height() * 0.75];
            s.points = vec![[r.min.x + r.width() * 0.3, r.max.y]];
        }
        // the popover's polygon is a hexagon, point up
        ShapeKind::Polygon => s.sides = 6,
        _ => {}
    }
    shapes::paint(p, &s, &|q| q, 1.0, None);
}

fn signature_thumb(p: &egui::Painter, r: Rect, strokes: &[ink::Stroke], col: Color32) {
    if let Some(mut s) = shapes::new_signature(strokes, r.center(), r.width(), shapes::rgba(col)) {
        // as tall as fits
        if s.frame[3] > r.height() {
            let k = r.height() / s.frame[3];
            let (w, h) = (s.frame[2] * k, r.height());
            s.frame = [r.center().x - w / 2.0, r.min.y, w, h];
        }
        shapes::paint(p, &s, &|q| q, 1.0, None);
    }
}
