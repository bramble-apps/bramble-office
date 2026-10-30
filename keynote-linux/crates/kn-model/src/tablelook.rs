//! Format > Table (Keynote 15.4): gridlines, table outline, alternating row colour and the
//! table's font size. Like Keynote, the table points at its own variation of its table style
//! (and of its region text styles) carrying the change; the theme's styles stay as they are.

use kn_format::{Msg, ObjId};

use crate::edit::History;
use crate::format::{fill_msg, stroke_msg, FillSpec, StrokeSpec};
use crate::{Color, Document, Fill, Stroke};

/// A table style preset's look, for drawing its thumbnail.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresetLook {
    pub header_row: Option<Color>,
    pub header_col: Option<Color>,
    pub footer: Option<Color>,
    pub body: Option<Color>,
    pub band: Option<Color>,
    pub line: Option<Color>,
    pub dashed: bool,
}

/// The inspector's gridline switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gridline {
    /// Horizontal lines between body rows.
    BodyRows,
    /// Vertical lines between body columns.
    BodyColumns,
    /// The line under the header rows.
    HeaderRow,
    /// The line right of the header columns.
    HeaderColumn,
    /// The line above the footer rows.
    Footer,
}

impl Gridline {
    fn flag(self) -> &'static str {
        match self {
            Gridline::BodyRows => "h_strokes_visible",
            Gridline::BodyColumns => "v_strokes_visible",
            Gridline::HeaderRow => "hr_separator_visible",
            Gridline::HeaderColumn => "hc_separator_visible",
            Gridline::Footer => "footer_separator_visible",
        }
    }
}

/// The table style properties that draw the table's outline.
const OUTLINE: [&str; 5] = ["header_row_border_stroke", "footer_row_border_stroke", "header_column_border_stroke", "table_body_horizontal_border_stroke", "table_body_vertical_border_stroke"];
/// Every text style of a table: the regions, chart categories / labels and the table name.
const TEXT_STYLES: [&str; 15] = [
    "header_row_text_style",
    "header_column_text_style",
    "footer_row_text_style",
    "body_text_style",
    "category_level_1_text_style",
    "category_level_2_text_style",
    "category_level_3_text_style",
    "category_level_4_text_style",
    "category_level_5_text_style",
    "label_level_1_text_style",
    "label_level_2_text_style",
    "label_level_3_text_style",
    "label_level_4_text_style",
    "label_level_5_text_style",
    "table_name_style",
];

impl Document {
    /// The theme's table style presets (Format > Table "Table Styles"): (preset, its thumbnail
    /// object, its style network).
    pub fn table_style_presets(&self) -> Vec<(ObjId, Option<ObjId>, Option<ObjId>)> {
        let Some((_, theme)) = self.file.first_of("KN.ThemeArchive") else { return Vec::new() };
        let Some(t) = theme.find_super("TSS.ThemeArchive") else { return Vec::new() };
        let ty = kn_format::schema::schema().type_idx("TST.ThemePresetsArchive");
        for f in &t.fields {
            let parsed = match &f.val {
                kn_format::Val::Msg(m) if m.type_name() == "TST.ThemePresetsArchive" => Some(m.clone()),
                kn_format::Val::Bytes(b) if f.num == 200 => Msg::parse(b, ty).ok(),
                _ => None,
            };
            if let Some(m) = parsed {
                let ids = m.ref_ids("table_style_presets");
                if !ids.is_empty() && ids.iter().all(|i| self.file.obj(*i).is_some()) {
                    return ids.into_iter().map(|p| (p, self.file.obj(p).and_then(|m| m.ref_id("image")), self.file.obj(p).and_then(|m| m.ref_id("style_network")))).collect();
                }
            }
        }
        Vec::new()
    }

    /// What a table style preset looks like (for its thumbnail): the fills of its header row,
    /// header column, footer and body, the alternating row fill and the grid line colour.
    pub fn table_preset_look(&self, network: ObjId) -> PresetLook {
        let net = self.file.obj(network);
        let r = |f: &str| net.and_then(|n| n.ref_id(f));
        let fill_of = |style: Option<ObjId>| -> Option<Color> {
            crate::style::chain(&self.file, style).iter().find_map(|m| m.msg("cell_properties").and_then(|c| c.msg("cell_fill"))).and_then(|f| match crate::style::fill(f) {
                Fill::Color(c) => Some(c),
                Fill::Gradient { stops, .. } => stops.first().map(|s| s.1),
                _ => None,
            })
        };
        let tchain = crate::style::chain(&self.file, r("table_style"));
        let prop = |get: &dyn Fn(&Msg) -> Option<Msg>| tchain.iter().find_map(|m| m.msg("table_properties").and_then(get));
        let band = tchain.iter().find_map(|m| m.msg("table_properties").and_then(|p| p.bool("banded_rows"))).unwrap_or(false);
        let band_fill = prop(&|p| p.msg("banded_fill").cloned()).and_then(|f| match crate::style::fill(&f) {
            Fill::Color(c) => Some(c),
            _ => None,
        });
        let line = prop(&|p| p.msg("table_body_horizontal_stroke").cloned()).and_then(|s| crate::style::stroke(&s));
        PresetLook {
            header_row: fill_of(r("header_row_style")),
            header_col: fill_of(r("header_column_style")),
            footer: fill_of(r("footer_row_style")),
            body: fill_of(r("body_cell_style")),
            band: if band { band_fill } else { None },
            line: line.as_ref().map(|s| s.color),
            dashed: line.as_ref().is_some_and(|s| !s.dash.is_empty()),
        }
    }

    /// Format > Table > Table Styles: the table takes the preset's styles (every style
    /// reference repointed to the preset's network, as Keynote does; R37). Its own cell
    /// formatting stays.
    pub fn apply_table_style_preset(&mut self, h: &mut History, table: ObjId, preset: ObjId) -> bool {
        let Some(model) = self.table_model(table) else { return false };
        let Some(net) = self.file.obj(preset).and_then(|p| p.ref_id("style_network")).and_then(|n| self.file.obj(n)).cloned() else { return false };
        const FIELDS: [&str; 34] = [
            "body_text_style",
            "header_row_text_style",
            "header_column_text_style",
            "footer_row_text_style",
            "body_cell_style",
            "header_row_style",
            "header_column_style",
            "footer_row_style",
            "table_style",
            "table_name_style",
            "table_name_shape_style",
            "category_level_1_text_style",
            "category_level_2_text_style",
            "category_level_3_text_style",
            "category_level_4_text_style",
            "category_level_5_text_style",
            "category_level_1_style",
            "category_level_2_style",
            "category_level_3_style",
            "category_level_4_style",
            "category_level_5_style",
            "label_level_1_text_style",
            "label_level_2_text_style",
            "label_level_3_text_style",
            "label_level_4_text_style",
            "label_level_5_text_style",
            "label_level_1_style",
            "label_level_2_style",
            "label_level_3_style",
            "label_level_4_style",
            "label_level_5_style",
            "pivot_body_summary_row_style",
            "pivot_body_summary_column_style",
            "pivot_header_column_summary_style",
        ];
        self.touch(h, model);
        let Some(m) = self.file.obj_mut(model) else { return false };
        for f in FIELDS {
            if let Some(id) = net.ref_id(f) {
                if m.has_field(f) {
                    m.set_ref(f, id);
                }
            }
        }
        m.set_ref("table_style_preset", preset);
        self.file.refresh_refs(model);
        if let Some(file) = self.file.file_of(model).map(str::to_string) {
            self.file.ensure_external_refs(&file);
        }
        true
    }

    /// The table style preset the table was last given, if any.
    pub fn table_preset_of(&self, table: ObjId) -> Option<ObjId> {
        self.table_model(table).and_then(|m| self.file.obj(m)).and_then(|m| m.ref_id("table_style_preset"))
    }

    /// A style as one stand-alone style: its whole chain's properties merged (as Keynote copies
    /// a table's looks into a saved style), named `ident`, no parent.
    fn flat_style(&self, id: ObjId, ident: &str, sheet: Option<ObjId>) -> Option<Msg> {
        let chain = crate::style::chain(&self.file, Some(id));
        let mut out = (*chain.last()?).clone();
        for m in chain.iter().rev().skip(1) {
            for container in ["table_properties", "cell_properties", "char_properties", "para_properties", "shape_properties"] {
                if let Some(src) = m.msg(container) {
                    let dst = out.msg_mut_or_create(container);
                    for f in &src.fields {
                        dst.fields.retain(|g| g.num != f.num);
                    }
                    dst.fields.extend(src.fields.iter().cloned());
                    dst.fields.sort_by_key(|f| f.num);
                }
            }
        }
        let t = out.find_super_mut("TSS.StyleArchive")?;
        t.remove("parent");
        t.remove("is_variation");
        t.remove("name");
        t.set_str("style_identifier", ident);
        if let Some(sh) = sheet {
            t.set_ref("stylesheet", sh);
        }
        kn_format::objects::fix_override_count(&mut out);
        Some(out)
    }

    /// Format > Table > Table Styles "+": the table's look saved as a new style of the theme, and
    /// the table switched to it (Keynote, R41): copies of its table style and of the per-preset
    /// category / label / pivot cell styles and label text styles (named "table-N-…"), a style
    /// network sharing the body / header / footer styles, and a preset listed in the theme.
    pub fn save_table_style_preset(&mut self, h: &mut History, table: ObjId) -> Result<ObjId, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let model_id = self.table_model(table).ok_or(err("not a table"))?;
        let model = self.file.obj(model_id).ok_or(err("no table model"))?.clone();
        let (theme_id, _) = self.file.first_of("KN.ThemeArchive").ok_or(err("no theme"))?;
        let n = self.table_style_presets().len();
        let sheet = self.file.stylesheet_id().ok_or(err("no stylesheet"))?;
        // the copies: (model field, identifier)
        let mut copies: Vec<(String, String)> = vec![("table_style".into(), format!("table-{n}-tableStyle"))];
        for k in 1..=5 {
            copies.push((format!("category_level_{k}_style"), format!("tableCell-{n}-categoryLevel{k}Row")));
        }
        for k in 1..=5 {
            copies.push((format!("label_level_{k}_style"), format!("tableCell-{n}-labelLevel{k}Row")));
        }
        copies.push(("pivot_body_summary_row_style".into(), format!("tableCell-{n}-pivotBodySummaryRow")));
        copies.push(("pivot_body_summary_column_style".into(), format!("tableCell-{n}-pivotBodySummaryColumn")));
        copies.push(("pivot_header_column_summary_style".into(), format!("tableCell-{n}-pivotHeaderColumnSummary")));
        for k in 1..=5 {
            copies.push((format!("label_level_{k}_text_style"), format!("tableCellText-{n}-labelLevel{k}Row")));
        }
        let mut made: Vec<(String, String, Msg)> = Vec::new();
        for (field, ident) in &copies {
            if let Some(src) = model.ref_id(field) {
                if let Some(m) = self.flat_style(src, ident, Some(sheet)) {
                    made.push((field.clone(), ident.clone(), m));
                }
            }
        }
        if made.is_empty() {
            return Err(err("the table has no styles to save"));
        }
        let style_file = self.file.file_of(sheet).unwrap_or("Index/DocumentStylesheet.iwa").to_string();
        let doc_file = self.table_style_presets().first().and_then(|p| self.file.file_of(p.0)).unwrap_or("Index/Document.iwa").to_string();
        const NET_FIELDS: [&str; 34] = [
            "body_text_style", "header_row_text_style", "header_column_text_style", "footer_row_text_style", "body_cell_style", "header_row_style",
            "header_column_style", "footer_row_style", "table_style", "table_name_style", "table_name_shape_style", "category_level_1_text_style",
            "category_level_2_text_style", "category_level_3_text_style", "category_level_4_text_style", "category_level_5_text_style",
            "category_level_1_style", "category_level_2_style", "category_level_3_style", "category_level_4_style", "category_level_5_style",
            "label_level_1_text_style", "label_level_2_text_style", "label_level_3_text_style", "label_level_4_text_style", "label_level_5_text_style",
            "label_level_1_style", "label_level_2_style", "label_level_3_style", "label_level_4_style", "label_level_5_style",
            "pivot_body_summary_row_style", "pivot_body_summary_column_style", "pivot_header_column_summary_style",
        ];
        let preset = self.structural(h, &[sheet, theme_id, model_id], |k| -> Result<ObjId, kn_format::Error> {
            let ids = k.alloc_ids(made.len() as u64 + 2);
            let (net_id, preset_id) = (ids[made.len()], ids[made.len() + 1]);
            let mut new_of: std::collections::HashMap<String, ObjId> = std::collections::HashMap::new();
            for ((field, ident, m), id) in made.iter().zip(&ids) {
                let mut a = kn_format::iwa::Archive::new_object(*id, m.clone());
                a.refresh_object_references_except(&[sheet]);
                k.insert_archive(&style_file, a);
                new_of.insert(field.clone(), *id);
                // registered in the stylesheet: listed, and found by its name (kept sorted)
                let sm = k.obj_mut(sheet).ok_or(err("stylesheet"))?;
                sm.push_ref("styles", *id);
                let num = sm.field_def("identifier_to_style_map").ok_or(err("identifier map"))?.num;
                let template = sm.msgs("identifier_to_style_map").next().cloned();
                if let Some(mut e) = template {
                    e.set_str("identifier", ident);
                    e.set_ref("style", *id);
                    let at = sm
                        .fields
                        .iter()
                        .position(|f| f.num == num && matches!(&f.val, kn_format::Val::Msg(x) if x.str("identifier").is_some_and(|s| s > ident.as_str())))
                        .or_else(|| sm.fields.iter().rposition(|f| f.num == num).map(|i| i + 1))
                        .unwrap_or(sm.fields.len());
                    sm.fields.insert(at, kn_format::msg::Field { num, val: kn_format::Val::Msg(e) });
                }
            }
            k.refresh_refs(sheet);
            // the network: the copies, and the table's shared body / header / footer styles
            let mut net = Msg::new("TST.TableStyleNetworkArchive");
            for f in NET_FIELDS {
                if let Some(id) = new_of.get(f).copied().or_else(|| model.ref_id(f)) {
                    net.set_ref(f, id);
                }
            }
            net.set_u64("preset_id", n as u64);
            net.fields.sort_by_key(|f| f.num);
            k.insert_archive(&doc_file, kn_format::iwa::Archive::new_object(net_id, net));
            let mut p = Msg::new("TST.TableStylePresetArchive");
            p.set_ref("style_network", net_id);
            k.insert_archive(&doc_file, kn_format::iwa::Archive::new_object(preset_id, p));
            // listed in the theme's presets (TSS.ThemeArchive field 200)
            let theme = k.obj_mut(theme_id).ok_or(err("theme"))?;
            let t = theme.find_super_mut("TSS.ThemeArchive").ok_or(err("theme"))?;
            let ext = t.fields.iter_mut().find_map(|f| match &mut f.val {
                kn_format::Val::Msg(m) if m.type_name() == "TST.ThemePresetsArchive" => Some(m),
                _ => None,
            });
            ext.ok_or(err("no table style presets in the theme"))?.push_ref("table_style_presets", preset_id);
            k.refresh_refs(theme_id);
            Ok(preset_id)
        })?;
        if let Some(f) = self.file.file_of(preset).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        self.apply_table_style_preset(h, table, preset);
        Ok(preset)
    }

    fn table_model(&self, table: ObjId) -> Option<ObjId> {
        self.file.obj(table).and_then(|t| t.ref_id("tableModel"))
    }

    /// Point the table at its own variation of `field` (a style) and change that.
    fn edit_table_ref_style(&mut self, h: &mut History, table: ObjId, field: &str, f: impl FnOnce(&mut Msg)) -> bool {
        let Some(model) = self.table_model(table) else { return false };
        let Some(base) = self.file.obj(model).and_then(|m| m.ref_id(field)) else { return false };
        let Some(v) = self.variation_of(h, base) else { return false };
        if v != base {
            self.touch(h, model);
            if let Some(m) = self.file.obj_mut(model) {
                m.set_ref(field, v);
            }
            self.file.refresh_refs(model);
            if let Some(file) = self.file.file_of(model).map(str::to_string) {
                self.file.ensure_external_refs(&file);
            }
        }
        self.touch(h, v);
        if let Some(m) = self.file.obj_mut(v) {
            f(m);
            kn_format::objects::fix_override_count(m);
        }
        self.file.refresh_refs(v);
        true
    }

    /// Change the table's own table style properties.
    pub(crate) fn edit_table_style(&mut self, h: &mut History, table: ObjId, f: impl FnOnce(&mut Msg)) -> bool {
        self.edit_table_ref_style(h, table, "table_style", |m| f(m.msg_mut_or_create("table_properties")))
    }

    /// A table style property through the style's parents.
    fn table_prop<T>(&self, table: ObjId, get: impl Fn(&Msg) -> Option<T>) -> Option<T> {
        let model = self.table_model(table).and_then(|m| self.file.obj(m))?;
        let ch = crate::style::chain(&self.file, model.ref_id("table_style"));
        ch.iter().find_map(|m| m.msg("table_properties").and_then(&get))
    }

    pub fn table_gridline(&self, table: ObjId, g: Gridline) -> bool {
        self.table_prop(table, |p| p.bool(g.flag())).unwrap_or(false)
    }

    pub fn set_table_gridline(&mut self, h: &mut History, table: ObjId, g: Gridline, on: bool) -> bool {
        self.edit_table_style(h, table, |p| p.set_bool(g.flag(), on))
    }

    /// Format > Table > Table Outline: the stroke round the table (None: no outline).
    pub fn table_outline(&self, table: ObjId) -> Option<Stroke> {
        if !self.table_prop(table, |p| p.bool("table_border_visible")).unwrap_or(false) {
            return None;
        }
        self.table_prop(table, |p| p.msg("table_body_horizontal_border_stroke").map(crate::style::stroke)).flatten()
    }

    pub fn set_table_outline(&mut self, h: &mut History, table: ObjId, stroke: Option<&StrokeSpec>) -> bool {
        self.edit_table_style(h, table, |p| {
            p.set_bool("table_border_visible", stroke.is_some());
            if let Some(s) = stroke {
                for f in OUTLINE {
                    p.remove(f);
                    p.push_msg(f, stroke_msg(Some(s)));
                }
            }
        })
    }

    /// The alternating rows' fill.
    pub fn table_band_fill(&self, table: ObjId) -> Option<Fill> {
        self.table_prop(table, |p| p.msg("banded_fill").map(crate::style::fill))
    }

    pub fn set_table_band_color(&mut self, h: &mut History, table: ObjId, c: Color) -> bool {
        self.edit_table_style(h, table, |p| {
            p.remove("banded_fill");
            p.push_msg("banded_fill", fill_msg(&FillSpec::Color(c)));
        })
    }

    /// The body text size of the table.
    pub fn table_font_size(&self, table: ObjId) -> f32 {
        let size = |field: &str| {
            let model = self.table_model(table).and_then(|m| self.file.obj(m))?;
            let ch = crate::style::chain(&self.file, model.ref_id(field));
            ch.iter().find_map(|m| m.msg("char_properties").and_then(|c| c.f32("font_size")))
        };
        size("body_text_style").unwrap_or(12.0)
    }

    /// Format > Table > Table Font Size bigger / smaller (Keynote: 1 pt a click): every text
    /// style of the table, with ONE new variation per distinct style (header row and header
    /// column share theirs, body and footer theirs; R34), the model repointed at them.
    pub fn bump_table_font_size(&mut self, h: &mut History, table: ObjId, delta: f32) -> bool {
        self.edit_table_text_styles(h, table, &|cur, m| {
            let size = cur.unwrap_or(12.0);
            m.msg_mut_or_create("char_properties").set_f32("font_size", (size + delta).clamp(4.0, 400.0));
        })
    }

    /// Format > Font on a whole table (no cells selected): every text style of the table.
    pub fn set_table_text_prop(&mut self, h: &mut History, table: ObjId, prop: &crate::format::TextProp) -> bool {
        self.edit_table_text_styles(h, table, &|_, m| crate::format::apply_text_prop(m, prop))
    }

    /// Every text style of the table: ONE new variation per distinct style (Keynote, R34),
    /// changed by `edit` (given that style's current font size), the model repointed.
    fn edit_table_text_styles(&mut self, h: &mut History, table: ObjId, edit: &dyn Fn(Option<f32>, &mut Msg)) -> bool {
        let Some(model) = self.table_model(table) else { return false };
        let fields: Vec<(&str, ObjId)> = TEXT_STYLES.iter().filter_map(|f| Some((*f, self.file.obj(model)?.ref_id(f)?))).collect();
        let mut made: Vec<(ObjId, ObjId)> = Vec::new();
        for (_, base) in &fields {
            if made.iter().any(|m| m.0 == *base) {
                continue;
            }
            let cur = crate::style::chain(&self.file, Some(*base)).iter().find_map(|m| m.msg("char_properties").and_then(|c| c.f32("font_size")));
            let Some(v) = self.variation_of(h, *base) else { continue };
            self.touch(h, v);
            if let Some(m) = self.file.obj_mut(v) {
                m.msg_mut_or_create("char_properties");
                m.msg_mut_or_create("para_properties");
                edit(cur, m);
                kn_format::objects::fix_override_count(m);
            }
            self.file.refresh_refs(v);
            made.push((*base, v));
        }
        if made.is_empty() {
            return false;
        }
        self.touch(h, model);
        if let Some(m) = self.file.obj_mut(model) {
            for (f, base) in &fields {
                if let Some((_, v)) = made.iter().find(|x| x.0 == *base) {
                    m.set_ref(f, *v);
                }
            }
        }
        self.file.refresh_refs(model);
        if let Some(file) = self.file.file_of(model).map(str::to_string) {
            self.file.ensure_external_refs(&file);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DrawableKind;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn gridlines_outline_band_and_font_size() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R13-condhl-0-before.key");
        let mut d = Document::open(p).unwrap();
        let t = d.scene(&d.slides(), 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
        let mut h = History::default();
        let was = d.table_gridline(t, Gridline::BodyColumns);
        let size = d.table_font_size(t);
        let green = Color { r: 0.2, g: 0.8, b: 0.3, a: 1.0 };
        d.begin(&mut h, "Table");
        assert!(d.set_table_gridline(&mut h, t, Gridline::BodyColumns, !was));
        assert!(d.set_table_outline(&mut h, t, Some(&StrokeSpec { color: Color::BLACK, width: 3.0, dash: crate::format::Dash::Solid })));
        assert!(d.set_table_band_color(&mut h, t, green));
        assert!(d.bump_table_font_size(&mut h, t, 2.0));
        d.end(&mut h);
        let d = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        assert_eq!(d.table_gridline(t, Gridline::BodyColumns), !was);
        assert!((d.table_outline(t).unwrap().width - 3.0).abs() < 1e-3);
        assert!(matches!(d.table_band_fill(t), Some(Fill::Color(c)) if (c.g - 0.8).abs() < 1e-3));
        assert!((d.table_font_size(t) - (size + 2.0)).abs() < 1e-3);
        let DrawableKind::Table(tb) = d.scene(&d.slides(), 0).unwrap().drawables.into_iter().find(|x| x.id == t).unwrap().kind else { panic!() };
        // the outline is drawn, and body text is 2 pt bigger
        assert!(tb.h_strokes[0].as_ref().is_some_and(|s| (s.width - 3.0).abs() < 1e-3));
        assert!((tb.cell_styles[(tb.cols + 1) as usize].text.size - (size + 2.0)).abs() < 1e-3);
    }

    /// Keynote's own A+ (R34 KN-table-font-size-plus1click.key): one variation per distinct
    /// text style; header row / column share one, body / footer share one.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn font_size_variations_like_keynote() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/r33/R33-0-before.key");
        let mut d = Document::open(p).unwrap();
        let t = d.scene(&d.slides(), 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
        let refs = |d: &Document, t: ObjId| {
            let m = d.file.obj(d.table_model(t).unwrap()).unwrap();
            TEXT_STYLES.iter().map(|f| m.ref_id(f)).collect::<Vec<_>>()
        };
        let before = refs(&d, t);
        let mut h = History::default();
        d.begin(&mut h, "A+");
        assert!(d.bump_table_font_size(&mut h, t, 1.0));
        d.end(&mut h);
        let after = refs(&d, t);
        // fields that shared a style before share one variation after, and none kept its old one
        for i in 0..before.len() {
            for j in 0..before.len() {
                assert_eq!(before[i] == before[j], after[i] == after[j], "{} / {}", TEXT_STYLES[i], TEXT_STYLES[j]);
            }
            assert_ne!(before[i], after[i]);
        }
        let k = Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/r34/KN-table-font-size-plus1click.key")).unwrap();
        let kt = k.scene(&k.slides(), 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
        let theirs = refs(&k, kt);
        let distinct = |v: &Vec<Option<ObjId>>| v.iter().collect::<std::collections::HashSet<_>>().len();
        assert_eq!(distinct(&after), distinct(&theirs), "as many variations as Keynote makes");
        assert!((d.table_font_size(t) - k.table_font_size(kt)).abs() < 1e-3);
    }

    /// R37: applying preset 2 points the table at the preset's styles, as Keynote's file has.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn table_style_preset_like_keynote() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/r37");
        let theirs = Document::open(dir.join("R37-2-tablestyle.key")).unwrap();
        let mut d = Document::open(dir.join("R37-1-webvideo.key")).unwrap();
        let t = d.scene(&d.slides(), 1).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
        let presets = d.table_style_presets();
        assert_eq!(presets.len(), 6);
        let mut h = History::default();
        d.begin(&mut h, "Table Style");
        assert!(d.apply_table_style_preset(&mut h, t, presets[1].0));
        d.end(&mut h);
        let model = |d: &Document| d.file.obj(d.table_model(t).unwrap()).unwrap().clone();
        let (ours, keynote) = (model(&d), model(&theirs));
        for f in ["body_cell_style", "header_row_style", "header_column_style", "footer_row_style", "table_style", "header_row_text_style", "label_level_1_text_style", "category_level_3_style", "table_style_preset"] {
            assert_eq!(ours.ref_id(f), keynote.ref_id(f), "{f}");
        }
        let look = d.table_preset_look(presets[1].2.unwrap());
        assert!(look.header_row.is_some());
    }

    /// R41: "+" saves the table's look as a new style of the theme, as Keynote's file has it.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn save_table_style_like_keynote() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/r41");
        let theirs = Document::open(dir.join("R41-1-saved-style.key")).unwrap();
        let mut d = Document::open(dir.join("R41-0-start.key")).unwrap();
        let t = d.scene(&d.slides(), 1).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
        // R41's table had Alternating Row Color turned off first
        let mut h = History::default();
        d.begin(&mut h, "Style");
        d.set_table_banded(&mut h, t, false);
        let preset = d.save_table_style_preset(&mut h, t).unwrap();
        d.end(&mut h);
        assert_eq!(d.table_style_presets().len(), theirs.table_style_presets().len());
        let net = |d: &Document, p: ObjId| d.file.obj(d.file.obj(p).unwrap().ref_id("style_network").unwrap()).unwrap().clone();
        let (ours, keynote) = (net(&d, preset), net(&theirs, theirs.table_style_presets().last().unwrap().0));
        assert_eq!(ours.u64("preset_id"), keynote.u64("preset_id"));
        // shared styles are the same objects; the copies carry Keynote's names
        for f in ["body_cell_style", "header_row_style", "header_column_style", "footer_row_style", "body_text_style", "header_row_text_style", "table_name_style"] {
            assert_eq!(ours.ref_id(f), keynote.ref_id(f), "{f}");
        }
        let ident = |d: &Document, id: ObjId| d.file.obj(id).unwrap().find_super("TSS.StyleArchive").unwrap().str("style_identifier").unwrap().to_string();
        for f in ["table_style", "category_level_1_style", "label_level_3_style", "label_level_5_text_style", "pivot_header_column_summary_style"] {
            assert_eq!(ident(&d, ours.ref_id(f).unwrap()), ident(&theirs, keynote.ref_id(f).unwrap()), "{f}");
        }
        // the copied table style carries the table's look (banded off) as a full style
        let ts = d.file.obj(ours.ref_id("table_style").unwrap()).unwrap();
        assert_eq!(ts.msg("table_properties").and_then(|p| p.bool("banded_rows")), Some(false));
        // the table now uses it
        let model = d.file.obj(d.table_model(t).unwrap()).unwrap();
        assert_eq!(model.ref_id("table_style_preset"), Some(preset));
        // and it reopens
        let p = std::env::temp_dir().join(format!("kd-saved-style-{}.key", std::process::id()));
        d.save_as(&p).unwrap();
        let back = Document::open(&p).unwrap();
        assert_eq!(back.table_style_presets().len(), 7);
        let _ = std::fs::remove_file(p);
    }
}
