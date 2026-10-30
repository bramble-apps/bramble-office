//! Edits of the selected object for the object inspector (Keydeck's editing.rs, for a sheet's
//! one selected object instead of a slide's selection).

use nm_core::kn_format::ObjId;
use nm_core::kn_model::edit::History;
use nm_core::kn_model::format::{FillSpec, TextProp};
use nm_core::kn_model::{Document, Fill};

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

/// Character / paragraph formatting: the selected cells, else the selected object's text.
pub fn set_text(app: &mut App, label: &str, prop: TextProp) {
    if let Some(sel) = app.sel {
        let t = sel.table;
        let blocks = app.model_blocks(t, sel.block());
        crate::canvas::commit(app);
        if let Some(Err(e)) = app.edit(label, |d, h| blocks.iter().try_for_each(|b| d.set_cells_text_prop(h, t, *b, &prop).map(|_| ()))) {
            app.notify(format!("{label}: {e}"));
        }
        return;
    }
    // text being edited in place: the selected characters, else all of it (Keydeck's editing)
    if let Some((st, a, b)) = app.canvas.editing.as_ref().map(|e| (e.storage, e.cursor.min(e.anchor), e.cursor.max(e.anchor))) {
        let (a, b) = if a < b { (a, b) } else { (0, app.doc().map(|d| d.storage_text(st).len()).unwrap_or(0)) };
        if let Some(w) = app.wb.as_mut() {
            w.doc.set_text_prop_range(&mut app.hist, st, a, b, &prop);
        }
        if let Some(e) = app.canvas.editing.as_mut() {
            e.version += 1;
        }
        app.changed();
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
                nm_core::kn_model::GradientKind::Linear { angle } => *angle,
                _ => 270.0,
            };
            if *advanced || stops.len() > 2 || matches!(kind, nm_core::kn_model::GradientKind::Radial) {
                let stops = stops.iter().enumerate().map(|(i, s)| (s.0, s.1, inflections.get(i).copied().unwrap_or(0.5))).collect();
                return FillSpec::Advanced { stops, radial: matches!(kind, nm_core::kn_model::GradientKind::Radial), angle };
            }
            let from = stops.first().map(|s| s.1).unwrap_or(nm_core::kn_model::Color::WHITE);
            let to = stops.last().map(|s| s.1).unwrap_or(nm_core::kn_model::Color::BLACK);
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
    stroke: Option<Option<nm_core::kn_model::format::StrokeSpec>>,
    shadow: bool,
    reflection: bool,
    opacity: f32,
    text: Option<nm_core::kn_model::CharStyle>,
}

fn first_run(d: &nm_core::kn_model::Drawable) -> Option<nm_core::kn_model::CharStyle> {
    match &d.kind {
        nm_core::kn_model::DrawableKind::Shape(s) => s.text.as_ref()?.paragraphs.first()?.runs.first().map(|r| r.style.clone()),
        _ => None,
    }
}

pub fn copy_style(app: &mut App) {
    use nm_core::kn_model::format::{Dash, StrokeSpec};
    use nm_core::kn_model::DrawableKind;
    let Some(d) = app.canvas.obj.and_then(|id| app.drawables.iter().find(|d| d.id == id)).cloned() else { return };
    let stroke_of = |s: &Option<nm_core::kn_model::Stroke>| {
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
    use nm_core::kn_model::DrawableKind;
    let Some(cs) = app.copied_style.clone() else { return };
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
