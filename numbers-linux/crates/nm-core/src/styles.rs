//! The theme's table style presets (Format > Table > Table Styles). Numbers keeps them in
//! TN.ThemeArchive (Keynote: KN.ThemeArchive); previews and applying use Keydeck's
//! table_preset_look / apply_table_style_preset.

use kn_format::{Msg, ObjId};
use kn_model::Document;

/// (preset, its style network), in the theme's order.
pub fn table_style_presets(doc: &Document) -> Vec<(ObjId, ObjId)> {
    let Some((_, theme)) = doc.file.first_of("TN.ThemeArchive") else { return Vec::new() };
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
            if !ids.is_empty() {
                return ids.into_iter().filter_map(|p| Some((p, doc.file.obj(p)?.ref_id("style_network")?))).collect();
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn blank_has_presets() {
        let wb = crate::Workbook::open(format!("{}/../../testdata/00-empty.numbers", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let p = super::table_style_presets(&wb.doc);
        assert!(p.len() >= 6, "{} presets", p.len());
        let look = wb.doc.table_preset_look(p[1].1);
        assert!(look.header_row.is_some());
    }
}
