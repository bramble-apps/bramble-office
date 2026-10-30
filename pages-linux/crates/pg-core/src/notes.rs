//! Comments, highlights and tracked changes in Pages text (research samples 20, 21, 31).
//!
//! A comment is a TSWP.HighlightArchive over a text range (table_highlight) pointing at a
//! TSD.CommentStorageArchive (text, date, author). Tracked changes: TP.DocumentArchive
//! change_tracking_enabled + change sessions (TSWP.ChangeSessionArchive: author, date); each
//! insertion / deletion is a TSWP.ChangeArchive referenced from table_insertion / table_deletion.
//! Deleted text stays in the storage until the deletion is accepted.

use kn_format::{Archive, Msg, ObjId};
use kn_model::edit::History;
use kn_model::textedit::byte_to_u16;

use crate::edit::object_table;
use crate::Pages;

/// A comment or highlight on text.
#[derive(Debug, Clone, PartialEq)]
pub struct TextComment {
    pub highlight: ObjId,
    pub storage_obj: Option<ObjId>,
    pub text: String,
    pub author: String,
    pub color: kn_model::Color,
    pub date: Option<f64>,
    /// The commented text's stored byte range.
    pub start: usize,
    pub end: usize,
}

/// A tracked change.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub change: ObjId,
    pub deletion: bool,
    pub author: String,
    pub color: kn_model::Color,
    pub date: Option<f64>,
    pub start: usize,
    pub end: usize,
    pub text: String,
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

fn write_table(st: &mut Msg, name: &str, entries: &[(usize, Option<ObjId>)]) {
    st.remove(name);
    let t = st.msg_mut_or_create(name);
    for (ci, obj) in entries {
        let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
        e.set_u64("character_index", *ci as u64);
        if let Some(o) = obj {
            e.set_ref("object", *o);
        }
        t.push_msg("entries", e);
    }
}

/// Set `obj` over UTF-16 range a..b of a char-indexed table, keeping what was there around it.
pub fn mark(st: &mut Msg, name: &str, a: usize, b: usize, obj: Option<ObjId>) {
    let mut entries = object_table(st, name);
    let after = entries.iter().filter(|e| e.0 <= b).last().and_then(|e| e.1);
    entries.retain(|e| e.0 < a || e.0 > b);
    entries.push((a, obj));
    entries.push((b, after));
    if !entries.iter().any(|e| e.0 == 0) {
        entries.push((0, None));
    }
    entries.sort_by_key(|e| e.0);
    entries.dedup_by(|x, y| x.0 == y.0);
    // neighbours with the same object merge
    let mut out: Vec<(usize, Option<ObjId>)> = Vec::new();
    for e in entries {
        if out.last().is_some_and(|l| l.1 == e.1) {
            continue;
        }
        out.push(e);
    }
    write_table(st, name, &out);
}

fn author_of(p: &Pages, author: Option<ObjId>) -> (String, kn_model::Color) {
    let a = author.and_then(|a| p.file().obj(a));
    (
        a.and_then(|a| a.str("name")).unwrap_or("").to_string(),
        a.and_then(|a| a.msg("color")).map(kn_model::style::color).unwrap_or(kn_model::Color { r: 0.98, g: 0.86, b: 0.25, a: 1.0 }),
    )
}

impl Pages {
    /// The comments and highlights of a storage, in text order.
    pub fn text_comments(&self, storage: ObjId) -> Vec<TextComment> {
        let k = self.file();
        let Some(st) = k.obj(storage) else { return Vec::new() };
        let text = self.doc.storage_text(storage);
        let t = object_table(st, "table_highlight");
        let mut out = Vec::new();
        for (i, (ci, obj)) in t.iter().enumerate() {
            let Some(hid) = *obj else { continue };
            let Some(hl) = k.obj(hid) else { continue };
            let end16 = t.get(i + 1).map(|e| e.0).unwrap_or(usize::MAX);
            let cs = hl.ref_id("commentStorage");
            let c = cs.and_then(|c| k.obj(c));
            let (author, color) = author_of(self, c.and_then(|c| c.ref_id("author")));
            out.push(TextComment {
                highlight: hid,
                storage_obj: cs,
                text: c.and_then(|c| c.str("text")).unwrap_or("").to_string(),
                author,
                color,
                date: c.and_then(|c| c.msg("creation_date")).and_then(|d| d.f64("seconds")),
                start: u16_to_byte(&text, *ci),
                end: u16_to_byte(&text, end16),
            });
        }
        out
    }

    /// Insert > Comment / Highlight over stored bytes a..b (text "" = a highlight).
    pub fn add_text_comment(&mut self, h: &mut History, storage: ObjId, a: usize, b: usize, text: &str, author: &str) -> Option<ObjId> {
        let hid = self.doc.add_highlight(h, storage, a, b, author)?;
        if !text.is_empty() {
            if let Some(c) = self.file().obj(hid).and_then(|m| m.ref_id("commentStorage")) {
                self.doc.set_comment_text(h, c, text);
            }
        }
        Some(hid)
    }

    pub fn set_text_comment(&mut self, h: &mut History, highlight: ObjId, text: &str) {
        if let Some(c) = self.file().obj(highlight).and_then(|m| m.ref_id("commentStorage")) {
            self.doc.set_comment_text(h, c, text);
        }
    }

    /// Delete a comment / highlight (its range in table_highlight goes back to none).
    pub fn delete_text_comment(&mut self, h: &mut History, storage: ObjId, highlight: ObjId) {
        self.doc.touch(h, storage);
        let Some(st) = self.doc.file.obj_mut(storage) else { return };
        let mut t = object_table(st, "table_highlight");
        for e in &mut t {
            if e.1 == Some(highlight) {
                e.1 = None;
            }
        }
        let mut out: Vec<(usize, Option<ObjId>)> = Vec::new();
        for e in t {
            if out.last().is_some_and(|l: &(usize, Option<ObjId>)| l.1 == e.1) {
                continue;
            }
            out.push(e);
        }
        write_table(st, "table_highlight", &out);
        self.doc.file.refresh_refs(storage);
    }

    /// Edit > Remove Highlights and Comments.
    pub fn remove_all_comments(&mut self, h: &mut History, storage: ObjId) {
        self.doc.touch(h, storage);
        if let Some(st) = self.doc.file.obj_mut(storage) {
            st.remove("table_highlight");
        }
        self.doc.file.refresh_refs(storage);
    }

    /// Author names and colours (TSK.AnnotationAuthorArchive).
    pub fn authors(&self) -> Vec<(ObjId, String, kn_model::Color)> {
        self.file()
            .objects_of("TSK.AnnotationAuthorArchive")
            .map(|(id, m)| (id, m.str("name").unwrap_or("").to_string(), m.msg("color").map(kn_model::style::color).unwrap_or(kn_model::Color { r: 0.98, g: 0.86, b: 0.25, a: 1.0 })))
            .collect()
    }

    pub fn tracking(&self) -> bool {
        self.root().and_then(|r| r.1.bool("change_tracking_enabled")).unwrap_or(false)
    }

    /// Edit > Track Changes on / off.
    pub fn set_tracking(&mut self, h: &mut History, on: bool) {
        let Some(root) = self.root_id() else { return };
        self.doc.touch(h, root);
        if let Some(m) = self.doc.file.obj_mut(root) {
            m.set_bool("change_tracking_enabled", on);
        }
    }

    /// The current change session of `author` (a new one per author, as Pages starts one per
    /// editing session).
    fn session(&mut self, h: &mut History, author: &str) -> Option<ObjId> {
        let root = self.root_id()?;
        let file = self.file().file_of(root)?.to_string();
        if let Some(s) = self.root()?.1.ref_id("most_recent_change_session") {
            let a = self.file().obj(s).and_then(|m| m.ref_id("author")).and_then(|a| self.file().obj(a)).and_then(|a| a.str("name")).map(str::to_string);
            if a.as_deref() == Some(author) {
                return Some(s);
            }
        }
        self.doc.touch_all(h);
        let author_id = self
            .file()
            .objects_of("TSK.AnnotationAuthorArchive")
            .find(|(_, m)| m.str("name") == Some(author))
            .map(|(id, _)| id);
        let author_id = match author_id {
            Some(a) => a,
            None => new_author(self, author)?,
        };
        let n = self.root()?.1.ref_ids("change_sessions").len() as u64 + 1;
        let id = self.doc.file.alloc_ids(1)[0];
        let mut s = Msg::new("TSWP.ChangeSessionArchive");
        s.set_u64("session_uid", n);
        s.set_ref("author", author_id);
        s.push_msg("date", now_date());
        let mut a = Archive::new_object(id, s);
        a.refresh_object_references();
        self.doc.file.insert_archive(&file, a);
        if let Some(r) = self.doc.file.archive_mut(root) {
            r.obj_mut().push_ref("change_sessions", id);
            r.obj_mut().set_ref("most_recent_change_session", id);
            r.refresh_object_references();
        }
        self.doc.file.ensure_external_refs(&file);
        Some(id)
    }

    /// A new TSWP.ChangeArchive (insertion / deletion) of the current session.
    fn new_change(&mut self, h: &mut History, deletion: bool, author: &str) -> Option<ObjId> {
        let session = self.session(h, author)?;
        let file = self.file().file_of(self.root_id()?)?.to_string();
        self.doc.touch_all(h);
        let id = self.doc.file.alloc_ids(1)[0];
        let mut c = Msg::new("TSWP.ChangeArchive");
        c.set_enum("kind", if deletion { "kChangeKindDeletion" } else { "kChangeKindInsertion" });
        c.set_ref("session", session);
        c.push_msg("date", now_date());
        c.set_str("text_attribute_uuid_string", &uuid_string());
        let mut a = Archive::new_object(id, c);
        a.refresh_object_references();
        self.doc.file.insert_archive(&file, a);
        Some(id)
    }

    /// Typing with Track Changes on: `insert` at stored byte `at` is marked as an insertion.
    pub fn tracked_insert(&mut self, h: &mut History, storage: ObjId, at: usize, insert: &str, author: &str) {
        // continuing an insertion of the same author: the change before the caret goes on
        let text = self.doc.storage_text(storage);
        let u = byte_to_u16(&text, at);
        let prev = self.file().obj(storage).map(|st| object_table(st, "table_insertion")).and_then(|t| t.iter().filter(|e| e.0 < u.max(1)).last().and_then(|e| e.1));
        let prev_ok = prev.filter(|c| self.change_author(*c).as_deref() == Some(author) && at > 0);
        crate::edit::replace(&mut self.doc, h, storage, at, at, insert);
        let change = match prev_ok {
            Some(c) => Some(c),
            None => self.new_change(h, false, author),
        };
        let Some(change) = change else { return };
        let n16 = insert.encode_utf16().count();
        self.doc.touch(h, storage);
        if let Some(st) = self.doc.file.obj_mut(storage) {
            mark(st, "table_insertion", u, u + n16, Some(change));
        }
        self.doc.file.refresh_refs(storage);
    }

    /// Deleting with Track Changes on: stored bytes a..b are marked as deleted (text inserted
    /// by the same author in this session is really removed).
    pub fn tracked_delete(&mut self, h: &mut History, storage: ObjId, a: usize, b: usize, author: &str) {
        if a >= b {
            return;
        }
        let text = self.doc.storage_text(storage);
        let (ua, ub) = (byte_to_u16(&text, a), byte_to_u16(&text, b));
        // own fresh insertions vanish outright
        let ins = self.file().obj(storage).map(|st| object_table(st, "table_insertion")).unwrap_or_default();
        let own = |p: &Pages, u: usize| ins.iter().filter(|e| e.0 <= u).last().and_then(|e| e.1).is_some_and(|c| p.change_author(c).as_deref() == Some(author));
        if own(self, ua) && own(self, ub.saturating_sub(1)) && ins.iter().all(|e| e.0 <= ua || e.0 >= ub) {
            crate::edit::replace(&mut self.doc, h, storage, a, b, "");
            return;
        }
        let Some(change) = self.new_change(h, true, author) else { return };
        self.doc.touch(h, storage);
        if let Some(st) = self.doc.file.obj_mut(storage) {
            mark(st, "table_deletion", ua, ub, Some(change));
        }
        self.doc.file.refresh_refs(storage);
    }

    fn change_author(&self, change: ObjId) -> Option<String> {
        let k = self.file();
        k.obj(change)?.ref_id("session").and_then(|s| k.obj(s)).and_then(|s| s.ref_id("author")).and_then(|a| k.obj(a)).and_then(|a| a.str("name")).map(str::to_string)
    }

    /// The tracked changes of a storage, in text order.
    pub fn changes(&self, storage: ObjId) -> Vec<Change> {
        let k = self.file();
        let Some(st) = k.obj(storage) else { return Vec::new() };
        let text = self.doc.storage_text(storage);
        let mut out = Vec::new();
        for (name, deletion) in [("table_insertion", false), ("table_deletion", true)] {
            let t = object_table(st, name);
            for (i, (ci, obj)) in t.iter().enumerate() {
                let Some(cid) = *obj else { continue };
                let Some(c) = k.obj(cid) else { continue };
                let end16 = t.get(i + 1).map(|e| e.0).unwrap_or(usize::MAX);
                let session = c.ref_id("session").and_then(|s| k.obj(s));
                let (author, color) = author_of(self, session.and_then(|s| s.ref_id("author")));
                let (s, e) = (u16_to_byte(&text, *ci), u16_to_byte(&text, end16));
                out.push(Change { change: cid, deletion, author, color, date: c.msg("date").and_then(|d| d.f64("seconds")), start: s, end: e, text: text[s..e].to_string() });
            }
        }
        out.sort_by_key(|c| c.start);
        out
    }

    /// Accept (true) or reject a change.
    pub fn resolve_change(&mut self, h: &mut History, storage: ObjId, change: &Change, accept: bool) {
        let remove_text = change.deletion == accept;
        let table = if change.deletion { "table_deletion" } else { "table_insertion" };
        let text = self.doc.storage_text(storage);
        let (ua, ub) = (byte_to_u16(&text, change.start), byte_to_u16(&text, change.end));
        // the mark goes first, then the text if it was deleted (accepted deletion / rejected insertion)
        self.doc.touch(h, storage);
        if let Some(st) = self.doc.file.obj_mut(storage) {
            mark(st, table, ua, ub, None);
            if st.msg(table).is_some_and(|t| t.msgs("entries").all(|e| e.ref_id("object").is_none())) {
                st.remove(table);
            }
        }
        if remove_text {
            crate::edit::replace(&mut self.doc, h, storage, change.start, change.end, "");
        }
        self.doc.file.refresh_refs(storage);
    }

    /// Accept or reject every change of a storage (from the end so offsets stay valid).
    pub fn resolve_all(&mut self, h: &mut History, storage: ObjId, accept: bool) -> usize {
        let mut n = 0;
        loop {
            let cs = self.changes(storage);
            let Some(c) = cs.last() else { break };
            self.resolve_change(h, storage, c, accept);
            n += 1;
            if n > 10_000 {
                break;
            }
        }
        n
    }
}

fn now_date() -> Msg {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let mut date = Msg::new("TSP.Date");
    date.set_f64("seconds", now - 978_307_200.0);
    date
}

fn uuid_string() -> String {
    let mut tmp = Msg::new("TSP.UUID");
    kn_format::objects::new_uuid(&mut tmp);
    let (lo, hi) = (tmp.u64("lower").unwrap_or(0), tmp.u64("upper").unwrap_or(0));
    let s = format!("{hi:016X}{lo:016X}");
    format!("{}-{}-{}-{}-{}", &s[0..8], &s[8..12], &s[12..16], &s[16..20], &s[20..32])
}

/// A new comment author (Pages' yellow-green for the first, then the View > Author Color list).
fn new_author(p: &mut Pages, name: &str) -> Option<ObjId> {
    let (store, _) = p.file().first_of("TSK.AnnotationAuthorStorageArchive")?;
    let file = p.file().file_of(store)?.to_string();
    let palette = [(0.75, 0.94, 0.30), (0.40, 0.70, 0.95), (0.95, 0.55, 0.75), (0.70, 0.55, 0.95), (0.35, 0.80, 0.75), (1.0, 0.65, 0.25)];
    let n = p.authors().len();
    let (r, g, b) = palette[n % palette.len()];
    let id = p.doc.file.alloc_ids(1)[0];
    let mut a = Msg::new("TSK.AnnotationAuthorArchive");
    a.set_str("name", name);
    a.push_msg("color", kn_model::format::color_msg(kn_model::Color { r, g, b, a: 1.0 }));
    a.set_str("public_id", "00000000-0000-0000-0000-000000000000:0");
    a.set_bool("is_public_author", false);
    let mut arch = Archive::new_object(id, a);
    arch.refresh_object_references();
    p.doc.file.insert_archive(&file, arch);
    if let Some(s) = p.doc.file.archive_mut(store) {
        s.obj_mut().push_ref("annotation_author", id);
        s.refresh_object_references();
    }
    p.doc.file.ensure_external_refs(&file);
    Some(id)
}
