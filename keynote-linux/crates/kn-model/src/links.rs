//! Links on text (Format > Add Link > Webpage / Email / Phone Number, and Smart Web and Email
//! Links): a TSWP.HyperlinkFieldArchive (its address in url_ref) on the linked characters in
//! the storage's table_smartfield, the characters in the stylesheet's "Link" character style
//! (underlined), as Numbers 15.4 writes them (Mac round N29 A1 / A2). Keynote and Pages store
//! them the same way.

use kn_format::{Archive, Msg, ObjId};

use crate::edit::History;
use crate::Document;

/// Set `obj` (None = nothing) on UTF-16 range `ua..ub` of attribute table `table`; what was
/// there continues after it.
fn put_range(st: &mut Msg, table: &str, ua: u64, ub: u64, obj: Option<ObjId>) {
    let mut entries: Vec<(u64, Option<ObjId>)> = st.msg(table).map(|t| t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0), e.ref_id("object"))).collect()).unwrap_or_default();
    entries.sort_by_key(|e| e.0);
    let after = entries.iter().filter(|e| e.0 <= ub).last().and_then(|e| e.1);
    entries.retain(|e| e.0 < ua || e.0 > ub);
    entries.push((ua, obj));
    entries.push((ub, after));
    if !entries.iter().any(|e| e.0 == 0) {
        entries.push((0, None));
    }
    entries.sort_by_key(|e| e.0);
    // neighbours with the same object merge
    entries.dedup_by(|b, a| a.1 == b.1);
    st.remove(table);
    let t = st.msg_mut_or_create(table);
    for (i, o) in entries {
        let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
        e.set_u64("character_index", i);
        if let Some(o) = o {
            e.set_ref("object", o);
        }
        t.push_msg("entries", e);
    }
}

impl Document {
    /// The stylesheet's "Link" character style (style_identifier character-style-hyperlink).
    fn link_char_style(&self) -> Option<ObjId> {
        self.file
            .objects_of("TSWP.CharacterStyleArchive")
            .find(|(_, m)| m.find_super("TSS.StyleArchive").and_then(|s| s.str("style_identifier")) == Some("character-style-hyperlink"))
            .map(|(id, _)| id)
    }

    /// Link bytes `a..b` of text storage `storage` to `url` ("https://…", "mailto:…", "tel:…").
    pub fn add_text_link(&mut self, h: &mut History, storage: ObjId, a: usize, b: usize, url: &str) -> Option<ObjId> {
        let file = self.file.file_of(storage)?.to_string();
        let raw = self.storage_text(storage);
        let (a, b) = (a.min(raw.len()), b.min(raw.len()));
        if a >= b || !raw.is_char_boundary(a) || !raw.is_char_boundary(b) {
            return None;
        }
        // a link over (part of) another replaces it
        let old: Vec<(u64, u64)> = self
            .text_links(storage)
            .into_iter()
            .filter(|l| l.0 < b && a < l.1)
            .map(|l| (raw[..l.0].encode_utf16().count() as u64, raw[..l.1].encode_utf16().count() as u64))
            .collect();
        let (ua, ub) = (raw[..a].encode_utf16().count() as u64, raw[..b].encode_utf16().count() as u64);
        let link_style = self.link_char_style();
        let url = url.to_string();
        let gone = self.link_objects_in(storage, &old);
        self.structural(h, &[storage], |k| {
            // the replaced links' field objects go with them (no orphans)
            for g in &gone {
                k.remove_archive(*g);
            }
            let id = k.alloc_ids(1)[0];
            let mut f = Msg::new("TSWP.HyperlinkFieldArchive");
            let mut tmp = Msg::new("TSP.UUID");
            kn_format::objects::new_uuid(&mut tmp);
            let mut bytes = [0u8; 16];
            bytes[..8].copy_from_slice(&tmp.u64("lower").unwrap_or(0).to_le_bytes());
            bytes[8..].copy_from_slice(&tmp.u64("upper").unwrap_or(0).to_le_bytes());
            f.msg_mut_or_create("super").set_str("text_attribute_uuid_string", &format!("{:X}", crate::comments::uuid_fmt(bytes)));
            f.set_str("url_ref", &url);
            k.insert_archive(&file, Archive::new_object(id, f));
            let st = k.obj_mut(storage)?;
            for (oa, ob) in &old {
                put_range(st, "table_smartfield", *oa, *ob, None);
                if link_style.is_some() {
                    put_range(st, "table_char_style", *oa, *ob, None);
                }
            }
            put_range(st, "table_smartfield", ua, ub, Some(id));
            if let Some(ls) = link_style {
                put_range(st, "table_char_style", ua, ub, Some(ls));
            }
            k.refresh_refs(storage);
            k.ensure_external_refs(&file);
            Some(id)
        })
    }

    /// The links on a text storage: (start byte, end byte, address).
    pub fn text_links(&self, storage: ObjId) -> Vec<(usize, usize, String)> {
        let Some(st) = self.file.obj(storage) else { return Vec::new() };
        let raw = st.str("text").unwrap_or("").to_string();
        let to_byte = |u: u64| {
            let mut n = 0u64;
            for (i, c) in raw.char_indices() {
                if n >= u {
                    return i;
                }
                n += c.len_utf16() as u64;
            }
            raw.len()
        };
        let entries: Vec<(u64, Option<ObjId>)> = st.msg("table_smartfield").map(|t| t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0), e.ref_id("object"))).collect()).unwrap_or_default();
        let mut out = Vec::new();
        for (i, (at, o)) in entries.iter().enumerate() {
            let Some(url) = o.and_then(|o| self.file.obj(o)).filter(|m| m.type_name() == "TSWP.HyperlinkFieldArchive").and_then(|m| m.str("url_ref")) else { continue };
            let end = entries.get(i + 1).map(|e| e.0).unwrap_or(raw.encode_utf16().count() as u64);
            out.push((to_byte(*at), to_byte(end), url.to_string()));
        }
        out
    }

    /// Remove the link covering byte `at` of the storage (its characters keep their text; the
    /// Link character style comes off, any other character style stays).
    pub fn remove_text_link(&mut self, h: &mut History, storage: ObjId, at: usize) -> bool {
        let Some((a, b, _)) = self.text_links(storage).into_iter().find(|l| l.0 <= at && at < l.1.max(l.0 + 1)) else { return false };
        let raw = self.storage_text(storage);
        let (ua, ub) = (raw[..a].encode_utf16().count() as u64, raw[..b].encode_utf16().count() as u64);
        let link_style = self.link_char_style();
        let gone = self.link_objects_in(storage, &[(ua, ub)]);
        self.structural(h, &[storage], |k| {
            for g in &gone {
                k.remove_archive(*g);
            }
            let Some(st) = k.obj_mut(storage) else { return false };
            let styled = link_style.is_some()
                && st.msg("table_char_style").is_some_and(|t| t.msgs("entries").filter(|e| e.u64("character_index").unwrap_or(0) <= ua).last().and_then(|e| e.ref_id("object")) == link_style);
            put_range(st, "table_smartfield", ua, ub, None);
            if styled {
                put_range(st, "table_char_style", ua, ub, None);
            }
            k.refresh_refs(storage);
            true
        })
    }

    /// The hyperlink field objects starting inside the given UTF-16 ranges of a storage.
    fn link_objects_in(&self, storage: ObjId, ranges: &[(u64, u64)]) -> Vec<ObjId> {
        let Some(st) = self.file.obj(storage) else { return Vec::new() };
        st.msg("table_smartfield")
            .map(|t| {
                t.msgs("entries")
                    .filter(|e| ranges.iter().any(|(a, b)| (*a..*b).contains(&e.u64("character_index").unwrap_or(0))))
                    .filter_map(|e| e.ref_id("object"))
                    .filter(|o| self.file.type_name(*o) == "TSWP.HyperlinkFieldArchive")
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use crate::edit::History;
    use crate::Document;

    /// A link on "Apple" in "Visit Apple now": read back after saving, then removed.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn text_link_round_trip() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R10-standard.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides()[0].slide;
        let mut h = History::default();
        d.begin(&mut h, "Text");
        let id = d.insert(&mut h, s, &crate::insert::NewObject::TextBox).unwrap();
        let st = d.storage_of(id).unwrap();
        d.set_storage_text(&mut h, st, "Visit Apple now");
        assert!(d.add_text_link(&mut h, st, 6, 11, "https://www.apple.com").is_some());
        d.end(&mut h);
        let mut d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        assert_eq!(d2.text_links(st), vec![(6, 11, "https://www.apple.com".to_string())]);
        assert_eq!(d2.storage_text(st), "Visit Apple now");
        let mut h2 = History::default();
        // typing before it: the link moves with its word
        d2.begin(&mut h2, "Typing");
        d2.replace_text(&mut h2, st, 0, 0, "Do ");
        d2.end(&mut h2);
        assert_eq!(d2.text_links(st), vec![(9, 14, "https://www.apple.com".to_string())]);
        // a link over part of it replaces it (no second, nested link)
        d2.begin(&mut h2, "Link");
        d2.add_text_link(&mut h2, st, 9, 12, "https://example.com");
        d2.end(&mut h2);
        assert_eq!(d2.text_links(st), vec![(9, 12, "https://example.com".to_string())]);
        d2.begin(&mut h2, "Remove Link");
        assert!(d2.remove_text_link(&mut h2, st, 10));
        d2.end(&mut h2);
        assert!(d2.text_links(st).is_empty());
        // no hyperlink field left without its text
        assert_eq!(d2.file.objects_of("TSWP.HyperlinkFieldArchive").count(), 0);
    }
}
