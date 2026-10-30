//! The inspector: Format (what is selected: text, an object, table cells, or "Nothing
//! selected") or Document (Document / Section / Bookmarks), switched from the toolbar like
//! Pages 15.4 (research features/inspectors/*, screenshots P-insp-*).

use egui::RichText;
use kn_ui::theme;
use pg_core::kn_model::DrawableKind;

use crate::app::{App, InspMode, InspTab, Owner};
use crate::objinsp::section;

pub fn inspector(app: &mut App, ui: &mut egui::Ui) {
    if app.insp_mode == InspMode::Document {
        document(app, ui);
        return;
    }
    // table cells: Table / Cell / Text / Arrange (Values' table inspector)
    if let Some(sel) = app.sel {
        let order = [InspTab::Table, InspTab::Cell, InspTab::Text, InspTab::Arrange];
        let mut cur = order.iter().position(|t| *t == app.insp_tab).unwrap_or(0);
        crate::objinsp::tabs(ui, &["Table", "Cell", "Text", "Arrange"], &mut cur);
        app.insp_tab = order[cur];
        let Some(d) = app.table(sel.table).map(|(d, _)| d.clone()) else { return };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match app.insp_tab {
            InspTab::Table => crate::tblinsp::table_panel(app, ui, &d),
            InspTab::Cell => crate::tblinsp::cell_panel(app, ui, &d),
            InspTab::Text => crate::tblinsp::cell_panel(app, ui, &d),
            _ => {
                arrange_extra(app, ui, d.id);
                crate::objinsp::arrange(app, ui, std::slice::from_ref(&d));
            }
        });
        return;
    }
    // text being edited (body, header, footnote, or inside a text box)
    if app.cursor.is_some() && app.canvas.obj.is_none_or(|o| app.canvas.editing == Some(o)) {
        // a text box's text: its Style / Text / Arrange tabs, Text first
        if let Some(Owner::Drawable(id)) = app.cursor.map(|c| c.owner) {
            object(app, ui, id, true);
            return;
        }
        let mut cur = 0;
        crate::objinsp::tabs(ui, &["Text"], &mut cur);
        ui.add_space(6.0);
        crate::textinsp::text_panel(app, ui);
        return;
    }
    if let Some(id) = app.canvas.obj {
        object(app, ui, id, false);
        return;
    }
    crate::textinsp::nothing_selected(ui);
}

/// An object's tabs: Style / Text / Arrange (shapes and text boxes), Style / Image / Arrange
/// (pictures), Chart… with Pages' Arrange extras (Object Placement, Text Wrap).
fn object(app: &mut App, ui: &mut egui::Ui, id: pg_core::kn_format::ObjId, text_first: bool) {
    let Some(d) = app.drawables.iter().find(|d| d.id == id).cloned() else { return };
    let labels: Vec<&str> = match &d.kind {
        DrawableKind::Shape(s) if s.is_line => vec!["Style", "Arrange"],
        DrawableKind::Shape(_) => vec!["Style", "Text", "Arrange"],
        DrawableKind::Image(im) if im.gallery.len() > 1 => vec!["Gallery", "Arrange"],
        DrawableKind::Image(im) if im.web_video.is_some() => vec!["Web Video", "Arrange"],
        DrawableKind::Image(_) => vec!["Style", "Image", "Arrange"],
        DrawableKind::Movie(m) if m.audio_only => vec!["Audio", "Arrange"],
        DrawableKind::Movie(_) => vec!["Style", "Movie", "Arrange"],
        DrawableKind::Chart(_) => vec!["Chart", "Axis", "Series", "Arrange"],
        DrawableKind::Table(_) => vec!["Table", "Arrange"],
        _ => vec!["Arrange"],
    };
    if text_first && app.obj_tab == 0 && labels.contains(&"Text") {
        app.obj_tab = 1;
    }
    let mut tab = app.obj_tab.min(labels.len() - 1);
    crate::objinsp::tabs(ui, &labels, &mut tab);
    app.obj_tab = tab;
    match labels[tab] {
        "Text" => {
            ui.add_space(6.0);
            crate::textinsp::text_panel(app, ui);
        }
        "Arrange" => {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                arrange_extra(app, ui, id);
                let all: Vec<pg_core::kn_model::Drawable> = app.selected_objs().iter().filter_map(|i| app.drawables.iter().find(|x| x.id == *i).cloned()).collect();
                crate::objinsp::arrange(app, ui, if all.is_empty() { std::slice::from_ref(&d) } else { &all });
            });
        }
        "Table" => {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| crate::tblinsp::table_panel(app, ui, &d));
        }
        "Movie" | "Audio" => {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| crate::mediainsp::movie_panel(app, ui, &d));
        }
        "Web Video" => {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| crate::mediainsp::web_video_panel(ui, &d));
        }
        "Gallery" => {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| crate::mediainsp::gallery_panel(app, ui, &d));
        }
        _ => crate::objinsp::ui_tab(app, ui, &d, labels[tab]),
    }
}

/// Pages' Arrange extras: Object Placement (Stay on Page / Move with Text) and Text Wrap.
fn arrange_extra(app: &mut App, ui: &mut egui::Ui, id: pg_core::kn_format::ObjId) {
    let Some(w) = app.wb.as_ref() else { return };
    let wp = w.is_word_processing();
    let inline = w.inline_attachment(id).is_some();
    let (wrap, margin) = w.wrap_of(id);
    if wp {
        section(ui, "Object Placement");
        let mut cur = usize::from(inline);
        crate::objinsp::seg_small(ui, &["Stay on Page", "Move with Text"], &mut cur);
        if cur != usize::from(inline) {
            let page = app.obj_page.get(&id).copied().unwrap_or(0);
            let g = app.drawables.iter().find(|d| d.id == id).map(|d| d.geom).unwrap_or_default();
            let at = match app.cursor {
                Some(c) if c.owner == Owner::Body => c.caret,
                _ => app.layout.as_ref().and_then(|l| l.pages.get(page)).and_then(|p| app.layout.as_ref()?.body.as_ref().map(|b| b.to_stored(p.start))).unwrap_or(0),
            };
            let place = if cur == 1 { pg_core::objects::Placement::Inline { at } } else { pg_core::objects::Placement::Floating { page, x: g.x, y: g.y } };
            app.edit_pages("Object Placement", |p, h| p.set_placement(h, id, place));
        }
    }
    section(ui, "Text Wrap");
    let names: &[(&str, u64)] = if inline { &[("Inline with Text", 0), ("Around", 5), ("Above and Below", 3), ("None", 2)] } else { &[("Automatic", 4), ("Around", 5), ("Above and Below", 3), ("None", 2)] };
    let cur_name = names.iter().find(|n| n.1 == wrap).map(|n| n.0).unwrap_or(names[0].0);
    let mut pick = None;
    theme::combo("text-wrap").selected_text(cur_name).width(ui.available_width()).show_ui(ui, |ui| {
        for (n, v) in names {
            if ui.selectable_label(*v == wrap, *n).clicked() {
                pick = Some(*v);
            }
        }
    });
    if let Some(v) = pick {
        app.edit_pages("Text Wrap", |p, h| p.set_wrap(h, id, v));
    }
    if !inline && wrap != 2 {
        ui.horizontal(|ui| {
            ui.label("Spacing");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut m = margin as f64;
                if theme::stepper(ui, &mut m, 1.0, 0.0..=200.0, " pt", 0) {
                    app.edit_pages("Text Wrap", |p, h| p.set_wrap_margin(h, id, m as f32));
                }
            });
        });
    }
}

fn units(app: &App) -> (&'static str, f32) {
    match app.settings.ruler_units {
        1 => ("in", 72.0),
        2 => ("pt", 1.0),
        3 => ("mm", 72.0 / 25.4),
        _ => ("cm", 72.0 / 2.54),
    }
}

/// Document inspector: Document | Section | Bookmarks.
fn document(app: &mut App, ui: &mut egui::Ui) {
    let mut tab = app.doc_tab.min(2);
    crate::objinsp::tabs(ui, &["Document", "Section", "Bookmarks"], &mut tab);
    app.doc_tab = tab;
    let Some(w) = app.wb.as_ref() else { return };
    let setup = w.setup();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match tab {
        1 => section_tab(app, ui),
        2 => bookmarks_tab(app, ui),
        _ => document_tab(app, ui, setup),
    });
}

fn document_tab(app: &mut App, ui: &mut egui::Ui, setup: pg_core::PageSetup) {
    let (unit, per) = units(app);
    let mut s = setup.clone();
    let mut changed = false;
    section(ui, "Printer & Paper Size");
    theme::combo("printer").selected_text("Any Printer").width(ui.available_width()).show_ui(ui, |ui| {
        let _ = ui.selectable_label(true, "Any Printer");
    });
    let paper = pg_core::paper::find(&s.paper_id, s.width, s.height).map(|p| p.0).unwrap_or("Custom");
    theme::combo("paper").selected_text(paper).width(ui.available_width()).show_ui(ui, |ui| {
        for (name, id, w, h) in pg_core::paper::PAPERS {
            if ui.selectable_label(*name == paper, *name).clicked() {
                s.paper_id = id.to_string();
                (s.width, s.height) = if s.landscape { (*h, *w) } else { (*w, *h) };
                changed = true;
            }
        }
    });
    ui.label(RichText::new(pg_core::paper::dimension_text(s.width, s.height, unit)).color(theme::TEXT_DIM()));
    ui.horizontal(|ui| {
        ui.label("Page Orientation");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut cur = usize::from(s.landscape);
            crate::objinsp::seg_small(ui, &["Portrait", "Landscape"], &mut cur);
            if cur != usize::from(s.landscape) {
                s.landscape = cur == 1;
                (s.width, s.height) = (s.height, s.width);
                changed = true;
            }
        });
    });
    ui.separator();
    for (label, on, dist) in [("Header", &mut s.headers, &mut s.header_margin), ("Footer", &mut s.footers, &mut s.footer_margin)] {
        ui.horizontal(|ui| {
            if theme::checkbox(ui, on, label).changed() {
                changed = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut v = (*dist / per) as f64;
                if theme::stepper(ui, &mut v, if per > 2.0 { 0.1 } else { 1.0 }, 0.0..=100.0, &format!(" {unit}"), if per > 2.0 { 2 } else { 0 }) {
                    *dist = v as f32 * per;
                    changed = true;
                }
            });
        });
    }
    ui.separator();
    let mut body = s.body;
    if theme::checkbox(ui, &mut body, theme::bold("Document Body")).changed() {
        // unchecking converts to page layout (Pages asks first; the Edit menu can undo it)
        crate::menus::convert(app, !body);
        return;
    }
    ui.label(RichText::new(if s.body { "A word-processing document: the text flows from page to page." } else { "A page-layout document: objects on free pages." }).small().color(theme::TEXT_DIM()));
    section(ui, "Document Margins");
    ui.columns(2, |c| {
        for (i, (label, v)) in [("Top", &mut s.top), ("Bottom", &mut s.bottom), ("Left", &mut s.left), ("Right", &mut s.right)].into_iter().enumerate() {
            let ui = &mut c[i % 2];
            let mut x = (*v / per) as f64;
            if theme::stepper(ui, &mut x, if per > 2.0 { 0.1 } else { 1.0 }, 0.0..=1000.0, &format!(" {unit}"), if per > 2.0 { 2 } else { 0 }) {
                *v = x as f32 * per;
                changed = true;
            }
            ui.label(RichText::new(label).small().color(theme::TEXT_DIM()));
        }
    });
    ui.separator();
    if theme::checkbox(ui, &mut s.facing_pages, "Facing Pages").changed() {
        changed = true;
    }
    if theme::checkbox(ui, &mut s.hyphenation, "Hyphenate document").changed() {
        changed = true;
    }
    if theme::checkbox(ui, &mut s.ligatures, "Show ligatures").changed() {
        changed = true;
    }
    if changed {
        app.edit_pages("Document Setup", |p, h| p.set_setup(h, &s));
    }
}

fn section_tab(app: &mut App, ui: &mut egui::Ui) {
    let Some(w) = app.wb.as_ref() else { return };
    let secs = w.sections();
    if secs.is_empty() {
        ui.label("This document has no sections.");
        return;
    }
    // the section of the current page
    let page = app.current_page();
    let si = app.layout.as_ref().and_then(|l| l.pages.get(page)).map(|p| p.section).unwrap_or(0).min(secs.len() - 1);
    let s = secs[si].clone();
    if secs.len() > 1 {
        ui.label(RichText::new(format!("Section {} of {}", si + 1, secs.len())).color(theme::TEXT_DIM()));
    }
    // Background: any fill (colour, gradient, image, advanced image / gradient), as for shapes
    section(ui, "Background");
    let fill = s.background.clone().unwrap_or(pg_core::kn_model::Fill::None);
    let picked = crate::objinsp::fill_editor(ui, "section-background", &fill, app.wb.as_mut().map(|w| &mut w.doc));
    if let Some(f) = picked {
        let id = s.id;
        app.edit_pages("Background", |p, h| p.set_section_fill(h, id, &f));
    }
    section(ui, "Headers & Footers");
    let mut hide = s.first_page_hides_header_footer;
    if theme::checkbox(ui, &mut hide, "Hide on first page of section").changed() {
        let id = s.id;
        app.edit_pages("Headers & Footers", |p, h| p.set_section(h, id, |m| m.set_bool("section_template_first_page_hides_header_footer", hide)));
    }
    let mut matchp = s.inherit_previous;
    if theme::checkbox(ui, &mut matchp, "Match previous section").changed() {
        let id = s.id;
        app.edit_pages("Headers & Footers", |p, h| p.set_section(h, id, |m| m.set_bool("inherit_previous_header_footer", matchp)));
    }
    let mut lr = s.even_odd_different;
    if theme::checkbox(ui, &mut lr, "Left and right pages are different").changed() {
        let id = s.id;
        app.edit_pages("Headers & Footers", |p, h| p.set_section(h, id, |m| m.set_bool("section_template_even_odd_pages_different", lr)));
    }
    section(ui, "Page Numbering");
    theme::combo("numbering-format").selected_text("1, 2, 3, …").width(ui.available_width()).show_ui(ui, |ui| {
        let _ = ui.selectable_label(true, "1, 2, 3, …");
    });
    let mut cont = s.page_number_kind == 0;
    let mut start = s.page_number_start as f64;
    ui.horizontal(|ui| {
        if ui.radio(cont, "Continue from previous section").clicked() && !cont {
            cont = true;
            let id = s.id;
            app.edit_pages("Page Numbering", |p, h| p.set_section(h, id, |m| m.set_u64("section_page_number_kind", 0)));
        }
    });
    ui.horizontal(|ui| {
        if ui.radio(!cont, "Start at:").clicked() && cont {
            let id = s.id;
            app.edit_pages("Page Numbering", |p, h| p.set_section(h, id, |m| m.set_u64("section_page_number_kind", 1)));
        }
        if theme::stepper(ui, &mut start, 1.0, 1.0..=9999.0, "", 0) {
            let id = s.id;
            app.edit_pages("Page Numbering", |p, h| {
                p.set_section(h, id, |m| {
                    m.set_u64("section_page_number_kind", 1);
                    m.set_u64("section_page_number_start", start as u64);
                })
            });
        }
    });
    ui.add_space(10.0);
    if app.wb.as_ref().is_some_and(|w| w.is_word_processing()) && ui.button("Create a New Section Starting with This Page").clicked() {
        crate::menus::run(app, ui.ctx(), "Section Break");
    }
}

fn bookmarks_tab(app: &mut App, ui: &mut egui::Ui) {
    let Some(w) = app.wb.as_ref() else { return };
    let marks = w.bookmarks();
    if marks.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("No Bookmarks").size(15.0).color(theme::TEXT_DIM()));
            ui.label(RichText::new("Select text and click Add Bookmark to create a bookmark.").small().color(theme::TEXT_DIM()));
        });
    } else {
        for (id, text, a, b) in &marks {
            ui.horizontal(|ui| {
                if ui.link(if text.trim().is_empty() { "(bookmark)" } else { text.trim() }).clicked() {
                    if let Some(st) = app.wb.as_ref().and_then(|w| w.body_storage()) {
                        app.cursor = Some(crate::app::Cursor { storage: st, owner: Owner::Body, anchor: *a, caret: *b, goal_x: None });
                        app.canvas.follow_caret = true;
                        crate::canvas::deselect_objects(app);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Remove Bookmark").clicked() {
                        let fid = *id;
                        app.edit_pages("Remove Bookmark", |p, h| p.remove_bookmark(h, fid));
                    }
                });
            });
        }
    }
    ui.add_space(10.0);
    let can = app.cursor.is_some_and(|c| c.owner == Owner::Body);
    if ui.add_enabled(can, egui::Button::new("Add Bookmark").min_size(egui::vec2(ui.available_width(), 26.0))).clicked() {
        crate::menus::run(app, ui.ctx(), "Bookmark");
    }
}
