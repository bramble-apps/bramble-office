//! Smart fields and document structure: links (TSWP.HyperlinkFieldArchive + the "Link"
//! character style), bookmarks, date & time, footnotes, section breaks, the table of contents,
//! columns, section settings, and Convert to Page Layout / Word Processing.

use kn_format::{Archive, KeyFile, Msg, ObjId};
use kn_model::edit::History;
use kn_model::textedit::byte_to_u16;

use crate::edit::object_table;
use crate::Pages;

const DONOR_FOOTNOTE: &[u8] = include_bytes!("../data/donors/18-footnote.pages");
const DONOR_TOC: &[u8] = include_bytes!("../data/donors/19-toc.pages");

fn uuid_string() -> String {
    let mut tmp = Msg::new("TSP.UUID");
    kn_format::objects::new_uuid(&mut tmp);
    let (lo, hi) = (tmp.u64("lower").unwrap_or(0), tmp.u64("upper").unwrap_or(0));
    let s = format!("{hi:016X}{lo:016X}");
    format!("{}-{}-{}-{}-{}", &s[0..8], &s[8..12], &s[12..16], &s[16..20], &s[20..32])
}

fn u16_to_byte(text: &str, u: usize) -> usize {
    let mut n = 0;
    for (b, ch) in text.char_indices() {
        if n >= u {
            return b;
        }
        n += ch.len_utf16();
    }
    text.len()
}

impl Pages {
    pub(crate) fn new_object(&mut self, h: &mut History, near: ObjId, m: Msg) -> Option<ObjId> {
        let file = self.file().file_of(near)?.to_string();
        Some(self.doc.structural(h, &[], |k| {
            let id = k.alloc_ids(1)[0];
            let mut a = Archive::new_object(id, m);
            a.refresh_object_references();
            k.insert_archive(&file, a);
            k.ensure_external_refs(&file);
            id
        }))
    }

    /// Format > Add Link over stored bytes a..b: the link field and the "Link" character style.
    pub fn add_link(&mut self, h: &mut History, storage: ObjId, a: usize, b: usize, url: &str) -> bool {
        if a >= b {
            return false;
        }
        let mut f = Msg::new("TSWP.HyperlinkFieldArchive");
        f.msg_mut_or_create("super").set_str("text_attribute_uuid_string", &uuid_string());
        f.set_str("url_ref", url);
        let Some(fid) = self.new_object(h, storage, f) else { return false };
        let text = self.doc.storage_text(storage);
        let (ua, ub) = (byte_to_u16(&text, a), byte_to_u16(&text, b));
        self.doc.touch(h, storage);
        if let Some(st) = self.doc.file.obj_mut(storage) {
            crate::notes::mark(st, "table_smartfield", ua, ub, Some(fid));
        }
        let link_style = self.doc.character_styles().into_iter().find(|(_, n)| n == "Link").map(|x| x.0);
        if let Some(ls) = link_style {
            self.doc.set_character_style(h, storage, a, b, Some(ls));
        }
        self.doc.file.refresh_refs(storage);
        true
    }

    /// The link over stored byte `at`: (field, url, range).
    pub fn link_at(&self, storage: ObjId, at: usize) -> Option<(ObjId, String, usize, usize)> {
        let st = self.file().obj(storage)?;
        let text = self.doc.storage_text(storage);
        let u = byte_to_u16(&text, at);
        let t = object_table(st, "table_smartfield");
        for (i, (ci, obj)) in t.iter().enumerate() {
            let end = t.get(i + 1).map(|e| e.0).unwrap_or(usize::MAX);
            let Some(id) = obj else { continue };
            if *ci <= u && u <= end {
                let m = self.file().obj(*id)?;
                if m.type_name() == "TSWP.HyperlinkFieldArchive" {
                    return Some((*id, m.str("url_ref").unwrap_or("").to_string(), u16_to_byte(&text, *ci), u16_to_byte(&text, end)));
                }
            }
        }
        None
    }

    /// Format > Remove Link.
    pub fn remove_link(&mut self, h: &mut History, storage: ObjId, at: usize) -> bool {
        let Some((_, _, a, b)) = self.link_at(storage, at) else { return false };
        let text = self.doc.storage_text(storage);
        let (ua, ub) = (byte_to_u16(&text, a), byte_to_u16(&text, b));
        self.doc.touch(h, storage);
        if let Some(st) = self.doc.file.obj_mut(storage) {
            crate::notes::mark(st, "table_smartfield", ua, ub, None);
        }
        self.doc.set_character_style(h, storage, a, b, None);
        self.doc.file.refresh_refs(storage);
        true
    }

    /// Insert > Bookmark over stored bytes a..b (or the word there).
    pub fn add_bookmark(&mut self, h: &mut History, storage: ObjId, a: usize, b: usize) -> bool {
        if a >= b {
            return false;
        }
        let mut f = Msg::new("TSWP.BookmarkFieldArchive");
        f.msg_mut_or_create("super").set_str("text_attribute_uuid_string", &uuid_string());
        f.set_u64("ranged", 1);
        f.set_u64("hidden", 0);
        let Some(fid) = self.new_object(h, storage, f) else { return false };
        let text = self.doc.storage_text(storage);
        let (ua, ub) = (byte_to_u16(&text, a), byte_to_u16(&text, b));
        self.doc.touch(h, storage);
        if let Some(st) = self.doc.file.obj_mut(storage) {
            crate::notes::mark(st, "table_bookmark", ua, ub, Some(fid));
        }
        self.doc.file.refresh_refs(storage);
        true
    }

    /// The body's bookmarks: (field, text, stored range).
    pub fn bookmarks(&self) -> Vec<(ObjId, String, usize, usize)> {
        let Some(b) = self.body_storage() else { return Vec::new() };
        let Some(st) = self.file().obj(b) else { return Vec::new() };
        let text = self.doc.storage_text(b);
        let t = object_table(st, "table_bookmark");
        t.iter()
            .enumerate()
            .filter_map(|(i, (ci, obj))| {
                let id = (*obj)?;
                let end = t.get(i + 1).map(|e| e.0).unwrap_or(usize::MAX);
                let (a, e) = (u16_to_byte(&text, *ci), u16_to_byte(&text, end));
                Some((id, crate::text_plain(&text[a..e]), a, e))
            })
            .collect()
    }

    pub fn remove_bookmark(&mut self, h: &mut History, field: ObjId) {
        let Some(b) = self.body_storage() else { return };
        self.doc.touch(h, b);
        if let Some(st) = self.doc.file.obj_mut(b) {
            let mut t = object_table(st, "table_bookmark");
            for e in &mut t {
                if e.1 == Some(field) {
                    e.1 = None;
                }
            }
            st.remove("table_bookmark");
            let tt = st.msg_mut_or_create("table_bookmark");
            for (ci, o) in t {
                let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
                e.set_u64("character_index", ci as u64);
                if let Some(o) = o {
                    e.set_ref("object", o);
                }
                tt.push_msg("entries", e);
            }
        }
        self.doc.file.refresh_refs(b);
    }

    /// Insert > Date & Time at stored byte `at`: the formatted date as text plus its field.
    pub fn insert_date(&mut self, h: &mut History, storage: ObjId, at: usize) -> usize {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let text = format_date_full(now);
        crate::edit::replace(&mut self.doc, h, storage, at, at, &text);
        let mut f = Msg::new("TSWP.DateTimeSmartFieldArchive");
        f.msg_mut_or_create("super").set_str("text_attribute_uuid_string", &uuid_string());
        f.set_str("format", "EEEE, MMMM d, y");
        f.set_str("locale_identifier", "en_US");
        f.set_enum("date_style", "kDateTimeFormatterStyleFull");
        f.set_enum("time_style", "kDateTimeFormatterStyleNone");
        f.set_enum("update_plan", "kDateTimeUpdatePlanNever");
        f.set_bool("needs_update", false);
        f.msg_mut_or_create("date").set_f64("seconds", now - 978_307_200.0);
        f.set_enum("custom_format", "kDateTimeCustomFormatNone");
        if let Some(fid) = self.new_object(h, storage, f) {
            let all = self.doc.storage_text(storage);
            let (ua, ub) = (byte_to_u16(&all, at), byte_to_u16(&all, at + text.len()));
            self.doc.touch(h, storage);
            if let Some(st) = self.doc.file.obj_mut(storage) {
                crate::notes::mark(st, "table_smartfield", ua, ub, Some(fid));
            }
            self.doc.file.refresh_refs(storage);
        }
        text.len()
    }

    /// Copy object `root` of donor `dk` (and what it owns in its file) into this document's
    /// Document.iwa, styles mapped. Returns the new root.
    fn paste_owned(&mut self, h: &mut History, dk: &KeyFile, root: ObjId) -> Result<ObjId, kn_format::Error> {
        let ex = dk.extract(root)?;
        let skip: Vec<ObjId> = ex.external.iter().copied().filter(|e| matches!(dk.type_name(*e), "TSWP.StorageArchive" | "TP.DocumentArchive")).collect();
        let ext: Vec<ObjId> = ex.external.iter().copied().filter(|e| !skip.contains(e)).collect();
        let map = self.doc.file.map_external(dk, &ext)?;
        let file = self.root_id().and_then(|r| self.file().file_of(r)).unwrap_or("Index/Document.iwa").to_string();
        let body = self.body_storage();
        self.doc.structural(h, &[], |k| crate::objects::paste_extract(k, &ex, &file, &map, body))
    }

    /// Insert > Footnote at stored byte `at` of the body: a footnote mark and the footnote's
    /// own storage (Pages' footnote from the research sample, emptied).
    pub fn insert_footnote(&mut self, h: &mut History, at: usize) -> Result<ObjId, kn_format::Error> {
        let body = self.body_storage().ok_or_else(|| kn_format::Error::Invalid("no body".into()))?;
        let dk = KeyFile::from_zip(DONOR_FOOTNOTE)?;
        let (did, _) = dk.first_of("TSWP.FootnoteReferenceAttachmentArchive").ok_or_else(|| kn_format::Error::Invalid("donor has no footnote".into()))?;
        let att = self.paste_owned(h, &dk, did)?;
        // the footnote's text: just its mark and a space
        if let Some(fs) = self.file().obj(att).and_then(|m| m.ref_id("contained_storage")) {
            let t = self.doc.storage_text(fs);
            if t.len() > 4 {
                crate::edit::replace(&mut self.doc, h, fs, '\u{FFFC}'.len_utf8() + 1, t.len(), "");
            }
        }
        crate::edit::replace(&mut self.doc, h, body, at, at, "\u{E}");
        let text = self.doc.storage_text(body);
        let u = byte_to_u16(&text, at);
        self.doc.touch(h, body);
        if let Some(st) = self.doc.file.obj_mut(body) {
            let mut t = object_table(st, "table_footnote");
            t.retain(|e| e.0 != u);
            t.push((u, Some(att)));
            t.sort_by_key(|e| e.0);
            st.remove("table_footnote");
            let tt = st.msg_mut_or_create("table_footnote");
            for (ci, o) in t {
                let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
                e.set_u64("character_index", ci as u64);
                if let Some(o) = o {
                    e.set_ref("object", o);
                }
                tt.push_msg("entries", e);
            }
        }
        // the mark is superscript like Pages' (the donor's mark style)
        self.doc.file.refresh_refs(body);
        Ok(self.file().obj(att).and_then(|m| m.ref_id("contained_storage")).unwrap_or(att))
    }

    /// Insert > Table of Contents > Document at stored byte `at` (on a paragraph of its own).
    pub fn insert_toc(&mut self, h: &mut History, at: usize) -> Result<(), kn_format::Error> {
        let dk = KeyFile::from_zip(DONOR_TOC)?;
        let (did, _) = dk.first_of("TSWP.TOCAttachmentArchive").ok_or_else(|| kn_format::Error::Invalid("donor has no table of contents".into()))?;
        let att = self.paste_owned(h, &dk, did)?;
        let body = self.body_storage().ok_or_else(|| kn_format::Error::Invalid("no body".into()))?;
        let text = self.doc.storage_text(body);
        let mut at = at;
        if at > 0 && !text[..at].ends_with(|c: char| crate::body::is_para_end(c)) {
            crate::edit::replace(&mut self.doc, h, body, at, at, "\n");
            at += 1;
        }
        crate::edit::insert_attachment(&mut self.doc, h, body, at, att);
        let t2 = self.doc.storage_text(body);
        let after = at + '\u{FFFC}'.len_utf8();
        if !t2[after..].starts_with(|c: char| crate::body::is_para_end(c)) {
            crate::edit::replace(&mut self.doc, h, body, after, after, "\n");
        }
        // the contents paragraph is plain body text, not the heading it was split from
        if let Some(st) = crate::edit::para_style_named(&self.doc, "Body") {
            crate::edit::set_para_style(&mut self.doc, h, body, at, after, st);
        }
        Ok(())
    }

    /// Insert > Section Break at stored byte `at`: U+0004, and a new section (a copy of the
    /// current one with its own header / footer masters) starting after it.
    pub fn insert_section_break(&mut self, h: &mut History, at: usize) -> Result<ObjId, kn_format::Error> {
        let body = self.body_storage().ok_or_else(|| kn_format::Error::Invalid("no body".into()))?;
        let text = self.doc.storage_text(body);
        let u = byte_to_u16(&text, at);
        let secs = self.sections();
        let cur = secs.iter().rev().find(|s| s.start16 <= u).or(secs.first()).map(|s| s.id).ok_or_else(|| kn_format::Error::Invalid("no section".into()))?;
        let snapshot = self.doc.file.clone();
        let new_sec = self.paste_owned(h, &snapshot, cur)?;
        crate::edit::replace(&mut self.doc, h, body, at, at, "\u{4}");
        let t2 = self.doc.storage_text(body);
        let start = byte_to_u16(&t2, at + 1);
        self.doc.touch(h, body);
        if let Some(st) = self.doc.file.obj_mut(body) {
            let mut t = object_table(st, "table_section");
            t.retain(|e| e.0 != start);
            t.push((start, Some(new_sec)));
            t.sort_by_key(|e| e.0);
            st.remove("table_section");
            let tt = st.msg_mut_or_create("table_section");
            for (ci, o) in t {
                let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
                e.set_u64("character_index", ci as u64);
                if let Some(o) = o {
                    e.set_ref("object", o);
                }
                tt.push_msg("entries", e);
            }
        }
        self.doc.file.refresh_refs(body);
        Ok(new_sec)
    }

    /// Document inspector > Section: change a section's switches.
    pub fn set_section(&mut self, h: &mut History, section: ObjId, f: impl FnOnce(&mut Msg)) {
        self.doc.touch(h, section);
        if let Some(m) = self.doc.file.obj_mut(section) {
            f(m);
        }
        if let Some(a) = self.doc.file.archive_mut(section) {
            a.refresh_object_references();
        }
    }

    /// Section background colour (None: no background).
    pub fn set_section_background(&mut self, h: &mut History, section: ObjId, c: Option<kn_model::Color>) {
        self.set_section(h, section, |m| {
            m.remove("background_fill");
            if let Some(c) = c {
                m.msg_mut_or_create("background_fill").push_msg("color", kn_model::format::color_msg(c));
            }
        });
    }

    /// Section background of any kind (FillSpec::None: no background).
    pub fn set_section_fill(&mut self, h: &mut History, section: ObjId, f: &kn_model::format::FillSpec) {
        let f = (*f != kn_model::format::FillSpec::None).then_some(f);
        self.doc.set_fill_field(h, section, "background_fill", f);
    }

    /// Layout > Columns of the body (count, gap as a fraction of the text width).
    pub fn set_columns(&mut self, h: &mut History, count: u32, gap: f32) -> bool {
        let Some(body) = self.body_storage() else { return false };
        let Some(base) = self.file().obj(body).and_then(|st| st.msg("table_layout_style")).and_then(|t| t.msgs("entries").find_map(|e| e.ref_id("object"))) else { return false };
        let sheet = self.doc.file.stylesheet_id();
        let touch: Vec<ObjId> = sheet.into_iter().collect();
        let Ok(v) = self.doc.structural(h, &touch, |k| k.new_style_variation(base)) else { return false };
        self.doc.touch(h, v);
        if let Some(m) = self.doc.file.obj_mut(v) {
            let c = m.msg_mut_or_create("column_properties").msg_mut_or_create("columns");
            c.remove("equal_columns");
            let e = c.msg_mut_or_create("equal_columns");
            e.set_u64("count", u64::from(count.clamp(1, 10)));
            e.set_f32("gap", gap.clamp(0.0, 0.5));
            kn_format::objects::fix_override_count(m);
        }
        self.doc.touch(h, body);
        if let Some(st) = self.doc.file.obj_mut(body) {
            if let Some(t) = st.msg_mut("table_layout_style") {
                for e in t.msgs_mut("entries") {
                    if e.ref_id("object").is_some() {
                        e.set_ref("object", v);
                    }
                }
            }
        }
        self.doc.file.refresh_refs(body);
        true
    }

    /// The body's columns: (count, gap fraction).
    pub fn columns(&self) -> (u32, f32) {
        let k = self.file();
        let style = self.body_storage().and_then(|b| k.obj(b)).and_then(|st| st.msg("table_layout_style")).and_then(|t| t.msgs("entries").find_map(|e| e.ref_id("object")));
        let ch = kn_model::style::chain(k, style);
        kn_model::style::lookup(&ch, |m| m.msg("column_properties").and_then(|p| p.msg("columns")).and_then(|c| c.msg("equal_columns")).map(|e| (e.u32("count").unwrap_or(1), e.f32("gap").unwrap_or(0.05)))).unwrap_or((1, 0.05))
    }

    /// File > Convert to Page Layout: the body's text goes into a text box on each page it was
    /// on; the document then has free pages (TP.SettingsArchive.body = false + a page template).
    pub fn convert_to_page_layout(&mut self, h: &mut History, page_texts: &[(usize, String)]) -> Result<(), kn_format::Error> {
        let setup = self.setup();
        for (page, text) in page_texts {
            if text.trim().is_empty() {
                continue;
            }
            let id = self.insert(h, crate::objects::NewObject::TextBox, crate::objects::Placement::Floating { page: *page, x: setup.left, y: setup.top })?;
            let mut g = self.file().obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
            g.w = setup.text_width();
            g.h = setup.text_height();
            self.doc.set_frame(h, id, g);
            if let Some(st) = self.doc.storage_of(id) {
                crate::edit::replace(&mut self.doc, h, st, 0, 0, text);
            }
            self.set_wrap(h, id, 5);
        }
        if let Some(body) = self.body_storage() {
            let t = self.doc.storage_text(body);
            if !t.is_empty() {
                crate::edit::replace(&mut self.doc, h, body, 0, t.len(), "");
            }
        }
        if self.page_templates().is_empty() {
            let root = self.root_id().ok_or_else(|| kn_format::Error::Invalid("no document".into()))?;
            let guide = self.new_object(h, root, Msg::new("TSD.GuideStorageArchive")).ok_or_else(|| kn_format::Error::Invalid("no document file".into()))?;
            let mut t = Msg::new("TP.PageTemplateArchive");
            t.set_str("name", "Blank");
            t.set_bool("headers_footers_match_previous_page", true);
            t.set_bool("hide_headers_footers", false);
            t.set_ref("guide_storage", guide);
            let tid = self.new_object(h, root, t).ok_or_else(|| kn_format::Error::Invalid("no document file".into()))?;
            self.doc.touch(h, root);
            if let Some(a) = self.doc.file.archive_mut(root) {
                a.obj_mut().push_ref("page_templates", tid);
                a.refresh_object_references();
            }
        }
        let n = page_texts.iter().map(|p| p.0 + 1).max().unwrap_or(1).max(self.saved_page_count());
        let mut s = setup;
        s.body = false;
        self.set_setup(h, &s);
        self.set_saved_page_count(h, n);
        Ok(())
    }

    /// File > Convert to Word Processing: the document gets its flowing body back (objects
    /// stay on their pages).
    pub fn convert_to_word_processing(&mut self, h: &mut History) {
        let mut s = self.setup();
        s.body = true;
        self.set_setup(h, &s);
    }
}

/// "Monday, October 19, 2026" from Unix seconds (UTC date).
pub fn format_date_full(unix: f64) -> String {
    let days = (unix / 86400.0).floor() as i64;
    // civil from days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let weekday = (days + 4).rem_euclid(7) as usize; // 1970-01-01 was a Thursday
    const WD: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    const MO: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    format!("{}, {} {}, {}", WD[weekday], MO[(m - 1) as usize], d, y)
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        // 2026-10-19 12:00 UTC
        assert_eq!(super::format_date_full(1_792_411_200.0), "Monday, October 19, 2026");
        assert_eq!(super::format_date_full(0.0), "Thursday, January 1, 1970");
    }
}
