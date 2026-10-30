//! Edits of the selected object for the object inspector (Keydeck's editing.rs, for a page's
//! selected object), Copy / Paste Style, Group / Ungroup and the shape boolean operations.

use pg_core::kn_format::ObjId;
use pg_core::kn_model::edit::History;
use pg_core::kn_model::format::{FillSpec, TextProp};
use pg_core::kn_model::{Document, Fill};

use crate::app::App;

/// Run a format change on the selected object as one undo step; consecutive changes with the
/// same label (slider drags) merge into one step.
pub fn apply(app: &mut App, label: &str, mut f: impl FnMut(&mut Document, &mut History, ObjId) -> bool) {
    let Some(id) = app.canvas.obj else { return };
    let Some(w) = app.wb.as_mut() else { return };
    w.doc.begin_merge(&mut app.hist, label);
    let ok = f(&mut w.doc, &mut app.hist, id);
    w.doc.end(&mut app.hist);
    if ok {
        app.changed();
    }
}

/// Character / paragraph formatting: the selected cells, the caret's text, else the selected
/// object's text.
pub fn set_text(app: &mut App, label: &str, prop: TextProp) {
    if let Some(sel) = app.sel {
        let t = sel.table;
        let blocks = app.model_blocks(t, sel.block());
        crate::tables::commit(app);
        if let Some(Err(e)) = app.edit(label, |d, h| blocks.iter().try_for_each(|b| d.set_cells_text_prop(h, t, *b, &prop).map(|_| ()))) {
            app.notify(format!("{label}: {e}"));
        }
        return;
    }
    if app.cursor.is_some() {
        use crate::text::CharChange;
        let c = match &prop {
            TextProp::Font(f) => Some(CharChange::Font(f.clone())),
            TextProp::Size(v) => Some(CharChange::Size(*v)),
            TextProp::Bold(b) => Some(CharChange::Bold(*b)),
            TextProp::Italic(b) => Some(CharChange::Italic(*b)),
            TextProp::Underline(b) => Some(CharChange::Underline(*b)),
            TextProp::Strike(b) => Some(CharChange::Strike(*b)),
            TextProp::Color(c) => Some(CharChange::Color(*c)),
            TextProp::Script(v) => Some(CharChange::Script(*v)),
            _ => None,
        };
        match (c, prop) {
            (Some(c), _) => crate::textinsp::apply_char(app, label, c),
            (None, TextProp::Align(a)) => {
                let v = match a {
                    pg_core::kn_model::Align::Center => "TATvalue2",
                    pg_core::kn_model::Align::Right => "TATvalue1",
                    pg_core::kn_model::Align::Justify => "TATvalue3",
                    _ => "TATvalue0",
                };
                crate::textinsp::apply_para(app, label, move |pp| pp.set_enum("alignment", v));
            }
            (None, TextProp::SpaceBefore(v)) => crate::textinsp::apply_para(app, label, move |pp| pp.set_f32("space_before", v)),
            (None, TextProp::SpaceAfter(v)) => crate::textinsp::apply_para(app, label, move |pp| pp.set_f32("space_after", v)),
            _ => {}
        }
        return;
    }
    apply(app, label, |d, h, id| d.set_text_prop(h, id, &prop));
}

pub fn fill_spec(f: &Fill) -> FillSpec {
    match f {
        Fill::None => FillSpec::None,
        Fill::Color(c) => FillSpec::Color(*c),
        Fill::Gradient { stops, kind, inflections, advanced, .. } => {
            let angle = match kind {
                pg_core::kn_model::GradientKind::Linear { angle } => *angle,
                _ => 270.0,
            };
            if *advanced || stops.len() > 2 || matches!(kind, pg_core::kn_model::GradientKind::Radial) {
                let stops = stops.iter().enumerate().map(|(i, s)| (s.0, s.1, inflections.get(i).copied().unwrap_or(0.5))).collect();
                return FillSpec::Advanced { stops, radial: matches!(kind, pg_core::kn_model::GradientKind::Radial), angle };
            }
            let from = stops.first().map(|s| s.1).unwrap_or(pg_core::kn_model::Color::WHITE);
            let to = stops.last().map(|s| s.1).unwrap_or(pg_core::kn_model::Color::BLACK);
            FillSpec::Gradient { from, to, angle }
        }
        Fill::Image { data_id, technique, reference, size, tint } => FillSpec::Image { data_id: *data_id, technique: *technique, size: *size, reference: *reference, tint: *tint },
    }
}

/// Font names for the font menu: the document's fonts first, then installed families.
pub fn font_names(app: &App) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(w) = app.wb.as_ref() {
        for (_, a) in w.doc.file.archives() {
            if let Some(f) = a.obj().msg("char_properties").and_then(|cp| cp.str("font_name")) {
                let fam = kn_render::fonts::parse_ps_name(f).0;
                if !out.contains(&fam) {
                    out.push(fam);
                }
            }
        }
    }
    out.sort();
    for f in app.canvas.tiles.font_families() {
        if !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// Format > Copy Style (⌥⌘C): an object's fill, border, shadow, reflection, opacity and text
/// style (Keydeck's).
#[derive(Clone, Debug)]
pub struct CopiedStyle {
    fill: Option<FillSpec>,
    stroke: Option<Option<pg_core::kn_model::format::StrokeSpec>>,
    shadow: bool,
    reflection: bool,
    opacity: f32,
    text: Option<pg_core::kn_model::CharStyle>,
}

fn first_run(d: &pg_core::kn_model::Drawable) -> Option<pg_core::kn_model::CharStyle> {
    match &d.kind {
        pg_core::kn_model::DrawableKind::Shape(s) => s.text.as_ref()?.paragraphs.first()?.runs.first().map(|r| r.style.clone()),
        _ => None,
    }
}

pub fn copy_style(app: &mut App) {
    use pg_core::kn_model::format::{Dash, StrokeSpec};
    // text: its character look
    if app.cursor.is_some() {
        if let Some(cs) = crate::menus::current_char(app) {
            app.copied_style = Some(CopiedStyle { fill: None, stroke: None, shadow: false, reflection: false, opacity: 1.0, text: Some(cs) });
        }
        return;
    }
    use pg_core::kn_model::DrawableKind;
    let Some(d) = app.canvas.obj.and_then(|id| app.drawables.iter().find(|d| d.id == id)).cloned() else { return };
    let stroke_of = |s: &Option<pg_core::kn_model::Stroke>| {
        Some(s.as_ref().map(|s| StrokeSpec { color: s.color, width: s.width, dash: if s.dash.is_empty() { Dash::Solid } else if s.dash[0] < 0.5 { Dash::Dotted } else { Dash::Dashed } }))
    };
    let (fill, stroke) = match &d.kind {
        DrawableKind::Shape(s) => ((!s.is_line).then(|| fill_spec(&s.fill)), stroke_of(&s.stroke)),
        DrawableKind::Image(i) => (None, stroke_of(&i.stroke)),
        _ => (None, None),
    };
    app.copied_style = Some(CopiedStyle { fill, stroke, shadow: d.shadow.is_some(), reflection: d.reflection.is_some(), opacity: d.opacity, text: first_run(&d) });
}

pub fn paste_style(app: &mut App) {
    use pg_core::kn_model::DrawableKind;
    let Some(cs) = app.copied_style.clone() else { return };
    if let (Some(t), Some(c)) = (cs.text.clone(), app.cursor.filter(|c| !c.is_empty())) {
        let (a, b) = c.range();
        let st = c.storage;
        app.edit("Paste Style", |d, h| {
            pg_core::edit::set_char_props(d, h, st, a, b, &|cp| {
                cp.set_str("font_name", &t.font_name);
                cp.set_f32("font_size", t.size);
                cp.set_bool("bold", t.bold);
                cp.set_bool("italic", t.italic);
                cp.set_enum("underline", if t.underline { "kSingleUnderline" } else { "kNoUnderline" });
                cp.set_enum("strikethru", if t.strike { "kSingleStrikethru" } else { "kNoStrikethru" });
                cp.remove("font_color");
                cp.push_msg("font_color", pg_core::kn_model::format::color_msg(t.color));
            });
        });
        return;
    }
    let Some(d) = app.canvas.obj.and_then(|id| app.drawables.iter().find(|d| d.id == id)).cloned() else { return };
    apply(app, "Paste Style", |doc, h, id| {
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
        if let (Some(t), true) = (&cs.text, first_run(&d).is_some()) {
            for p in [TextProp::Font(t.font_name.clone()), TextProp::Size(t.size), TextProp::Bold(t.bold), TextProp::Italic(t.italic), TextProp::Underline(t.underline), TextProp::Strike(t.strike), TextProp::Color(t.color)] {
                ok |= doc.set_text_prop(h, id, &p);
            }
        }
        ok
    });
}

/// Arrange > Group (two or more floating objects on one page).
pub fn group(app: &mut App) {
    let ids = app.selected_objs();
    match app.edit_pages("Group", |p, h| p.group(h, &ids)) {
        Some(Ok(g)) => {
            app.refresh();
            crate::canvas::select_object(app, g);
        }
        Some(Err(e)) => app.notify(format!("Couldn't group: {e}")),
        None => {}
    }
}

/// Arrange > Ungroup.
pub fn ungroup(app: &mut App) {
    let Some(g) = app.canvas.obj else { return };
    match app.edit_pages("Ungroup", |p, h| p.ungroup(h, g)) {
        Some(Ok(kids)) => {
            app.canvas.obj = kids.first().copied();
            app.canvas.objs = kids;
        }
        Some(Err(e)) => app.notify(format!("Couldn't ungroup: {e}")),
        None => {}
    }
}

/// Format > Shapes and Lines > Unite / Intersect / Subtract / Exclude: a new shape with the
/// combined outline replaces the selected shapes (Values' way).
pub fn bool_op(app: &mut App, op: pg_core::kn_model::boolops::BoolOp) {
    let ids = app.selected_objs();
    let page = ids.first().and_then(|i| app.obj_page.get(i)).copied().unwrap_or(0);
    let b = match app.doc().map(|d| d.boolean_shape(&app.drawables, &ids, op)) {
        Some(Ok(b)) => b,
        Some(Err(e)) => return app.notify(e.to_string()),
        None => return,
    };
    let res = app.edit_pages("Combine Shapes", |p, h| -> Result<ObjId, pg_core::kn_format::Error> {
        let id = p.insert(h, pg_core::objects::NewObject::Shape(pg_core::kn_model::insert::BasicShape::Rectangle), pg_core::objects::Placement::Floating { page, x: b.x, y: b.y })?;
        p.doc.apply_boolean_shape(h, id, &b);
        p.delete_objects(h, &b.originals);
        Ok(id)
    });
    match res {
        Some(Ok(id)) => {
            app.refresh();
            crate::canvas::select_object(app, id);
        }
        Some(Err(e)) => app.notify(e.to_string()),
        None => {}
    }
}

/// Format > Shapes and Lines > Break Apart: each separate outline of the selected shape becomes
/// a shape of its own (same look, same place).
pub fn break_apart(app: &mut App) {
    let [id] = app.selected_objs()[..] else { return };
    let Some(outline) = app.drawables.iter().find(|d| d.id == id).and_then(|d| match &d.kind {
        pg_core::kn_model::DrawableKind::Shape(s) => Some(s.path.clone()),
        _ => None,
    }) else {
        return;
    };
    let parts = match app.doc().map(|d| d.break_apart_parts(id, &outline)) {
        Some(Ok(e)) => e,
        Some(Err(e)) => return app.notify(e.to_string()),
        None => return,
    };
    let page = app.obj_page.get(&id).copied().unwrap_or(0);
    let (Some(root), Some(clip)) = (app.root_id(), app.doc().and_then(|d| d.copy_objects(&[id]))) else { return };
    let res = app.edit_pages("Break Apart", |p, h| -> Result<Vec<ObjId>, pg_core::kn_format::Error> {
        let mut out = vec![id];
        for _ in 1..parts.subpaths.len() {
            let ids = p.doc.paste_objects(h, root, &clip, 0.0)?;
            for i in &ids {
                p.attach_floating(h, *i, page);
                p.doc.touch(h, *i);
                if let Some(d) = p.doc.file.obj_mut(*i).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
                    d.remove("parent");
                }
            }
            out.extend(ids);
        }
        for (k, sid) in out.iter().enumerate() {
            let one = pg_core::kn_model::points::Editable { subpaths: vec![parts.subpaths[k].clone()] };
            p.doc.set_editable_nodes(h, *sid, &one);
        }
        Ok(out)
    });
    match res {
        Some(Ok(ids)) => {
            app.refresh();
            app.canvas.obj = ids.first().copied();
            app.canvas.objs = ids;
        }
        Some(Err(e)) => app.notify(e.to_string()),
        None => {}
    }
}

/// My Shapes: Keydeck's folder (~/.local/share/keydeck/shapes), shared by the suite as iWork
/// shares them. Each file: width, height (f32 LE), then the TSD.PathSourceArchive.
fn my_shapes_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))?;
    Some(base.join("keydeck").join("shapes"))
}

/// Deletes a My Shape (the Content Hub's context menu).
pub fn remove_my_shape(name: &str) {
    let Some(dir) = my_shapes_dir() else { return };
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path().file_stem().is_some_and(|s| s.to_string_lossy() == name) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

pub fn my_shapes() -> Vec<(String, pg_core::kn_format::Msg, f32, f32)> {
    let Some(dir) = my_shapes_dir() else { return Vec::new() };
    let ty = pg_core::kn_format::schema::schema().type_idx("TSD.PathSourceArchive");
    let mut v: Vec<(String, pg_core::kn_format::Msg, f32, f32)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let b = std::fs::read(e.path()).ok()?;
                    if b.len() < 9 {
                        return None;
                    }
                    let w = f32::from_le_bytes(b[0..4].try_into().ok()?);
                    let h = f32::from_le_bytes(b[4..8].try_into().ok()?);
                    let m = pg_core::kn_format::Msg::parse(&b[8..], ty).ok()?;
                    Some((e.path().file_stem()?.to_string_lossy().to_string(), m, w, h))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// Format > Shapes and Lines > Save to My Shapes.
pub fn save_to_my_shapes(app: &mut App) {
    let [id] = app.selected_objs()[..] else { return };
    let Some((ps, w, h)) = app.doc().and_then(|d| d.path_source_of(id)) else {
        return app.notify("Select a shape to save");
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

/// A shape from a path source (My Shapes), floating on the current page.
pub fn insert_path_source(app: &mut App, ps: pg_core::kn_format::Msg, w: f32, h: f32) {
    let page = app.current_page();
    let res = app.edit_pages("Insert Shape", |p, hist| -> Result<ObjId, pg_core::kn_format::Error> {
        let id = p.insert(hist, pg_core::objects::NewObject::Shape(pg_core::kn_model::insert::BasicShape::Rectangle), pg_core::objects::Placement::Floating { page, x: 100.0, y: 150.0 })?;
        p.doc.touch(hist, id);
        if let Some(sh) = p.doc.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) {
            sh.remove("pathsource");
            sh.push_msg("pathsource", ps.clone());
            if let Some(d) = sh.find_super_mut("TSD.DrawableArchive") {
                let s = d.msg_mut_or_create("geometry").msg_mut_or_create("size");
                s.set_f32("width", w);
                s.set_f32("height", h);
            }
        }
        Ok(id)
    });
    match res {
        Some(Ok(id)) => {
            app.refresh();
            crate::canvas::select_object(app, id);
        }
        Some(Err(e)) => app.notify(e.to_string()),
        None => {}
    }
}
