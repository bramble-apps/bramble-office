//! Comments (sticky notes attached to objects): TSD.CommentStorageArchive, authors in
//! TSK.AnnotationAuthorArchive.

use kn_format::{Archive, Msg, ObjId};

use crate::edit::History;
use crate::types::Color;
use crate::Document;

#[derive(Debug, Clone, PartialEq)]
pub struct CommentInfo {
    pub id: ObjId,
    pub drawable: ObjId,
    pub text: String,
    pub author: String,
    pub color: Option<Color>,
    /// Seconds since 2001-01-01 (Apple reference date).
    pub date: Option<f64>,
}

impl Document {
    /// The comment attached to a drawable, if any.
    pub fn comment(&self, drawable: ObjId) -> Option<CommentInfo> {
        let d = self.file.obj(drawable)?.find_super("TSD.DrawableArchive")?;
        let cid = d.ref_id("comment")?;
        let c = self.file.obj(cid)?;
        let author = c.ref_id("author").and_then(|a| self.file.obj(a));
        Some(CommentInfo {
            id: cid,
            drawable,
            text: c.str("text").unwrap_or("").to_string(),
            author: author.and_then(|a| a.str("name")).unwrap_or("").to_string(),
            color: author.and_then(|a| a.msg("color")).map(crate::style::color),
            date: c.msg("creation_date").and_then(|d| d.f64("seconds")),
        })
    }

    /// View > Comments > Author Color: the colour of `author`'s comments (nothing to do when
    /// the author has written none in this document yet).
    pub fn set_author_color(&mut self, h: &mut History, author: &str, color: crate::Color) -> bool {
        let Some((store, _)) = self.file.first_of("TSK.AnnotationAuthorStorageArchive") else { return false };
        let Some(id) = self.file.obj(store).map(|s| s.ref_ids("annotation_author")).unwrap_or_default().into_iter().find(|a| self.file.obj(*a).and_then(|m| m.str("name")) == Some(author)) else {
            return false;
        };
        self.touch(h, id);
        let Some(a) = self.file.obj_mut(id) else { return false };
        a.remove("color");
        a.push_msg("color", crate::format::color_msg(color));
        true
    }

    /// The comment author of that name (created with Keynote's yellow when new). Call inside a
    /// `touch_all` edit.
    fn author_id(&mut self, author: &str) -> Option<ObjId> {
        let (store, _) = self.file.first_of("TSK.AnnotationAuthorStorageArchive")?;
        let existing = self.file.obj(store)?.ref_ids("annotation_author").into_iter().find(|a| self.file.obj(*a).and_then(|m| m.str("name")) == Some(author));
        if let Some(a) = existing {
            return Some(a);
        }
        let id = self.file.alloc_ids(1)[0];
        let mut a = Msg::new("TSK.AnnotationAuthorArchive");
        a.set_str("name", author);
        let mut c = Msg::new("TSP.Color");
        c.set_enum("model", "rgb");
        for (k, v) in [("r", 0.98039216), ("g", 0.9372549), ("b", 0.3529412), ("a", 1.0)] {
            c.set_f32(k, v);
        }
        c.set_enum("rgbspace", "srgb");
        a.push_msg("color", c);
        a.set_bool("is_public_author", false);
        let sfile = self.file.file_of(store)?.to_string();
        self.file.insert_archive(&sfile, Archive::new_object(id, a));
        let sa = self.file.archive_mut(store)?;
        sa.obj_mut().push_ref("annotation_author", id);
        sa.refresh_object_references();
        Some(id)
    }

    /// Insert > Highlight: mark bytes `a..b` of a text storage as a review highlight (with an
    /// empty comment by `author`), stored as Keynote does: table_highlight -> HighlightArchive ->
    /// CommentStorageArchive -> author.
    pub fn add_highlight(&mut self, h: &mut History, storage: ObjId, a: usize, b: usize, author: &str) -> Option<ObjId> {
        let file = self.file.file_of(storage)?.to_string();
        let raw = self.storage_text(storage);
        let (a, b) = (a.min(raw.len()), b.min(raw.len()));
        if a >= b || !raw.is_char_boundary(a) || !raw.is_char_boundary(b) {
            return None;
        }
        let (ua, ub) = (raw[..a].encode_utf16().count() as u64, raw[..b].encode_utf16().count() as u64);
        self.touch_all(h);
        let author_id = self.author_id(author)?;
        let ids = self.file.alloc_ids(2);
        let (cid, hid) = (ids[0], ids[1]);
        let mut c = Msg::new("TSD.CommentStorageArchive");
        c.set_str("text", "");
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let mut date = Msg::new("TSP.Date");
        date.set_f64("seconds", now - 978_307_200.0);
        c.push_msg("creation_date", date);
        c.set_ref("author", author_id);
        kn_format::objects::new_uuid(c.msg_mut_or_create("storage_uuid"));
        self.file.insert_archive(&file, Archive::new_object(cid, c));
        let mut hl = Msg::new("TSWP.HighlightArchive");
        hl.set_ref("commentStorage", cid);
        let mut tmp = Msg::new("TSP.UUID");
        kn_format::objects::new_uuid(&mut tmp);
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&tmp.u64("lower").unwrap_or(0).to_le_bytes());
        bytes[8..].copy_from_slice(&tmp.u64("upper").unwrap_or(0).to_le_bytes());
        hl.set_str("text_attribute_uuid_string", &format!("{:X}", uuid_fmt(bytes)));
        self.file.insert_archive(&file, Archive::new_object(hid, hl));
        self.file.add_uuid_entry(&file, cid);
        // the attribute table: the new range, what was there continues after it
        let st = self.file.obj_mut(storage)?;
        let mut entries: Vec<(u64, Option<ObjId>)> = st
            .msg("table_highlight")
            .map(|t| t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0), e.ref_id("object"))).collect())
            .unwrap_or_default();
        entries.sort_by_key(|e| e.0);
        let after = entries.iter().filter(|e| e.0 <= ub).last().and_then(|e| e.1);
        entries.retain(|e| e.0 < ua || e.0 > ub);
        entries.push((ua, Some(hid)));
        entries.push((ub, after));
        if !entries.iter().any(|e| e.0 == 0) {
            entries.push((0, None));
        }
        entries.sort_by_key(|e| e.0);
        st.remove("table_highlight");
        let t = st.msg_mut_or_create("table_highlight");
        for (i, o) in entries {
            let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
            e.set_u64("character_index", i);
            if let Some(o) = o {
                e.set_ref("object", o);
            }
            t.push_msg("entries", e);
        }
        self.file.refresh_refs(storage);
        self.file.ensure_external_refs(&file);
        Some(hid)
    }

    /// Insert > Comment: attach a new comment to a drawable, written by `author` (an existing
    /// comment author of that name, else a new one with Keynote's yellow).
    pub fn add_comment(&mut self, h: &mut History, drawable: ObjId, text: &str, author: &str) -> Option<ObjId> {
        let file = self.file.file_of(drawable)?.to_string();
        let (store, _) = self.file.first_of("TSK.AnnotationAuthorStorageArchive")?;
        self.touch_all(h);
        let _ = store;
        let author_id = self.author_id(author)?;
        let id = self.file.alloc_ids(1)[0];
        let mut c = Msg::new("TSD.CommentStorageArchive");
        c.set_str("text", text);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let mut date = Msg::new("TSP.Date");
        date.set_f64("seconds", now - 978_307_200.0);
        c.push_msg("creation_date", date);
        c.set_ref("author", author_id);
        kn_format::objects::new_uuid(c.msg_mut_or_create("storage_uuid"));
        let mut arch = Archive::new_object(id, c);
        arch.refresh_object_references();
        self.file.insert_archive(&file, arch);
        let a = self.file.archive_mut(drawable)?;
        a.payloads[0].find_super_mut("TSD.DrawableArchive")?.set_ref("comment", id);
        a.refresh_object_references();
        self.file.ensure_external_refs(&file);
        Some(id)
    }

    pub fn set_comment_text(&mut self, h: &mut History, comment: ObjId, text: &str) {
        self.touch(h, comment);
        if let Some(c) = self.file.obj_mut(comment) {
            c.set_str("text", text);
        }
    }

    /// Detach a drawable's comment (the comment object is left for Keynote to clean up).
    pub fn delete_comment(&mut self, h: &mut History, drawable: ObjId) {
        self.touch(h, drawable);
        if let Some(a) = self.file.archive_mut(drawable) {
            if let Some(d) = a.payloads[0].find_super_mut("TSD.DrawableArchive") {
                d.remove("comment");
            }
            a.refresh_object_references();
        }
    }
}

/// "17 Oct 2026" from seconds since 2001-01-01.
pub fn format_date(secs: f64) -> String {
    let days = (secs / 86400.0).floor() as i64 + 11323;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{d} {} {}", months[(m - 1) as usize], if m <= 2 { y + 1 } else { y })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn reads_and_edits_catalog_comment() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let mut d = Document::open(p).unwrap();
        let slides = d.slides();
        let found = (0..slides.len())
            .flat_map(|i| d.scene(&slides, i).unwrap().drawables)
            .find_map(|x| d.comment(x.id))
            .expect("catalog has a comment");
        assert!(!found.text.is_empty());
        let mut h = History::default();
        d.begin(&mut h, "Edit Comment");
        d.set_comment_text(&mut h, found.id, "Edited on Linux");
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        assert_eq!(d2.comment(found.drawable).unwrap().text, "Edited on Linux");
        d.begin(&mut h, "Delete Comment");
        d.delete_comment(&mut h, found.drawable);
        d.end(&mut h);
        assert!(d.comment(found.drawable).is_none());
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn adds_comment_with_new_author() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let obj = (0..s.len()).flat_map(|i| d.scene(&s, i).unwrap().drawables).find(|x| !x.from_template && d.comment(x.id).is_none()).unwrap().id;
        let mut h = History::default();
        d.begin(&mut h, "Add Comment");
        d.add_comment(&mut h, obj, "Hello from Linux", "Linux Tester").unwrap();
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let c = d2.comment(obj).unwrap();
        assert_eq!((c.text.as_str(), c.author.as_str()), ("Hello from Linux", "Linux Tester"));
        assert!(c.date.unwrap() > 8e8);
    }
}

/// RFC 4122 text form of 16 bytes ("C6361C4E-DAEC-…").
pub(crate) struct UuidFmt([u8; 16]);
pub(crate) fn uuid_fmt(b: [u8; 16]) -> UuidFmt {
    UuidFmt(b)
}
impl std::fmt::UpperHex for UuidFmt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let b = self.0;
        for (i, x) in b.iter().enumerate() {
            if [4, 6, 8, 10].contains(&i) {
                write!(f, "-")?;
            }
            write!(f, "{x:02X}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod highlight_tests {
    use crate::edit::History;
    use crate::{Document, DrawableKind};

    fn texts(d: &Document) -> Vec<(crate::ObjId, crate::TextBox)> {
        let s = d.slides();
        (0..s.len())
            .filter_map(|i| d.scene(&s, i))
            .flat_map(|sc| sc.drawables.into_iter())
            .filter_map(|x| match x.kind {
                DrawableKind::Shape(sh) => sh.text.map(|t| (x.id, t)),
                _ => None,
            })
            .collect()
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn reads_and_writes_highlights() {
        let mut d = Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R12-highlight.key")).unwrap();
        let (_, hl) = texts(&d).into_iter().find(|(_, t)| !t.highlights.is_empty()).unwrap();
        let (a, b, c) = hl.highlights[0];
        assert_eq!(b - a, 11);
        assert!(c.g > 0.9 && c.b < 0.4, "green author colour");
        // add one to another text
        let (_, other) = texts(&d).into_iter().find(|(_, t)| t.highlights.is_empty() && t.text.len() > 6).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Highlight");
        assert!(d.add_highlight(&mut h, other.storage, 0, 5, "Linux Tester").is_some());
        d.end(&mut h);
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let d2 = Document::from_file(k);
        let t2 = texts(&d2).into_iter().find(|(_, t)| t.storage == other.storage).unwrap().1;
        assert_eq!((t2.highlights[0].0, t2.highlights[0].1), (0, 5));
    }
}
