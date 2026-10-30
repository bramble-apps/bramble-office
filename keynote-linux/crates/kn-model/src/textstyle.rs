//! Character formatting of part of a text (Keynote's "select a word, make it bold").
//! Like Keynote, a formatted range gets a character style variation (of the run's current
//! character style, or of the stylesheet's "None" character style) in the char-style table.

use kn_format::{Msg, ObjId};

use crate::edit::History;
use crate::format::{apply_text_prop, TextProp};
use crate::textedit::byte_to_u16;
use crate::Document;

impl Document {
    /// Apply a character property to bytes `start..end` of a storage's text.
    /// Paragraph properties (alignment, spacing, bullets) are not handled here.
    /// A paragraph attribute (alignment, spacing, ...) for the paragraphs that bytes
    /// start..end touch (the caret's paragraph when start == end), as Keynote applies it while
    /// text is being edited: each paragraph points at a variation of its own style.
    pub fn set_para_prop_range(&mut self, h: &mut History, storage: ObjId, start: usize, end: usize, prop: &TextProp) -> bool {
        if prop.is_char() {
            return false;
        }
        let text = self.storage_text(storage);
        let units: Vec<u16> = text.encode_utf16().collect();
        let (s16, e16) = (byte_to_u16(&text, start.min(text.len())), byte_to_u16(&text, end.min(text.len())));
        // paragraph starts (after each paragraph break)
        let mut starts = vec![0usize];
        for (i, u) in units.iter().enumerate() {
            if (*u == 0x0a || *u == 0x2029) && i + 1 <= units.len() {
                starts.push(i + 1);
            }
        }
        let next_start = |p: usize| starts.iter().copied().find(|q| *q > p).unwrap_or(units.len() + 1);
        let touched: Vec<usize> = starts.iter().copied().filter(|p| *p <= e16.max(s16) && next_start(*p) > s16).collect();
        if touched.is_empty() {
            return false;
        }
        let Some(st) = self.file.obj(storage) else { return false };
        let mut table: Vec<(usize, Option<ObjId>)> = st
            .msg("table_para_style")
            .map(|t| t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0) as usize, e.ref_id("object"))).collect())
            .unwrap_or_default();
        table.sort_by_key(|e| e.0);
        let at = |table: &[(usize, Option<ObjId>)], i: usize| table.iter().take_while(|e| e.0 <= i).last().and_then(|e| e.1);
        // every touched paragraph and the one after them start an entry of their own
        let mut cuts = touched.clone();
        if let Some(last) = touched.last() {
            let n = next_start(*last);
            if n <= units.len() {
                cuts.push(n);
            }
        }
        for c in cuts {
            if !table.iter().any(|e| e.0 == c) {
                let style = at(&table, c);
                table.push((c, style));
                table.sort_by_key(|e| e.0);
            }
        }
        // one new variation per distinct current style
        let sheet = self.file.stylesheet_id();
        let touch: Vec<ObjId> = sheet.into_iter().collect();
        let mut made: Vec<(ObjId, ObjId)> = Vec::new();
        for p in &touched {
            let Some(i) = table.iter().position(|e| e.0 == *p) else { continue };
            let Some(base) = table[i].1 else { continue };
            let v = match made.iter().find(|m| m.0 == base) {
                Some(m) => m.1,
                None => {
                    let Ok(v) = self.structural(h, &touch, |k| k.new_style_variation(base)) else { continue };
                    self.touch(h, v);
                    if let Some(m) = self.file.obj_mut(v) {
                        crate::format::apply_text_prop(m, prop);
                        kn_format::objects::fix_override_count(m);
                    }
                    made.push((base, v));
                    v
                }
            };
            table[i].1 = Some(v);
        }
        self.touch(h, storage);
        let Some(st) = self.file.obj_mut(storage) else { return false };
        let t = st.msg_mut_or_create("table_para_style");
        t.remove("entries");
        for (c, o) in table {
            let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
            e.set_u64("character_index", c as u64);
            if let Some(o) = o {
                e.set_ref("object", o);
            }
            t.push_msg("entries", e);
        }
        self.file.refresh_refs(storage);
        if let Some(f) = self.file.file_of(storage).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        true
    }

    pub fn set_text_prop_range(&mut self, h: &mut History, storage: ObjId, start: usize, end: usize, prop: &TextProp) -> bool {
        if !prop.is_char() || start >= end {
            return false;
        }
        let text = self.storage_text(storage);
        let (s16, e16) = (byte_to_u16(&text, start), byte_to_u16(&text, end.min(text.len())));
        if s16 >= e16 {
            return false;
        }
        let Some(st) = self.file.obj(storage) else { return false };
        let mut table: Vec<(usize, Option<ObjId>)> = st
            .msg("table_char_style")
            .map(|t| t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0) as usize, e.ref_id("object"))).collect())
            .unwrap_or_default();
        table.sort_by_key(|e| e.0);
        let at = |i: usize| table.iter().take_while(|e| e.0 <= i).last().and_then(|e| e.1);
        // run starts inside the range, each with the style in force there
        let mut cuts: Vec<usize> = vec![s16];
        cuts.extend(table.iter().map(|e| e.0).filter(|c| *c > s16 && *c < e16));
        let segments: Vec<(usize, Option<ObjId>)> = cuts.iter().map(|c| (*c, at(*c))).collect();
        let after = at(e16);

        // one new variation per distinct current style
        let null = self.style_by_identifier("TSWP.CharacterStyleArchive", "character-style-null");
        let mut map: Vec<(Option<ObjId>, ObjId)> = Vec::new();
        for (_, cur) in &segments {
            if map.iter().any(|(k, _)| k == cur) {
                continue;
            }
            let Some(base) = cur.or(null) else { return false };
            let Some(v) = self.variation_of(h, base) else { return false };
            self.touch(h, v);
            if let Some(a) = self.file.archive_mut(v) {
                apply_text_prop(a.obj_mut(), prop);
                kn_format::objects::fix_override_count(a.obj_mut());
            }
            map.push((*cur, v));
        }

        // rebuild the table: entries outside the range kept, the range re-pointed, and the
        // style in force after the range restored at its end
        let mut new: Vec<(usize, Option<ObjId>)> = table.iter().copied().filter(|e| e.0 < s16 || e.0 > e16).collect();
        for (c, cur) in &segments {
            let v = map.iter().find(|(k, _)| k == cur).map(|(_, v)| *v);
            new.push((*c, v));
        }
        if e16 < text.encode_utf16().count() || table.iter().any(|e| e.0 == e16) {
            new.push((e16, after));
        }
        new.sort_by_key(|e| e.0);
        new.dedup_by_key(|e| e.0);

        self.touch(h, storage);
        let Some(a) = self.file.archive_mut(storage) else { return false };
        let t = a.obj_mut().msg_mut_or_create("table_char_style");
        t.remove("entries");
        for (ci, obj) in new {
            let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
            e.set_u64("character_index", ci as u64);
            if let Some(o) = obj {
                e.set_ref("object", o);
            }
            t.push_msg("entries", e);
        }
        self.file.refresh_refs(storage);
        if let Some(f) = self.file.file_of(storage).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        true
    }
}

impl Document {
    /// Change the list (bullet) level of the paragraphs overlapping bytes `start..end`
    /// (Tab / Shift+Tab, Format > Increase/Decrease Indent Level). Levels are 0..=8.
    pub fn change_list_level(&mut self, h: &mut History, storage: ObjId, start: usize, end: usize, delta: i32) -> bool {
        let text = self.storage_text(storage);
        // paragraph starts in UTF-16 units, and which overlap the range
        let mut starts16 = vec![0usize];
        let mut u = 0usize;
        let mut byte_starts = vec![0usize];
        for (b, ch) in text.char_indices() {
            u += ch.len_utf16();
            // Keynote writes '\n'; AppleScript-made text uses '\r'
            if matches!(ch, '\n' | '\r' | '\u{2029}') {
                starts16.push(u);
                byte_starts.push(b + 1);
            }
        }
        let affected: Vec<usize> = (0..starts16.len())
            .filter(|&i| {
                let ps = byte_starts[i];
                let pe = byte_starts.get(i + 1).map(|x| x - 1).unwrap_or(text.len());
                ps <= end && pe >= start
            })
            .map(|i| starts16[i])
            .collect();
        if affected.is_empty() {
            return false;
        }
        self.touch(h, storage);
        let Some(st) = self.file.obj_mut(storage) else { return false };
        let t = st.msg_mut_or_create("table_para_data");
        let mut entries: Vec<(usize, u32, u32)> =
            t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0) as usize, e.u32("first").unwrap_or(0), e.u32("second").unwrap_or(0))).collect();
        entries.sort_by_key(|e| e.0);
        let level_at = |es: &[(usize, u32, u32)], i: usize| es.iter().take_while(|e| e.0 <= i).last().map(|e| (e.1, e.2)).unwrap_or((0, 0));
        // make sure every paragraph start has its own entry, then change the affected ones
        let mut new: Vec<(usize, u32, u32)> = starts16.iter().map(|&p| { let (f, s2) = level_at(&entries, p); (p, f, s2) }).collect();
        for e in &mut new {
            if affected.contains(&e.0) {
                e.1 = (e.1 as i32 + delta).clamp(0, 8) as u32;
            }
        }
        t.remove("entries");
        for (ci, first, second) in new {
            let mut e = Msg::new("TSWP.ParaDataAttributeTable.ParaDataAttribute");
            e.set_u64("character_index", ci as u64);
            e.set_u64("first", u64::from(first));
            e.set_u64("second", u64::from(second));
            t.push_msg("entries", e);
        }
        true
    }

    /// Upper/lower/capitalised case for bytes `start..end` (Edit > Transformations).
    pub fn transform_case(&mut self, h: &mut History, storage: ObjId, start: usize, end: usize, how: &str) -> bool {
        let text = self.storage_text(storage);
        let (s, e) = (start.min(text.len()), end.min(text.len()));
        if s >= e || !text.is_char_boundary(s) || !text.is_char_boundary(e) {
            return false;
        }
        let part = &text[s..e];
        let out: String = match how {
            "upper" => part.to_uppercase(),
            "lower" => part.to_lowercase(),
            _ => {
                let mut o = String::new();
                let mut first = true;
                for c in part.chars() {
                    if first && c.is_alphabetic() {
                        o.extend(c.to_uppercase());
                    } else {
                        o.extend(c.to_lowercase());
                    }
                    first = c.is_whitespace();
                }
                o
            }
        };
        self.set_text_by_diff(h, storage, &format!("{}{}{}", &text[..s], out, &text[e..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DrawableKind, Color};

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn theme_shape_presets_apply() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/02-shape.key");
        let mut d = Document::open(p).unwrap();
        let presets = d.shape_style_presets(false);
        assert!(presets.len() >= 6, "theme has shape presets: {presets:?}");
        let s = d.slides();
        let shape = d.scene(&s, 0).unwrap().drawables.into_iter().find(|x| !x.from_template && matches!(&x.kind, DrawableKind::Shape(_))).unwrap().id;
        let mut h = History::default();
        d.begin(&mut h, "Preset");
        assert!(d.apply_shape_style(&mut h, shape, presets[2]));
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let want = crate::style::graphic(&d2.file, Some(presets[2])).fill;
        let sc = d2.scene(&d2.slides(), 0).unwrap();
        let DrawableKind::Shape(sh) = &sc.drawables.iter().find(|x| x.id == shape).unwrap().kind else { panic!() };
        assert_eq!(sh.fill, want);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn replace_font_everywhere() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let mut d = Document::open(p).unwrap();
        let used = d.fonts_used();
        let (from, n) = used.iter().max_by_key(|u| u.1).unwrap().clone();
        let mut h = History::default();
        d.begin(&mut h, "Replace Fonts");
        assert_eq!(d.replace_font(&mut h, &from, "DejaVuSans"), n);
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        assert!(!d2.fonts_used().iter().any(|u| u.0 == from));
        assert!(d.undo(&mut h));
        assert!(d.fonts_used().iter().any(|u| u.0 == from));
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn spacing_props_survive_save() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/01-text-box.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let id = d.scene(&s, 0).unwrap().drawables.into_iter().find(|x| !x.from_template && matches!(&x.kind, DrawableKind::Shape(sh) if sh.text.is_some())).unwrap().id;
        let mut h = History::default();
        d.begin(&mut h, "Spacing");
        assert!(d.set_text_prop(&mut h, id, &TextProp::Tracking(0.1)));
        assert!(d.set_text_prop(&mut h, id, &TextProp::SpaceBefore(12.0)));
        assert!(d.set_text_prop(&mut h, id, &TextProp::SpaceAfter(6.0)));
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let sc = d2.scene(&d2.slides(), 0).unwrap();
        let DrawableKind::Shape(sh) = &sc.drawables.iter().find(|x| x.id == id).unwrap().kind else { panic!() };
        let p0 = &sh.text.as_ref().unwrap().paragraphs[0];
        assert_eq!((p0.space_before, p0.space_after), (12.0, 6.0));
        assert!((p0.runs[0].style.tracking - 0.1).abs() < 1e-6);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn bold_one_word_survives_save() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let mut d = Document::open(p).unwrap();
        let (sid, _) = d.file.objects_of("TSWP.StorageArchive").find(|(_, m)| m.strs("text").concat().starts_with("Mixed: word1")).unwrap();
        let text = d.storage_text(sid);
        let w3 = text.find("word3").unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Bold");
        assert!(d.set_text_prop_range(&mut h, sid, w3, w3 + 5, &TextProp::Bold(true)));
        assert!(d.set_text_prop_range(&mut h, sid, 7, 12, &TextProp::Color(Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }))); // word1
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let s = d2.slides();
        let tb = (0..s.len())
            .flat_map(|i| d2.scene(&s, i).unwrap().drawables)
            .find_map(|x| match x.kind {
                DrawableKind::Shape(sh) => sh.text.filter(|t| t.storage == sid),
                _ => None,
            })
            .unwrap();
        let runs = &tb.paragraphs[0].runs;
        let style_of = |w: &str| runs.iter().find(|r| tb.text[r.start..r.end].contains(w)).unwrap().style.clone();
        assert!(style_of("word3").bold);
        assert!(!style_of("word4").bold);
        assert!(!style_of("Mixed").bold);
        assert!(style_of("word1").color.r > 0.9, "word1 now red");
        assert!(style_of("word2").font_name.contains("Courier"), "word2 keeps Courier");
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn indent_levels_and_case() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let mut d = Document::open(p).unwrap();
        let (sid, _) = d.file.objects_of("TSWP.StorageArchive").find(|(_, m)| m.strs("text").concat().starts_with("Body placeholder of layout \"Title & Bullets\"")).unwrap();
        let text = d.storage_text(sid);
        let second = text.find("Second").unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Indent");
        assert!(d.change_list_level(&mut h, sid, second, second, 1));
        assert!(d.transform_case(&mut h, sid, 0, 4, "upper"));
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let s = d2.slides();
        let tb = (0..s.len())
            .flat_map(|i| d2.scene(&s, i).unwrap().drawables)
            .find_map(|x| match x.kind {
                DrawableKind::Shape(sh) => sh.text.filter(|t| t.storage == sid),
                _ => None,
            })
            .unwrap();
        assert!(tb.text.starts_with("BODY placeholder"));
        assert_eq!(tb.paragraphs[0].level, 0);
        assert_eq!(tb.paragraphs[1].level, 1, "second bullet indented");
        assert_eq!(tb.paragraphs[2].level, 0);
    }
}
