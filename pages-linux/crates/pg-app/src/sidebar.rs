//! View > Page Thumbnails / Table of Contents: the sidebar left of the pages (Pages: 160 pt
//! list, thumbnails 148 pt tall with the page number under them, the current page framed; the
//! table of contents from the headings, click to go there).

use egui::{pos2, vec2, Color32, Rect, RichText, Sense, Stroke};
use kn_ui::theme;
use tiny_skia as sk;

use crate::app::{App, Cursor, Owner, Sidebar};

thread_local! {
    /// Thumbnails: page -> (content key, texture).
    static THUMBS: std::cell::RefCell<std::collections::HashMap<usize, (u64, egui::TextureHandle)>> = Default::default();
}

pub fn sidebar(app: &mut App, ui: &mut egui::Ui) {
    match app.sidebar {
        Sidebar::Toc => toc(app, ui),
        _ => thumbnails(app, ui),
    }
}

fn page_key(app: &App, pi: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Some(l) = &app.layout {
        if let Some(p) = l.pages.get(pi) {
            for line in &p.lines {
                (std::sync::Arc::as_ptr(&line.piece) as usize, line.line, line.dy.to_bits()).hash(&mut h);
            }
            for o in &p.objects {
                (o.id, o.x.to_bits(), o.y.to_bits()).hash(&mut h);
            }
            format!("{:?}", p.background).hash(&mut h);
        }
        (l.setup.width.to_bits(), l.setup.height.to_bits()).hash(&mut h);
    }
    app.version.hash(&mut h);
    h.finish()
}

fn thumbnails(app: &mut App, ui: &mut egui::Ui) {
    let Some(n) = app.layout.as_ref().map(|l| l.page_count()) else { return };
    let (pw, ph) = app.layout.as_ref().map(|l| (l.setup.width, l.setup.height)).unwrap_or((612.0, 792.0));
    let cur = app.current_page();
    let thumb_h = 130.0;
    let thumb_w = thumb_h * pw / ph;
    let mut go = None;
    let mut rendered = 0;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(10.0);
        for pi in 0..n {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), thumb_h + 30.0), Sense::click());
            let tr = Rect::from_center_size(pos2(r.center().x, r.min.y + 6.0 + thumb_h / 2.0), vec2(thumb_w, thumb_h));
            if pi == cur {
                ui.painter().rect_filled(tr.expand(6.0), 6.0, theme::ACCENT().gamma_multiply(0.35));
            } else if resp.hovered() {
                ui.painter().rect_filled(tr.expand(6.0), 6.0, theme::gray(0x40));
            }
            ui.painter().rect_filled(tr, 0.0, Color32::WHITE);
            if ui.is_rect_visible(tr) {
                let key = page_key(app, pi);
                let have = THUMBS.with(|t| t.borrow().get(&pi).map(|(k, tex)| (*k == key, tex.clone())));
                let tex = match have {
                    Some((true, tex)) => Some(tex),
                    stale => {
                        // a few a frame; a stale one stands in meanwhile
                        if rendered < 2 {
                            rendered += 1;
                            render_thumb(app, ui, pi, thumb_h).inspect(|t| {
                                THUMBS.with(|m| m.borrow_mut().insert(pi, (key, t.clone())));
                            })
                        } else {
                            ui.ctx().request_repaint();
                            stale.map(|s| s.1)
                        }
                    }
                };
                if let Some(t) = tex {
                    ui.painter().image(t.id(), tr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
            }
            ui.painter().rect_stroke(tr, 0.0, Stroke::new(0.5, theme::gray(0x60)), egui::StrokeKind::Outside);
            ui.painter().text(pos2(tr.min.x, tr.max.y + 6.0), egui::Align2::LEFT_TOP, (pi + 1).to_string(), egui::FontId::proportional(11.0), theme::TEXT_DIM());
            if resp.clicked() {
                go = Some(pi);
            }
            resp.context_menu(|ui| {
                let layout_doc = app.wb.as_ref().is_some_and(|w| !w.is_word_processing());
                if layout_doc {
                    if ui.button("Add Page After").clicked() {
                        let at = pi;
                        app.edit_pages("Add Page", |p, h| p.add_layout_page(h, at));
                        ui.close();
                    }
                    if ui.button("Delete Page").clicked() {
                        let at = pi;
                        app.edit_pages("Delete Page", |p, h| p.delete_layout_page(h, at));
                        ui.close();
                    }
                } else if ui.button("Go to Page").clicked() {
                    go = Some(pi);
                    ui.close();
                }
            });
        }
        ui.add_space(10.0);
    });
    if let Some(p) = go {
        crate::menus::go_to_page(app, p);
    }
    THUMBS.with(|t| t.borrow_mut().retain(|p, _| *p < n));
}

fn render_thumb(app: &mut App, ui: &egui::Ui, pi: usize, h: f32) -> Option<egui::TextureHandle> {
    let l = app.layout.as_ref()?;
    // (each page at its own size: a Word section may be landscape)
    let ps = l.page_setup(pi);
    let scale = h / ps.height.max(ps.width) * ui.ctx().pixels_per_point() * if ps.width > ps.height { 1.0 } else { ps.height / ps.height.max(1.0) };
    let (w, hh) = ((ps.width * scale).ceil() as u32, (ps.height * scale).ceil() as u32);
    let mut pm = sk::Pixmap::new(w.max(1), hh.max(1))?;
    let drawables: Vec<pg_core::kn_model::Drawable> = crate::canvas::page_layout_drawables(app, l, pi).into_iter().chain(app.drawables_on(pi).cloned()).collect();
    let pages = app.wb.as_ref()?;
    pg_core::render::draw_page_with(pages, l, pi, &mut app.canvas.tiles.renderer, &mut pm, sk::Transform::from_scale(scale, scale), Default::default(), &drawables);
    let img = egui::ColorImage::from_rgba_premultiplied([w as usize, hh as usize], pm.data());
    Some(ui.ctx().load_texture(format!("thumb-{pi}"), img, egui::TextureOptions::LINEAR))
}

fn toc(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(theme::bold("Table of Contents"));
    });
    ui.separator();
    let heads = app.layout.as_ref().map(|l| l.headings.clone()).unwrap_or_default();
    let mut go = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 50.0).show(ui, |ui| {
        if heads.is_empty() {
            ui.add_space(20.0);
            ui.label(RichText::new("Use paragraph styles in your document to populate the table of contents.").small().color(theme::TEXT_DIM()));
        }
        for (at, level, text, page) in &heads {
            ui.horizontal(|ui| {
                ui.add_space(8.0 + level.saturating_sub(1) as f32 * 12.0);
                let t = if text.trim().is_empty() { "(empty heading)".to_string() } else { text.trim().to_string() };
                if ui.add(egui::Label::new(RichText::new(t).size(if *level <= 1 { 13.0 } else { 12.0 })).sense(Sense::click()).truncate()).clicked() {
                    go = Some(*at);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0);
                    ui.label(RichText::new(page.to_string()).small().color(theme::TEXT_DIM()));
                });
            });
        }
    });
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
        ui.add_space(8.0);
        if ui.add_enabled(app.cursor.is_some_and(|c| c.owner == Owner::Body), egui::Button::new("Insert Table of Contents")).clicked() {
            crate::menus::run(app, ui.ctx(), "Table of Contents > Document");
        }
    });
    if let Some(shown) = go {
        let (st, stored) = match (app.wb.as_ref().and_then(|w| w.body_storage()), app.layout.as_ref().and_then(|l| l.body.as_ref())) {
            (Some(st), Some(b)) => (st, b.to_stored(shown)),
            _ => return,
        };
        crate::canvas::deselect_objects(app);
        app.cursor = Some(Cursor { storage: st, owner: Owner::Body, anchor: stored, caret: stored, goal_x: None });
        app.canvas.follow_caret = true;
    }
}
