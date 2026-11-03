//! A table cell being edited, drawn in the cell's own look (Pages, Numbers and Keynote edit in
//! place in the cell's font): the suite's three apps hand over the font file the page draws the
//! cell with (kn-render's Fonts::face_data) and get the text format for egui's field.
use egui::{Color32, FontFamily, Stroke, TextFormat};
use kn_model::{CharStyle, LineSpacing};

/// The egui family for a font face (key, bytes, face index, face is italic), loading the face
/// into egui the first time (it is ready from the next frame; the interface font until then).
/// Returns the family and whether the face itself is italic.
pub fn face_family(ctx: &egui::Context, face: Option<(String, Vec<u8>, u32, bool)>) -> (FontFamily, bool) {
    let Some((key, bytes, index, italic)) = face else { return (FontFamily::Proportional, false) };
    let fam = FontFamily::Name(key.clone().into());
    if ctx.fonts(|f| f.definitions().families.contains_key(&fam)) {
        return (fam, italic);
    }
    let mut defs = ctx.fonts(|f| f.definitions().clone());
    let mut data = egui::FontData::from_owned(bytes);
    data.index = index;
    defs.font_data.insert(key.clone(), std::sync::Arc::new(data));
    // characters the face lacks fall back to the interface font
    let mut list = vec![key];
    list.extend(defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    defs.families.insert(fam, list);
    ctx.set_fonts(defs);
    ctx.request_repaint();
    (FontFamily::Proportional, false)
}

/// The text format of a cell being edited: `cs`'s face (from [`face_family`]), size `font_px`
/// (already zoomed), colour, bold / italic, underline, strike, letter spacing, and the cell's
/// line spacing (points scaled by `zoom`).
pub fn cell_format(ui: &egui::Ui, family: (FontFamily, bool), cs: &CharStyle, spacing: &LineSpacing, font_px: f32, zoom: f32) -> TextFormat {
    let (family, face_italic) = family;
    let font = egui::FontId::new(font_px, family);
    let natural = ui.fonts_mut(|f| f.row_height(&font));
    let line_h = match *spacing {
        LineSpacing::Relative(x) if x > 0.0 => natural * x,
        LineSpacing::AtLeast(pt) => natural.max(pt * zoom),
        LineSpacing::Exactly(pt) if pt > 0.0 => pt * zoom,
        LineSpacing::AtMost(pt) if pt > 0.0 => natural.min(pt * zoom),
        LineSpacing::Between(pt) => natural + pt * zoom,
        _ => natural,
    };
    let k = cs.color;
    let color = if k.a <= 0.0 { Color32::BLACK } else { Color32::from_rgba_unmultiplied((k.r * 255.0) as u8, (k.g * 255.0) as u8, (k.b * 255.0) as u8, (k.a * 255.0) as u8) };
    let line = Stroke::new((font_px / 14.0).max(1.0), color);
    TextFormat {
        font_id: font,
        color,
        line_height: Some(line_h),
        extra_letter_spacing: cs.tracking * font_px,
        italics: cs.italic && !face_italic,
        underline: if cs.underline { line } else { Stroke::NONE },
        strikethrough: if cs.strike { line } else { Stroke::NONE },
        // (extra line spacing shared above and below, as the page sets it)
        valign: egui::Align::Center,
        ..Default::default()
    }
}

/// Where egui puts the first baseline of text in `format`, below the field's top: the field's
/// top is the page's baseline minus this, so the edited text sits where the page draws it.
pub fn baseline(ui: &egui::Ui, format: &TextFormat) -> f32 {
    let g = ui.fonts_mut(|f| f.layout_job(egui::text::LayoutJob::single_section("x".into(), format.clone())));
    g.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.pos.y).unwrap_or(format.font_id.size * 0.8)
}

/// The distance between lines the page laid a cell's text out with, from its lines' (baseline,
/// height): the gap between the first two baselines, else the one line's height.
pub fn pitch(lines: &[(f32, f32)]) -> Option<f32> {
    match lines {
        [a, b, ..] if b.0 > a.0 => Some(b.0 - a.0),
        [a, ..] if a.1 > 0.0 => Some(a.1),
        _ => None,
    }
}
