//! Editing commands shared by the toolbar, menus and inspector: inserting objects, the object
//! clipboard, copy/paste style, align/distribute, and applying format changes to the selection.

use kn_format::ObjId;
use kn_model::edit::History;
use kn_model::format::{FillSpec, StrokeSpec, TextProp};
use kn_model::insert::NewObject;
use kn_model::{Document, Drawable, DrawableKind, Fill, Geometry};

use crate::app::App;

/// Insert an object centred on the current slide and select it.
pub fn insert_object(app: &mut App, obj: NewObject) {
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    app.canvas.cancel();
    let Some(doc) = app.doc.as_mut() else { return };
    let label = match &obj {
        NewObject::TextBox => "Insert Text Box".to_string(),
        NewObject::Shape(s) => format!("Insert {}", s.name()),
        NewObject::Line { .. } => "Insert Line".to_string(),
        NewObject::Image(..) => "Insert Image".to_string(),
        NewObject::Chart { .. } => "Insert Chart".to_string(),
        NewObject::Table { .. } => "Insert Table".to_string(),
    };
    doc.begin(&mut app.hist, &label);
    let r = crate::textures::guarded("insert", || doc.insert(&mut app.hist, slide, &obj));
    doc.end(&mut app.hist);
    match r {
        Some(Ok(id)) => {
            // Set as Default Text Box Appearance
            if matches!(obj, NewObject::TextBox) {
                let me = app.doc.as_ref().and_then(|d| d.path.clone());
                if let (Some((owner, style)), Some(doc)) = (app.default_textbox.clone(), app.doc.as_mut()) {
                    if owner == me && me.is_some() && doc.file.obj(style).is_some() {
                        doc.begin(&mut app.hist, &label);
                        doc.apply_shape_style(&mut app.hist, id, style);
                        doc.end(&mut app.hist);
                    }
                }
            }
            app.sel = vec![id];
            app.bump(slide);
        }
        Some(Err(e)) => {
            undo_failed(app, &label);
            app.notify(format!("Couldn't insert: {e}"));
        }
        None => {
            undo_failed(app, &label);
            app.notify("Couldn't insert: internal error");
        }
    }
}

/// Format > Table > Add Row Above / Below, Add Column Before / After, Delete Row / Column, at
/// the table cell being edited (or edited last).
pub fn table_rows(app: &mut App, cmd: &str) {
    use kn_model::tablerows::Axis;
    let Some((id, r, c)) = crate::canvas::table_cursor(app) else { return };
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    crate::canvas::commit_cell(app);
    let label = cmd.trim_start_matches("Table > ").to_string();
    let Some(doc) = app.doc.as_mut() else { return };
    doc.begin(&mut app.hist, &label);
    let res = crate::textures::guarded("table rows", || match label.as_str() {
        "Add Row Above" => doc.table_insert(&mut app.hist, id, Axis::Row, r, r).map(|_| (r, c)),
        "Add Row Below" => doc.table_insert(&mut app.hist, id, Axis::Row, r + 1, r).map(|_| (r + 1, c)),
        "Add Column Before" => doc.table_insert(&mut app.hist, id, Axis::Column, c, c).map(|_| (r, c)),
        "Add Column After" => doc.table_insert(&mut app.hist, id, Axis::Column, c + 1, c).map(|_| (r, c + 1)),
        "Delete Row" => doc.table_delete(&mut app.hist, id, Axis::Row, r).map(|_| (r.saturating_sub(1), c)),
        _ => doc.table_delete(&mut app.hist, id, Axis::Column, c).map(|_| (r, c.saturating_sub(1))),
    });
    doc.end(&mut app.hist);
    match res {
        Some(Ok(cell)) => {
            // the cursor follows Keynote: onto the new row / column, or next to the deleted one
            app.canvas.table_cursor = Some((id, cell.0, cell.1));
            app.sel = vec![id];
            app.bump(slide);
        }
        Some(Err(e)) => {
            undo_failed(app, &label);
            app.notify(format!("Couldn't change the table: {e}"));
        }
        None => {
            undo_failed(app, &label);
            app.notify("Couldn't change the table: internal error");
        }
    }
}

/// Roll back the half-done action `label` after an error.
fn undo_failed(app: &mut App, label: &str) {
    if app.hist.undo_label() == Some(label) {
        app.undo();
    }
}

/// Put on the system clipboard when objects are copied, so Ctrl+V reaches the app (egui only
/// reports a paste when the clipboard has text) and we know to paste the copied objects.
pub const CLIP_MARKER: &str = "\u{2063}keydeck objects\u{2063}";

/// Text copied from another app pasted onto the slide: a new text box holding it (Keynote).
pub fn paste_text_as_box(app: &mut App, text: &str) {
    let text = text.trim_end_matches(['\n', '\r']).replace("\r\n", "\n");
    if text.is_empty() || app.current_slide().is_none() {
        return;
    }
    insert_object(app, NewObject::TextBox);
    let Some(&id) = app.sel.first() else { return };
    let slide = app.current_slide().map(|s| s.slide);
    if let Some(doc) = app.doc.as_mut() {
        let storage = doc.file.obj(id).and_then(|m| m.find_super("TSWP.ShapeInfoArchive")).and_then(|i| i.ref_id("owned_storage"));
        if let Some(st) = storage {
            doc.begin(&mut app.hist, "Paste");
            let len = doc.storage_text(st).len();
            doc.replace_text(&mut app.hist, st, 0, len, &text);
            doc.end(&mut app.hist);
        }
    }
    if let Some(s) = slide {
        app.bump(s);
    }
}

/// An image on the clipboard (a screenshot, a copied picture) pasted as a picture.
pub fn paste_image(app: &mut App, img: &egui::ColorImage) {
    let [w, h] = img.size;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for p in &img.pixels {
        rgba.extend_from_slice(&p.to_srgba_unmultiplied());
    }
    let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, rgba) else { return };
    let mut png = std::io::Cursor::new(Vec::new());
    if buf.write_to(&mut png, image::ImageFormat::Png).is_ok() {
        insert_object(app, NewObject::Image(png.into_inner(), "Pasted Image.png".into()));
    }
}

pub fn copy(app: &mut App) -> bool {
    let Some(doc) = app.doc.as_ref() else { return false };
    match doc.copy_objects(&app.sel) {
        Some(c) => {
            app.clip = Some(c);
            true
        }
        None => {
            app.notify("Nothing to copy (placeholders and tables can't be copied yet)");
            false
        }
    }
}

pub fn paste(app: &mut App, label: &str) {
    let Some(clip) = app.clip.clone() else { return };
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    let Some(doc) = app.doc.as_mut() else { return };
    // pasting onto the slide the objects are still on offsets them, like Keynote
    let here = doc
        .file
        .obj(slide)
        .map(|s| s.ref_ids("drawables_z_order"))
        .unwrap_or_default();
    let offset = if clip.objects.iter().any(|e| here.contains(&e.root)) && clip.source_id == kn_model::insert::doc_identity(doc) { 20.0 } else { 0.0 };
    doc.begin(&mut app.hist, label);
    let r = doc.paste_objects(&mut app.hist, slide, &clip, offset);
    doc.end(&mut app.hist);
    match r {
        Ok(ids) => {
            app.sel = ids;
            app.bump(slide);
        }
        Err(e) => {
            undo_failed(app, label);
            app.notify(format!("Couldn't paste: {e}"));
        }
    }
}

pub fn duplicate(app: &mut App) {
    let saved = app.clip.take();
    if copy(app) {
        if let Some(c) = app.clip.as_mut() {
            c.source_id = kn_model::insert::doc_identity(app.doc.as_ref().unwrap());
        }
        paste(app, "Duplicate");
    }
    app.clip = saved;
}

/// Run a format change on every selected object as one undo step. Consecutive changes with
/// the same label (slider drags) merge into one step.
pub fn apply(app: &mut App, label: &str, mut f: impl FnMut(&mut Document, &mut History, ObjId) -> bool) {
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    let ids = app.sel.clone();
    let Some(doc) = app.doc.as_mut() else { return };
    doc.begin_merge(&mut app.hist, label);
    let mut ok = false;
    for id in ids {
        ok |= crate::textures::guarded("format", || f(doc, &mut app.hist, id)).unwrap_or(false);
    }
    doc.end(&mut app.hist);
    if ok {
        app.bump(slide);
    }
}

pub fn set_text(app: &mut App, label: &str, prop: TextProp) {
    // while typing with a selection, character formatting applies to the selection only
    if let Some((storage, a, b)) = app.canvas.editing.as_ref().map(|e| (e.storage, e.cursor.min(e.anchor), e.cursor.max(e.anchor))) {
        // paragraph attributes (alignment, spacing...) go to the paragraphs being edited
        if !prop.is_char() {
            let Some(doc) = app.doc.as_mut() else { return };
            doc.end(&mut app.hist);
            doc.begin(&mut app.hist, label);
            let ok = doc.set_para_prop_range(&mut app.hist, storage, a, b, &prop);
            doc.end(&mut app.hist);
            doc.begin(&mut app.hist, "Typing");
            if ok {
                refresh_editing(app);
                return;
            }
        }
        if a < b && prop.is_char() {
            let Some(doc) = app.doc.as_mut() else { return };
            doc.end(&mut app.hist); // close the typing step: formatting is its own undo step
            doc.begin(&mut app.hist, label);
            let ok = doc.set_text_prop_range(&mut app.hist, storage, a, b, &prop);
            doc.end(&mut app.hist);
            doc.begin(&mut app.hist, "Typing");
            if ok {
                if let Some(ed) = app.canvas.editing.as_mut() {
                    ed.version += 1;
                    ed.scene = None;
                }
            }
            return;
        }
    }
    // a table: the selected cells, or (none selected) the whole table's text
    if let Some(table) = selected_table(app) {
        let range = crate::tableops::range(app).filter(|r| r.0 == table);
        let slide = app.current_slide().map(|s| s.slide);
        let Some(doc) = app.doc.as_mut() else { return };
        doc.begin(&mut app.hist, label);
        let res = match range {
            Some((_, r0, c0, r1, c1)) => doc.set_cells_text_prop(&mut app.hist, table, (r0, c0, r1, c1), &prop).map(|_| ()),
            None => {
                doc.set_table_text_prop(&mut app.hist, table, &prop);
                Ok(())
            }
        };
        doc.end(&mut app.hist);
        if let Err(e) = res {
            app.notify(e.to_string());
        }
        if let Some(s) = slide {
            app.bump(s);
        }
        return;
    }
    apply(app, label, |d, h, id| d.set_text_prop(h, id, &prop));
    // the box being typed in shows the change now, not when editing ends
    refresh_editing(app);
}

/// The text being edited is drawn from its own copy of the slide: rebuild it.
fn refresh_editing(app: &mut App) {
    if let Some(ed) = app.canvas.editing.as_mut() {
        ed.version += 1;
        ed.scene = None;
    }
    if let Some(s) = app.current_slide().map(|s| s.slide) {
        app.bump(s);
    }
}

/// The one selected table (or the table whose cell is being worked on).
fn selected_table(app: &mut App) -> Option<kn_format::ObjId> {
    if let Some((id, ..)) = crate::canvas::table_cursor(app) {
        return Some(id);
    }
    let [id] = app.sel[..] else { return None };
    app.doc.as_ref().is_some_and(|d| d.file.type_name(id) == "TST.TableInfoArchive").then_some(id)
}

/// Selected drawables of the current slide, in z-order.
pub fn selected(app: &mut App) -> Vec<Drawable> {
    let Some(sc) = app.scene(app.cur) else { return Vec::new() };
    sc.drawables.into_iter().filter(|d| app.sel.contains(&d.id)).collect()
}

/// First text run style of a drawable, if it has text.
pub fn first_run(d: &Drawable) -> Option<kn_model::CharStyle> {
    match &d.kind {
        DrawableKind::Shape(s) => s.text.as_ref()?.paragraphs.first()?.runs.first().map(|r| r.style.clone()),
        _ => None,
    }
}

/// Toggle a character attribute (Format ▸ Font ▸ Bold etc.) on the selection.
/// The character style of the selected text (or of the first selected text box).
pub fn current_run(app: &mut App) -> Option<kn_model::CharStyle> {
    // a table: the selected cell's text (else its first body cell's)
    if let Some(table) = selected_table(app) {
        let (r, c) = crate::tableops::range(app).filter(|x| x.0 == table).map(|x| (x.1, x.2)).unwrap_or((1, 1));
        return selected(app).iter().find_map(|d| match &d.kind {
            DrawableKind::Table(t) if d.id == table => {
                let (r, c) = (r.min(t.rows.saturating_sub(1)), c.min(t.cols.saturating_sub(1)));
                t.cell_styles.get((r * t.cols + c) as usize).map(|s| s.text.clone())
            }
            _ => None,
        });
    }
    selected(app).iter().find_map(first_run)
}

pub fn toggle_font(app: &mut App, name: &str) {
    let Some(st) = current_run(app) else { return };
    let prop = match name {
        "Bold" => TextProp::Bold(!st.bold),
        "Italic" => TextProp::Italic(!st.italic),
        "Underline" => TextProp::Underline(!st.underline),
        "Strikethrough" => TextProp::Strike(!st.strike),
        "Bigger" => TextProp::Size((st.size + 1.0).round()),
        "Smaller" => TextProp::Size((st.size - 1.0).max(1.0).round()),
        _ => return,
    };
    set_text(app, name, prop);
}

pub fn fill_spec(f: &Fill) -> FillSpec {
    match f {
        Fill::None => FillSpec::None,
        Fill::Color(c) => FillSpec::Color(*c),
        Fill::Gradient { stops, kind, inflections, advanced, .. } => {
            let angle = match kind {
                kn_model::GradientKind::Linear { angle } => *angle,
                _ => 270.0,
            };
            if *advanced || stops.len() > 2 || matches!(kind, kn_model::GradientKind::Radial) {
                let stops = stops.iter().enumerate().map(|(i, s)| (s.0, s.1, inflections.get(i).copied().unwrap_or(0.5))).collect();
                return FillSpec::Advanced { stops, radial: matches!(kind, kn_model::GradientKind::Radial), angle };
            }
            let from = stops.first().map(|s| s.1).unwrap_or(kn_model::Color::WHITE);
            let to = stops.last().map(|s| s.1).unwrap_or(kn_model::Color::BLACK);
            FillSpec::Gradient { from, to, angle }
        }
        Fill::Image { data_id, technique, reference, size, tint } => FillSpec::Image { data_id: *data_id, technique: *technique, size: *size, reference: *reference, tint: *tint },
    }
}

/// Graphic + text attributes picked up by Copy Style.
#[derive(Clone, Debug)]
pub struct CopiedStyle {
    fill: Option<FillSpec>,
    stroke: Option<Option<StrokeSpec>>,
    shadow: bool,
    reflection: bool,
    opacity: f32,
    text: Option<kn_model::CharStyle>,
}

pub fn copy_style(app: &mut App) {
    let Some(d) = selected(app).into_iter().next() else { return };
    let stroke_of = |s: &Option<kn_model::Stroke>| {
        Some(s.as_ref().map(|s| StrokeSpec {
            color: s.color,
            width: s.width,
            dash: if s.dash.is_empty() { kn_model::format::Dash::Solid } else if s.dash[0] < 0.5 { kn_model::format::Dash::Dotted } else { kn_model::format::Dash::Dashed },
        }))
    };
    let (fill, stroke) = match &d.kind {
        DrawableKind::Shape(s) => ((!s.is_line).then(|| fill_spec(&s.fill)), stroke_of(&s.stroke)),
        DrawableKind::Image(i) => (None, stroke_of(&i.stroke)),
        _ => (None, None),
    };
    app.copied_style = Some(CopiedStyle {
        fill,
        stroke,
        shadow: d.shadow.is_some(),
        reflection: d.reflection.is_some(),
        opacity: d.opacity,
        text: first_run(&d),
    });
}

pub fn paste_style(app: &mut App) {
    let Some(cs) = app.copied_style.clone() else { return };
    let targets = selected(app);
    apply(app, "Paste Style", |doc, h, id| {
        let Some(d) = targets.iter().find(|d| d.id == id) else { return false };
        let mut ok = false;
        let is_shape = matches!(&d.kind, DrawableKind::Shape(s) if !s.is_line);
        if let (Some(f), true) = (&cs.fill, is_shape) {
            ok |= doc.set_fill(h, id, f);
        }
        if let Some(s) = &cs.stroke {
            ok |= doc.set_stroke(h, id, s.as_ref());
        }
        ok |= doc.set_shadow(h, id, cs.shadow);
        ok |= doc.set_reflection(h, id, cs.reflection);
        ok |= doc.set_opacity(h, id, cs.opacity);
        if let (Some(t), true) = (&cs.text, first_run(d).is_some()) {
            for p in [
                TextProp::Font(t.font_name.clone()),
                TextProp::Size(t.size),
                TextProp::Bold(t.bold),
                TextProp::Italic(t.italic),
                TextProp::Underline(t.underline),
                TextProp::Strike(t.strike),
                TextProp::Color(t.color),
            ] {
                ok |= doc.set_text_prop(h, id, &p);
            }
        }
        ok
    });
}

/// Arrange ▸ Align Objects / Distribute Objects.
pub fn align(app: &mut App, how: &str) {
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    let ds: Vec<Drawable> = selected(app).into_iter().filter(|d| !d.locked).collect();
    let items: Vec<(ObjId, Geometry, (f32, f32, f32, f32))> = ds
        .iter()
        .map(|d| {
            let g = app.renderer.effective_geometry(d);
            let (lo, hi) = g.bounds();
            (d.id, g, (lo.x, lo.y, hi.x, hi.y))
        })
        .collect();
    if items.len() < 2 {
        return;
    }
    let minx = items.iter().map(|i| i.2.0).fold(f32::INFINITY, f32::min);
    let miny = items.iter().map(|i| i.2.1).fold(f32::INFINITY, f32::min);
    let maxx = items.iter().map(|i| i.2.2).fold(f32::NEG_INFINITY, f32::max);
    let maxy = items.iter().map(|i| i.2.3).fold(f32::NEG_INFINITY, f32::max);
    let mut moves: Vec<(ObjId, f32, f32)> = Vec::new();
    match how {
        "Left" | "Center" | "Right" | "Top" | "Middle" | "Bottom" => {
            for (id, _, (x0, y0, x1, y1)) in &items {
                let (dx, dy) = match how {
                    "Left" => (minx - x0, 0.0),
                    "Center" => ((minx + maxx) / 2.0 - (x0 + x1) / 2.0, 0.0),
                    "Right" => (maxx - x1, 0.0),
                    "Top" => (0.0, miny - y0),
                    "Middle" => (0.0, (miny + maxy) / 2.0 - (y0 + y1) / 2.0),
                    _ => (0.0, maxy - y1),
                };
                moves.push((*id, dx, dy));
            }
        }
        _ => {
            // distribute: equal gaps between objects, first and last stay put
            for axis in [0, 1] {
                if (how == "Horizontally" && axis == 1) || (how == "Vertically" && axis == 0) {
                    continue;
                }
                let mut v: Vec<&(ObjId, Geometry, (f32, f32, f32, f32))> = items.iter().collect();
                let lo = |i: &(ObjId, Geometry, (f32, f32, f32, f32))| if axis == 0 { i.2.0 } else { i.2.1 };
                let hi = |i: &(ObjId, Geometry, (f32, f32, f32, f32))| if axis == 0 { i.2.2 } else { i.2.3 };
                v.sort_by(|a, b| lo(a).total_cmp(&lo(b)));
                let total: f32 = v.iter().map(|i| hi(i) - lo(i)).sum();
                let span = hi(v.last().unwrap()).max(v.iter().map(|i| hi(i)).fold(f32::NEG_INFINITY, f32::max)) - lo(v[0]);
                let gap = (span - total) / (v.len() - 1) as f32;
                let mut pos = lo(v[0]);
                for i in v {
                    let d = pos - lo(i);
                    pos += hi(i) - lo(i) + gap;
                    match moves.iter_mut().find(|m| m.0 == i.0) {
                        Some(m) => {
                            if axis == 0 {
                                m.1 = d
                            } else {
                                m.2 = d
                            }
                        }
                        None => moves.push(if axis == 0 { (i.0, d, 0.0) } else { (i.0, 0.0, d) }),
                    }
                }
            }
        }
    }
    let Some(doc) = app.doc.as_mut() else { return };
    doc.begin(&mut app.hist, if how.ends_with("ly") { "Distribute Objects" } else { "Align Objects" });
    for (id, dx, dy) in moves {
        if dx.abs() > 0.001 || dy.abs() > 0.001 {
            doc.move_by(&mut app.hist, id, dx, dy);
        }
    }
    doc.end(&mut app.hist);
    app.bump(slide);
}

/// Font names for the font menu: the document's fonts first, then installed families.
pub fn font_names(app: &App) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(doc) = app.doc.as_ref() {
        for (_, a) in doc.file.archives() {
            let m = a.obj();
            if let Some(cp) = m.msg("char_properties") {
                if let Some(f) = cp.str("font_name") {
                    let fam = kn_render::fonts::parse_ps_name(f).0;
                    if !out.contains(&fam) {
                        out.push(fam);
                    }
                }
            }
        }
    }
    out.sort();
    for f in app.renderer.fonts.families() {
        if !out.contains(f) {
            out.push(f.clone());
        }
    }
    out
}

/// The toolbar's Shape popover: Text, lines and Keynote's 12 basic shapes, drawn as icons.
pub fn shape_popover(app: &mut App, ui: &mut egui::Ui) {
    use egui::{pos2, vec2, Sense, Stroke};
    let mut pick: Option<NewObject> = None;
    let cell = vec2(64.0, 56.0);
    let icon_fill = kn_ui::contenthub::ink();
    egui::Grid::new("shape-popover").spacing([6.0, 6.0]).show(ui, |ui| {
        let (r, resp) = ui.allocate_exact_size(cell, Sense::click());
        ui.painter().rect_filled(r, 6.0, if resp.hovered() { kn_ui::contenthub::tile_hover() } else { kn_ui::contenthub::tile_bg() });
        crate::theme::centered(ui.painter(), r.center(), "Text", egui::FontId::proportional(14.0), icon_fill);
        if resp.on_hover_text("Text Box").clicked() {
            pick = Some(NewObject::TextBox);
        }
        for arrow in [false, true] {
            let (r, resp) = ui.allocate_exact_size(cell, Sense::click());
            ui.painter().rect_filled(r, 6.0, if resp.hovered() { kn_ui::contenthub::tile_hover() } else { kn_ui::contenthub::tile_bg() });
            let (a, b) = (pos2(r.min.x + 14.0, r.max.y - 12.0), pos2(r.max.x - 14.0, r.min.y + 12.0));
            ui.painter().line_segment([a, b], Stroke::new(2.0, icon_fill));
            if arrow {
                ui.painter().arrow(a, b - a, Stroke::new(2.0, icon_fill));
            }
            if resp.on_hover_text(if arrow { "Line with Arrowhead" } else { "Line" }).clicked() {
                pick = Some(NewObject::Line { arrow });
            }
        }
        ui.end_row();
        for (i, s) in kn_model::insert::BasicShape::ALL.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(cell, Sense::click());
            ui.painter().rect_filled(r, 6.0, if resp.hovered() { kn_ui::contenthub::tile_hover() } else { kn_ui::contenthub::tile_bg() });
            let ps = s.path_source();
            let path = kn_model::path::from_path_source(&ps, 320.0, 220.0);
            let inner = r.shrink2(vec2(12.0, 10.0));
            let to = |x: f32, y: f32| pos2(inner.min.x + x / 320.0 * inner.width(), inner.min.y + y / 220.0 * inner.height());
            let mesh = shape_icon(&path, &to, icon_fill);
            ui.painter().add(mesh);
            if resp.on_hover_text(s.name()).clicked() {
                pick = Some(NewObject::Shape(*s));
            }
            if i % 3 == 2 {
                ui.end_row();
            }
        }
    });
    // as on the Mac: Draw with Pen, then the Content Hub (the shape library and My Shapes)
    ui.add_space(6.0);
    let w = ui.available_width();
    if ui.add_sized([w, 30.0], egui::Button::new("Draw with Pen").fill(kn_ui::contenthub::tile_bg()).corner_radius(15.0)).clicked() {
        ui.close();
        app.command(ui.ctx(), "Draw With Pen");
        return;
    }
    if ui.add_sized([w, 30.0], egui::Button::new("Browse Content Hub").fill(kn_ui::contenthub::tile_bg()).corner_radius(15.0)).clicked() {
        ui.close();
        app.hub.show_tab(kn_ui::contenthub::Tab::Shapes);
        return;
    }
    let pick_mine: Option<(kn_format::Msg, f32, f32)> = None;
    if let Some(o) = pick {
        ui.close();
        insert_object(app, o);
    }
    if let Some((ps, w, h)) = pick_mine {
        ui.close();
        insert_path_source(app, ps, w, h);
    }
}

/// Inserts a shape with its own outline (the shape library, My Shapes).
pub fn insert_path_source(app: &mut App, ps: kn_format::Msg, w: f32, h: f32) {
    {
        let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
        let mut res = None;
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Insert Shape");
            res = doc.insert_with_path_source(&mut app.hist, slide, ps, w, h).ok();
            doc.end(&mut app.hist);
        }
        if let Some(id) = res {
            app.sel = vec![id];
        }
        app.bump(slide);
    }
}

/// The Content Hub's Shapes: Text, the lines and the basic shapes, then My Shapes.
pub fn hub_shapes() -> (Vec<kn_ui::contenthub::AppShape>, Vec<kn_ui::contenthub::AppShape>) {
    use kn_ui::contenthub::{AppShape, Glyph};
    let mut basic = vec![
        AppShape { key: "text".into(), name: "Text Box".into(), glyph: Glyph::Text },
        AppShape { key: "line".into(), name: "Line".into(), glyph: Glyph::Line { arrow: false } },
        AppShape { key: "arrow".into(), name: "Line with Arrowhead".into(), glyph: Glyph::Line { arrow: true } },
    ];
    for (i, s) in kn_model::insert::BasicShape::ALL.iter().enumerate() {
        let path = kn_model::path::from_path_source(&s.path_source(), 100.0, 100.0);
        basic.push(AppShape { key: format!("basic:{i}"), name: s.name().to_string(), glyph: Glyph::Path { path, w: 100.0, h: 100.0 } });
    }
    let mine = my_shapes()
        .into_iter()
        .map(|(name, ps, w, h)| AppShape { key: format!("mine:{name}"), name, glyph: Glyph::Path { path: kn_model::path::from_path_source(&ps, w, h), w, h } })
        .collect();
    (basic, mine)
}

/// Inserts what the Content Hub returned.
pub fn insert_from_hub(app: &mut App, p: kn_ui::contenthub::Picked) {
    use kn_ui::contenthub::Picked;
    match p {
        Picked::Image(pic) => insert_object(app, NewObject::Image(pic.bytes, pic.name)),
        Picked::Library(s) => insert_path_source(app, s.path_source(), s.w, s.h),
        Picked::App(key) => match key.as_str() {
            "text" => insert_object(app, NewObject::TextBox),
            "line" => insert_object(app, NewObject::Line { arrow: false }),
            "arrow" => insert_object(app, NewObject::Line { arrow: true }),
            k => {
                if let Some(i) = k.strip_prefix("basic:").and_then(|i| i.parse::<usize>().ok())
                    && let Some(s) = kn_model::insert::BasicShape::ALL.get(i)
                {
                    insert_object(app, NewObject::Shape(*s));
                } else if let Some(name) = k.strip_prefix("mine:")
                    && let Some((_, ps, w, h)) = my_shapes().into_iter().find(|m| m.0 == name)
                {
                    insert_path_source(app, ps, w, h);
                }
            }
        },
        Picked::Remove(key) => {
            if let (Some(name), Some(dir)) = (key.strip_prefix("mine:"), my_shapes_dir()) {
                for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                    if e.path().file_stem().is_some_and(|s| s.to_string_lossy() == name) {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
                app.hub.refresh_shapes();
            }
        }
    }
}

/// Filled icon of a path (via a small tiny-skia raster, so concave shapes look right).
fn shape_icon(path: &kn_model::Path, to: &dyn Fn(f32, f32) -> egui::Pos2, fill: egui::Color32) -> egui::Shape {
    // outline polygon of the path, triangulated as a fan from the centroid (good enough for
    // the basic shapes, which are star-shaped around their centre)
    let pts: Vec<egui::Pos2> = polyline(path).into_iter().map(|(x, y)| to(x, y)).collect();
    if pts.len() < 3 {
        return egui::Shape::Noop;
    }
    let c = pts.iter().fold(egui::Vec2::ZERO, |a, p| a + p.to_vec2()) / pts.len() as f32;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(c.to_pos2(), fill);
    for p in &pts {
        mesh.colored_vertex(*p, fill);
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    egui::Shape::mesh(mesh)
}

/// First subpath of a path as points (curves sampled).
fn polyline(path: &kn_model::Path) -> Vec<(f32, f32)> {
    use kn_model::PathEl;
    let mut out: Vec<(f32, f32)> = Vec::new();
    let mut cur = (0.0, 0.0);
    for el in &path.els {
        match *el {
            PathEl::Move(p) => {
                if out.len() > 2 {
                    break;
                }
                out.clear();
                cur = (p.x, p.y);
                out.push(cur);
            }
            PathEl::Line(p) => {
                cur = (p.x, p.y);
                out.push(cur);
            }
            PathEl::Quad(c, p) => {
                for i in 1..=8 {
                    let t = i as f32 / 8.0;
                    let u = 1.0 - t;
                    out.push((u * u * cur.0 + 2.0 * u * t * c.x + t * t * p.x, u * u * cur.1 + 2.0 * u * t * c.y + t * t * p.y));
                }
                cur = (p.x, p.y);
            }
            PathEl::Cubic(c1, c2, p) => {
                for i in 1..=8 {
                    let t = i as f32 / 8.0;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    out.push((a * cur.0 + b * c1.x + c * c2.x + d * p.x, a * cur.1 + b * c1.y + c * c2.y + d * p.y));
                }
                cur = (p.x, p.y);
            }
            PathEl::Close => {}
        }
    }
    out
}

/// Format > Shapes and Lines > Save to My Shapes: kept in ~/.local/share/keydeck/shapes.
fn my_shapes_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))?;
    Some(base.join("keydeck").join("shapes"))
}

pub fn save_to_my_shapes(app: &mut App) {
    let Some(&id) = app.sel.first() else { return };
    let Some((ps, w, h)) = app.doc.as_ref().and_then(|d| d.path_source_of(id)) else {
        app.notify("Select a shape to save");
        return;
    };
    let Some(dir) = my_shapes_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let n = std::fs::read_dir(&dir).map(|r| r.count()).unwrap_or(0) + 1;
    let mut bytes = Vec::new();
    bytes.extend(w.to_le_bytes());
    bytes.extend(h.to_le_bytes());
    bytes.extend(ps.encode());
    match std::fs::write(dir.join(format!("Shape {n}.shape")), bytes) {
        Ok(()) => app.notify("Saved to My Shapes (in the Shape menu)"),
        Err(e) => app.notify(format!("Couldn't save the shape: {e}")),
    }
}

/// My Shapes: (name, path source, width, height).
pub fn my_shapes() -> Vec<(String, kn_format::Msg, f32, f32)> {
    let Some(dir) = my_shapes_dir() else { return Vec::new() };
    let ty = kn_format::schema::schema().type_idx("TSD.PathSourceArchive");
    let mut v: Vec<(String, kn_format::Msg, f32, f32)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let b = std::fs::read(e.path()).ok()?;
                    if b.len() < 9 {
                        return None;
                    }
                    let w = f32::from_le_bytes(b[0..4].try_into().ok()?);
                    let h = f32::from_le_bytes(b[4..8].try_into().ok()?);
                    let m = kn_format::Msg::parse(&b[8..], ty).ok()?;
                    Some((e.path().file_stem()?.to_string_lossy().to_string(), m, w, h))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// Format > Image > Auto Crop: the selected picture masked to its content, without the even
/// border (or transparent margin) around it.
pub fn auto_crop(app: &mut App) {
    let Some(sc) = app.scene(app.cur) else { return };
    let Some((id, data, gw, gh)) = sc.drawables.iter().filter(|d| app.sel.contains(&d.id)).find_map(|d| match &d.kind {
        DrawableKind::Image(i) => Some((d.id, i.data_id?, d.geom.w, d.geom.h)),
        _ => None,
    }) else {
        return;
    };
    let Some(img) = app.doc.as_ref().and_then(|doc| doc.data(data)).and_then(|b| image::load_from_memory(&b).ok()).map(|i| i.to_rgba8()) else {
        app.notify("Couldn't read the picture");
        return;
    };
    let (w, h) = img.dimensions();
    let corner = *img.get_pixel(0, 0);
    let differs = |p: &image::Rgba<u8>| {
        if corner[3] < 16 {
            p[3] >= 16
        } else {
            (0..3).any(|k| (i32::from(p[k]) - i32::from(corner[k])).abs() > 30) || p[3] < 200
        }
    };
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for (x, y, p) in img.enumerate_pixels() {
        if differs(p) {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
    }
    if x1 <= x0 || y1 <= y0 || (x0, y0, x1, y1) == (0, 0, w, h) {
        app.notify("There's no border to crop");
        return;
    }
    let (fx, fy) = (gw / w as f32, gh / h as f32);
    let slide = sc.slide;
    if let Some(doc) = app.doc.as_mut() {
        doc.begin(&mut app.hist, "Auto Crop");
        doc.set_mask_rect(&mut app.hist, id, x0 as f32 * fx, y0 as f32 * fy, (x1 - x0) as f32 * fx, (y1 - y0) as f32 * fy);
        doc.end(&mut app.hist);
    }
    app.bump(slide);
}
