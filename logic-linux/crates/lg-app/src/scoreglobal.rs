//! Global text (Logic: text dropped in the Page view's header space or on or outside the page
//! margins): one object for the whole project, shown on the pages of every score set at a
//! graphic page position (not a bar), on the pages its Pages setting picks (1, 2, Odd, Even,
//! All), in a margin Zone (Top, Header, Side, Footer, Bottom). The Pointer moves it up and
//! down only, with Shift (or the Layout tool) in both directions; a double-click edits it.

use crate::app::App;
use crate::look;
use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke};
use lg_core::score::{GlobalText, GLOBAL_PAGES, GLOBAL_ZONES};

fn edit_id() -> egui::Id {
    egui::Id::unique("lg-global-text-edit")
}

/// The zone a page point is in (the page's content rect inside the margins and header).
pub fn zone_of(page: Rect, content: Rect, header_bottom: f32, at: Pos2) -> u8 {
    if at.y < page.min.y + (content.min.y - page.min.y) * 0.5 && at.y < content.min.y {
        0
    } else if at.y < header_bottom.max(content.min.y) {
        1
    } else if at.y > content.max.y + (page.max.y - content.max.y) * 0.5 {
        4
    } else if at.y > content.max.y {
        3
    } else {
        2
    }
}

/// Text dropped (or the Text tool) at a Page view point: global text when it's in the header
/// space or on / outside the margins. True when it became one.
pub fn try_insert(app: &mut App, ctx: &egui::Context, at: Pos2, pages: &[Rect], content: &dyn Fn(usize) -> (Rect, f32)) -> bool {
    let Some(k) = pages.iter().position(|p| p.contains(at)) else { return false };
    let page = pages[k];
    let (inner, header_bottom) = content(k);
    let in_music = inner.contains(at) && at.y >= header_bottom;
    if in_music {
        return false;
    }
    let g = GlobalText { text: "Text".into(), x: (at.x - page.min.x) / page.width(), y: (at.y - page.min.y) / page.height(), pages: 4, zone: zone_of(page, inner, header_bottom, at), style: "Plain Text".into(), align: 0 };
    app.checkpoint("Insert Global Text");
    let idx = if let Some(p) = app.project.as_mut() {
        p.score.globals.push(g);
        p.score.globals.len() - 1
    } else {
        return false;
    };
    app.after_edit();
    ctx.data_mut(|m| m.insert_temp(edit_id(), Some((idx, true))));
    true
}

/// Draws the global text on the pages (and moves it).
pub fn draw(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, pages: &[Rect], printing: bool) {
    let Some(proj) = app.project.as_ref() else { return };
    let globals = proj.score.globals.clone();
    let styles = proj.score.styles();
    let mut moved: Option<(usize, f32, f32)> = None;
    let mut done = false;
    for (k, page) in pages.iter().enumerate() {
        // (the page's own scale: a point of the style is a point of the paper)
        let pt = page.width() / 595.0;
        for (gi, g) in globals.iter().enumerate().filter(|(_, g)| g.on_page(k)) {
            let st = styles.iter().find(|s| s.name == g.style).cloned().unwrap_or_else(|| styles[0].clone());
            let font = egui::FontId::new(st.size * pt, if st.bold { egui::FontFamily::Name("bold".into()) } else { egui::FontFamily::Proportional });
            let at = pos2(page.min.x + g.x * page.width(), page.min.y + g.y * page.height());
            let al = match g.align {
                1 => Align2::CENTER_CENTER,
                2 => Align2::RIGHT_CENTER,
                _ => Align2::LEFT_CENTER,
            };
            let fmt = egui::TextFormat { font_id: font, color: Color32::from_rgb(0x10, 0x10, 0x10), italics: st.italic, underline: if st.underline { Stroke::new(1.0, Color32::BLACK) } else { Stroke::NONE }, ..Default::default() };
            let gal = p.layout_job(egui::text::LayoutJob::single_section(g.text.clone(), fmt));
            let r = al.anchor_size(at, gal.size());
            p.galley(r.min, gal, Color32::BLACK);
            if printing {
                continue;
            }
            let resp = ui.interact(r.expand(2.0), ui.scope_id().with(("sc-global", k, gi)), Sense::click_and_drag());
            if resp.hovered() {
                p.rect_stroke(r.expand(2.0), 2.0, Stroke::new(1.0, look::accent()), egui::StrokeKind::Outside);
            }
            if resp.double_clicked() {
                ui.ctx().data_mut(|m| m.insert_temp(edit_id(), Some((gi, false))));
            }
            if resp.dragged() {
                let d = resp.drag_delta();
                // the Pointer: vertically only; Shift: both ways
                let both = ui.input(|i| i.modifiers.shift) || crate::score::state(ui.ctx()).tool == 4;
                moved = Some((gi, if both { d.x / page.width() } else { 0.0 }, d.y / page.height()));
            }
            if resp.drag_stopped() {
                done = true;
            }
        }
    }
    if let Some((gi, dx, dy)) = moved {
        // (one undo step per drag: the checkpoint when it starts)
        if !ui.ctx().data(|m| m.get_temp::<bool>(egui::Id::unique("lg-global-drag")).unwrap_or(false)) {
            app.checkpoint("Move Global Text");
            ui.ctx().data_mut(|m| m.insert_temp(egui::Id::unique("lg-global-drag"), true));
        }
        if let Some(g) = app.project.as_mut().and_then(|p| p.score.globals.get_mut(gi)) {
            g.x = (g.x + dx).clamp(0.0, 1.0);
            g.y = (g.y + dy).clamp(0.0, 1.0);
        }
    }
    if done {
        ui.ctx().data_mut(|m| m.insert_temp(egui::Id::unique("lg-global-drag"), false));
        app.after_edit();
    }
}

/// The global text's settings window (text, Pages, Zone, Align, Style, Delete).
pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some((gi, _new)) = ctx.data(|m| m.get_temp::<Option<(usize, bool)>>(edit_id())).flatten() else { return };
    let Some(mut g) = app.project.as_ref().and_then(|p| p.score.globals.get(gi).cloned()) else {
        ctx.data_mut(|m| m.insert_temp::<Option<(usize, bool)>>(edit_id(), None));
        return;
    };
    let styles: Vec<String> = app.project.as_ref().map(|p| p.score.styles().into_iter().map(|s| s.name).collect()).unwrap_or_default();
    let before = g.clone();
    let mut open = true;
    let mut delete = false;
    egui::Window::new("Global Text").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        egui::Grid::new("gt-grid").num_columns(2).show(ui, |ui| {
            ui.label("Text:");
            ui.text_edit_singleline(&mut g.text);
            ui.end_row();
            ui.label("Pages:");
            egui::ComboBox::from_id_salt("gt-pages").selected_text(GLOBAL_PAGES[g.pages as usize % 5]).show_ui(ui, |ui| {
                for (k, n) in GLOBAL_PAGES.iter().enumerate() {
                    ui.selectable_value(&mut g.pages, k as u8, *n);
                }
            });
            ui.end_row();
            ui.label("Zone:");
            egui::ComboBox::from_id_salt("gt-zone").selected_text(GLOBAL_ZONES[g.zone as usize % 5]).show_ui(ui, |ui| {
                for (k, n) in GLOBAL_ZONES.iter().enumerate() {
                    ui.selectable_value(&mut g.zone, k as u8, *n);
                }
            });
            ui.end_row();
            ui.label("Align:");
            egui::ComboBox::from_id_salt("gt-align").selected_text(["Left", "Center", "Right"][g.align.min(2) as usize]).show_ui(ui, |ui| {
                for (k, n) in ["Left", "Center", "Right"].iter().enumerate() {
                    ui.selectable_value(&mut g.align, k as u8, *n);
                }
                // (bar-related alignments are shown but can't be chosen for global text)
                for n in ["Left to Bar", "Center to Bar", "Right to Bar"] {
                    ui.add_enabled(false, egui::Button::selectable(false, n));
                }
            });
            ui.end_row();
            ui.label("Style:");
            egui::ComboBox::from_id_salt("gt-style").selected_text(g.style.clone()).show_ui(ui, |ui| {
                for n in &styles {
                    ui.selectable_value(&mut g.style, n.clone(), n);
                }
            });
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.button("Delete").clicked() {
                delete = true;
            }
        });
        let _ = vec2(0.0, 0.0);
    });
    if delete {
        app.checkpoint("Delete Global Text");
        if let Some(p) = app.project.as_mut() {
            if gi < p.score.globals.len() {
                p.score.globals.remove(gi);
            }
        }
        app.after_edit();
        open = false;
    } else if g != before {
        app.checkpoint("Global Text");
        if let Some(x) = app.project.as_mut().and_then(|p| p.score.globals.get_mut(gi)) {
            *x = g;
        }
        app.after_edit();
    }
    if !open {
        ctx.data_mut(|m| m.insert_temp::<Option<(usize, bool)>>(edit_id(), None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_by_place_on_the_page() {
        let page = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 850.0));
        let content = Rect::from_min_max(pos2(60.0, 80.0), pos2(540.0, 770.0));
        let hb = 160.0;
        assert_eq!(zone_of(page, content, hb, pos2(300.0, 20.0)), 0);
        assert_eq!(zone_of(page, content, hb, pos2(300.0, 120.0)), 1);
        assert_eq!(zone_of(page, content, hb, pos2(20.0, 400.0)), 2);
        assert_eq!(zone_of(page, content, hb, pos2(300.0, 790.0)), 3);
        assert_eq!(zone_of(page, content, hb, pos2(300.0, 840.0)), 4);
    }
}
