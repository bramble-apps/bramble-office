//! Text editing inside a TSWP storage that keeps formatting: attribute tables (keyed by UTF-16
//! index) are shifted around the edit instead of being clamped, and new paragraphs inherit
//! the paragraph attributes of the paragraph they were split from.

use kn_format::ObjId;

use crate::edit::History;
use crate::Document;

/// Tables whose entries mark paragraph starts.
const PARA_TABLES: &[&str] = &[
    "table_para_style",
    "table_list_style",
    "table_para_data",
    "table_para_starts",
    "table_para_bidi",
    "table_drop_cap_style",
];

/// Tables whose entries mark character-run starts.
const CHAR_TABLES: &[&str] = &[
    "table_char_style",
    "table_language",
    "table_smartfield",
    "table_attachment",
    "table_highlight",
    "table_dictation",
    "table_insertion",
    "table_deletion",
    "table_rubyfield",
    "table_tatechuyoko",
    "table_bookmark",
];

fn u16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Byte offset in `s` -> UTF-16 index.
pub fn byte_to_u16(s: &str, byte: usize) -> usize {
    s[..byte.min(s.len())].encode_utf16().count()
}

impl Document {
    /// Plain text of a storage exactly as stored (paragraphs separated by '\n').
    pub fn storage_text(&self, storage: ObjId) -> String {
        self.file.obj(storage).map(|s| s.strs("text").concat()).unwrap_or_default()
    }

    /// Replace bytes `start..end` of the storage's text with `insert`, keeping formatting.
    /// Call between `begin`/`end` (typing in one session is one undo step).
    pub fn replace_text(&mut self, h: &mut History, storage: ObjId, start: usize, end: usize, insert: &str) {
        self.touch(h, storage);
        let Some(st) = self.file.obj_mut(storage) else { return };
        let old: String = st.strs("text").concat();
        let (start, end) = (start.min(old.len()), end.min(old.len()).max(start.min(old.len())));
        if !old.is_char_boundary(start) || !old.is_char_boundary(end) {
            return;
        }
        let s16 = byte_to_u16(&old, start);
        let e16 = byte_to_u16(&old, end);
        let ins16 = u16_len(insert);
        let delta = ins16 as i64 - (e16 - s16) as i64;
        let new_text = format!("{}{}{}", &old[..start], insert, &old[end..]);
        st.set_strs("text", &[&new_text]);

        // new paragraph starts created by the insertion (UTF-16 positions in the new text)
        let mut new_paras = Vec::new();
        let mut pos = s16;
        for ch in insert.chars() {
            pos += ch.len_utf16();
            if ch == '\n' {
                new_paras.push(pos);
            }
        }

        for (tables, is_para) in [(PARA_TABLES, true), (CHAR_TABLES, false)] {
            for name in tables {
                let Some(t) = st.msg_mut(name) else { continue };
                // the entry in force at the edit start (template for new paragraph entries)
                let mut template = None;
                for e in t.msgs("entries") {
                    if e.u64("character_index").unwrap_or(0) as usize <= s16 {
                        template = Some(e.clone());
                    }
                }
                let typing_at_boundary = !is_para && s16 == e16 && s16 > 0;
                // a deleted field (U+FFFC) takes its attachment entry with it
                let attachment = *name == "table_attachment";
                let mut drop = Vec::new();
                for (i, e) in t.msgs_mut("entries").enumerate() {
                    let ci = e.u64("character_index").unwrap_or(0) as usize;
                    if attachment && ci >= s16 && ci < e16 {
                        drop.push(i);
                        continue;
                    }
                    let nci = if ci > e16 || (typing_at_boundary && ci == s16) || (attachment && ci >= e16) {
                        // after the edit (or a run starting exactly where we type: the typed
                        // text continues the previous run, as in Keynote)
                        (ci as i64 + delta).max(0) as usize
                    } else if ci > s16 {
                        // inside the replaced range
                        if is_para {
                            drop.push(i); // merged paragraphs keep the first one's style
                            continue;
                        }
                        s16 + ins16
                    } else {
                        ci
                    };
                    e.set_u64("character_index", nci as u64);
                }
                let mut i = 0;
                t.retain_msgs("entries", |_| {
                    let keep = !drop.contains(&i);
                    i += 1;
                    keep
                });
                if is_para {
                    if let Some(tpl) = template {
                        for p in &new_paras {
                            let mut e = tpl.clone();
                            e.set_u64("character_index", *p as u64);
                            // insert in index order
                            let at = t.msgs("entries").take_while(|x| x.u64("character_index").unwrap_or(0) < *p as u64).count();
                            t.insert_msg("entries", at, e);
                        }
                    }
                }
                // several entries may now share an index: keep the last of each
                let idx: Vec<u64> = t.msgs("entries").map(|e| e.u64("character_index").unwrap_or(0)).collect();
                let keep: Vec<bool> = (0..idx.len()).map(|i| !idx[i + 1..].contains(&idx[i])).collect();
                let mut j = 0;
                t.retain_msgs("entries", |_| {
                    let k = keep[j];
                    j += 1;
                    k
                });
            }
        }
    }
}

impl Document {
    /// Set a storage's whole text, replacing only the part that changed (common prefix and
    /// suffix kept), so formatting outside the edit survives.
    pub fn set_text_by_diff(&mut self, h: &mut History, storage: ObjId, new: &str) -> bool {
        let old = self.storage_text(storage);
        if old == new {
            return false;
        }
        let mut pre = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
        while !old.is_char_boundary(pre) || !new.is_char_boundary(pre) {
            pre -= 1;
        }
        let max_suf = old.len().min(new.len()) - pre;
        let mut suf = old.bytes().rev().zip(new.bytes().rev()).take(max_suf).take_while(|(a, b)| a == b).count();
        while !old.is_char_boundary(old.len() - suf) || !new.is_char_boundary(new.len() - suf) {
            suf -= 1;
        }
        self.replace_text(h, storage, pre, old.len() - suf, &new[pre..new.len() - suf]);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Document {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        Document::open(p).unwrap()
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn typing_keeps_word_colours() {
        let mut d = doc();
        // "Mixed: word1 word2 word3 word4" — word1 is blue, word2 Courier
        let (sid, _) = d.file.objects_of("TSWP.StorageArchive").find(|(_, m)| m.strs("text").concat().starts_with("Mixed: word1")).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Typing");
        d.replace_text(&mut h, sid, 0, 0, "Very ");
        d.end(&mut h);
        assert_eq!(d.storage_text(sid), "Very Mixed: word1 word2 word3 word4");
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let d2 = Document::from_file(k);
        let slides = d2.slides();
        let sc = (0..slides.len())
            .filter_map(|i| d2.scene(&slides, i))
            .find(|s| s.drawables.iter().any(|x| matches!(&x.kind, crate::DrawableKind::Shape(s) if s.text.as_ref().is_some_and(|t| t.storage == sid))))
            .unwrap();
        let tb = sc
            .drawables
            .iter()
            .find_map(|x| match &x.kind {
                crate::DrawableKind::Shape(s) => s.text.as_ref().filter(|t| t.storage == sid),
                _ => None,
            })
            .unwrap();
        let run = tb.paragraphs[0].runs.iter().find(|r| &tb.text[r.start..r.end] == "word1").expect("word1 still its own run");
        assert!(run.style.color.b > 0.5 && run.style.color.r < 0.5, "word1 stays blue");
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn enter_splits_paragraph_with_same_style() {
        let mut d = doc();
        let (sid, _) = d.file.objects_of("TSWP.StorageArchive").find(|(_, m)| m.strs("text").concat().starts_with("Body placeholder of layout \"Title & Bullets\"")).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Typing");
        let t = d.storage_text(sid);
        let at = t.find("Second").unwrap();
        d.replace_text(&mut h, sid, at, at, "New bullet\n");
        d.end(&mut h);
        let st = d.file.obj(sid).unwrap();
        let paras = st.msg("table_para_style").unwrap().msgs("entries").count();
        let lists = st.msg("table_list_style").unwrap().msgs("entries").count();
        assert!(paras >= 3 && lists >= 1);
        assert_eq!(d.storage_text(sid).matches('\n').count(), t.matches('\n').count() + 1);
        assert!(d.undo(&mut h));
        assert_eq!(d.storage_text(sid), t);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn diff_edit_only_touches_changed_words() {
        let mut d = doc();
        let (sid, _) = d.file.objects_of("TSWP.StorageArchive").find(|(_, m)| m.strs("text").concat().starts_with("Mixed: word1")).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Edit");
        assert!(d.set_text_by_diff(&mut h, sid, "Mixed: word1 CHANGED word3 word4"));
        d.end(&mut h);
        assert_eq!(d.storage_text(sid), "Mixed: word1 CHANGED word3 word4");
        let st = d.file.obj(sid).unwrap();
        // the blue run for word1 still starts at index 7
        assert!(st.msg("table_char_style").unwrap().msgs("entries").any(|e| e.u64("character_index") == Some(7) && e.ref_id("object").is_some()));
    }

    /// Text around a field (slide number) can be edited: the field moves with the text, and
    /// deleting it removes its attachment entry.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn edit_around_field() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let mut d = Document::open(p).unwrap();
        let sc = d.scene(&d.slides(), 0).unwrap();
        let tb = sc.drawables.iter().find(|x| x.id == 32590).and_then(|x| match &x.kind {
            crate::DrawableKind::Shape(s) => s.text.clone(),
            _ => None,
        }).unwrap();
        let st = tb.storage;
        let mut h = History::default();
        d.begin(&mut h, "Typing");
        d.replace_text(&mut h, st, 0, 0, "Slide ");
        d.end(&mut h);
        let sc = d.scene(&d.slides(), 0).unwrap();
        let shown = |d: &Document| d.scene(&d.slides(), 0).unwrap().drawables.iter().find(|x| x.id == 32590).and_then(|x| match &x.kind {
            crate::DrawableKind::Shape(s) => s.text.as_ref().map(|t| (t.text.clone(), t.fields.clone())),
            _ => None,
        }).unwrap();
        let _ = sc;
        assert_eq!(shown(&d), ("Slide 1".to_string(), vec![(6, 1, 6)]));
        let att = |d: &Document| d.file.obj(st).unwrap().msg("table_attachment").map(|t| t.msgs("entries").filter(|e| e.ref_id("object").is_some()).count()).unwrap_or(0);
        assert_eq!(att(&d), 1);
        d.begin(&mut h, "Typing");
        d.replace_text(&mut h, st, 6, 9, "");
        d.end(&mut h);
        assert_eq!(shown(&d).0, "Slide ");
        assert_eq!(att(&d), 0);
    }
}
