//! The theme's presets (Table Styles grid): Pages keeps them in TP.ThemeArchive's TSS.ThemeArchive
//! extension 200 (TST.ThemePresetsArchive), as Numbers does in TN.ThemeArchive.

use kn_format::{Msg, ObjId};
use kn_model::Document;

/// (table style preset, its style network), in the theme's order.
pub fn table_style_presets(doc: &Document) -> Vec<(ObjId, ObjId)> {
    let Some((_, theme)) = doc.file.first_of("TP.ThemeArchive") else { return Vec::new() };
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
    // a document without the extension: the stylesheet's presets in file order
    doc.file.objects_of("TST.TableStylePresetArchive").filter_map(|(p, m)| Some((p, m.ref_id("style_network")?))).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn blank_has_presets() {
        let p = crate::Pages::new_blank(false);
        assert_eq!(super::table_style_presets(&p.doc).len(), 6);
    }
}
