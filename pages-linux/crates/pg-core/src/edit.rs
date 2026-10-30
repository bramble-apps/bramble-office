//! Editing Pages text storages: typing and deleting (with Pages' break characters and the
//! paragraph-, section- and footnote-indexed tables kept in step), paragraph formatting of
//! the touched paragraphs (style, list, level, any paragraph property) and character
//! formatting with any character property.
//!
//! Positions are byte offsets in the *stored* text (see [`crate::body`]).

use kn_format::{Msg, ObjId};
use kn_model::edit::History;
use kn_model::textedit::byte_to_u16;
use kn_model::Document;

use crate::body::is_para_end;

/// Paragraph-indexed tables kn-model's `replace_text` doesn't know (Pages-only), and the
/// character-indexed ones.
const PAGES_PARA_TABLES: &[&str] = &["table_layout_style", "table_section"];
const PAGES_CHAR_TABLES: &[&str] = &["table_footnote"];

/// UTF-16 indices where paragraphs start (0 and after each paragraph end).
pub fn para_starts16(text: &str) -> Vec<usize> {
    let mut v = vec![0usize];
    let mut u = 0usize;
    for ch in text.chars() {
        u += ch.len_utf16();
        if is_para_end(ch) {
            v.push(u);
        }
    }
    v
}

/// Byte ranges of the paragraphs (without their end character).
pub fn para_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    let mut s = 0;
    for (b, ch) in text.char_indices() {
        if is_para_end(ch) {
            v.push((s, b));
            s = b + ch.len_utf8();
        }
    }
    v.push((s, text.len()));
    v
}

fn entries_of(st: &Msg, name: &str) -> Vec<Msg> {
    st.msg(name).map(|t| t.msgs("entries").cloned().collect()).unwrap_or_default()
}

/// Insert / replace text, keeping formatting (kn-model's replace_text) plus what Pages needs:
/// break characters other than '\n' start paragraphs too, and the section / layout / footnote
/// tables shift with the text. Call between `begin` and `end`.
pub fn replace(doc: &mut Document, h: &mut History, storage: ObjId, start: usize, end: usize, insert: &str) {
    let old = doc.storage_text(storage);
    let (start, end) = (start.min(old.len()), end.min(old.len()).max(start.min(old.len())));
    if !old.is_char_boundary(start) || !old.is_char_boundary(end) {
        return;
    }
    let s16 = byte_to_u16(&old, start);
    let e16 = byte_to_u16(&old, end);
    let ins16 = insert.encode_utf16().count();
    let delta = ins16 as i64 - (e16 - s16) as i64;
    // kn-model makes paragraph entries only for '\n': write other ends as '\n' first, then
    // swap the characters back in (the tables are the same either way)
    let as_newlines: String = insert.chars().map(|c| if is_para_end(c) { '\n' } else { c }).collect();
    let saved: Vec<(&str, Vec<Msg>)> = doc
        .file
        .obj(storage)
        .map(|st| PAGES_PARA_TABLES.iter().chain(PAGES_CHAR_TABLES).map(|n| (*n, entries_of(st, n))).collect())
        .unwrap_or_default();
    doc.replace_text(h, storage, start, end, &as_newlines);
    if as_newlines != insert {
        if let Some(st) = doc.file.obj_mut(storage) {
            let t: String = st.strs("text").concat();
            let fixed = format!("{}{}{}", &t[..start], insert, &t[start + as_newlines.len()..]);
            st.set_strs("text", &[&fixed]);
        }
    }
    if let Some(st) = doc.file.obj_mut(storage) {
        for (name, entries) in saved {
            if entries.is_empty() {
                continue;
            }
            let is_para = PAGES_PARA_TABLES.contains(&name);
            let mut out: Vec<Msg> = Vec::new();
            for mut e in entries {
                let ci = e.u64("character_index").unwrap_or(0) as usize;
                let nci = if ci < s16 || (ci == s16 && is_para) {
                    ci
                } else if ci < e16 || (is_para && ci == e16 && e16 > s16) {
                    // inside the deleted text: a footnote goes with its mark, a section with its break
                    continue;
                } else {
                    (ci as i64 + delta).max(0) as usize
                };
                e.set_u64("character_index", nci as u64);
                out.push(e);
            }
            out.sort_by_key(|e| e.u64("character_index").unwrap_or(0));
            out.dedup_by(|a, b| a.u64("character_index") == b.u64("character_index"));
            if let Some(t) = st.msg_mut(name) {
                t.remove("entries");
                for e in out {
                    t.push_msg("entries", e);
                }
            }
        }
    }
    doc.file.refresh_refs(storage);
}

/// The character style variation base: the stylesheet's "None" character style.
fn null_char_style(doc: &Document) -> Option<ObjId> {
    doc.file
        .objects_of("TSWP.CharacterStyleArchive")
        .find(|(_, m)| m.find_super("TSS.StyleArchive").and_then(|s| s.str("style_identifier")) == Some("character-style-null"))
        .map(|(id, _)| id)
        .or_else(|| {
            doc.file
                .objects_of("TSWP.CharacterStyleArchive")
                .find(|(_, m)| m.find_super("TSS.StyleArchive").is_some_and(|s| s.bool("is_variation") != Some(true) && s.str("name").is_none()))
                .map(|(id, _)| id)
        })
}

fn write_table(st: &mut Msg, name: &str, entries: &[(usize, Option<ObjId>)]) {
    let t = st.msg_mut_or_create(name);
    t.remove("entries");
    for (ci, obj) in entries {
        let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
        e.set_u64("character_index", *ci as u64);
        if let Some(o) = obj {
            e.set_ref("object", *o);
        }
        t.push_msg("entries", e);
    }
}

pub(crate) fn object_table(st: &Msg, name: &str) -> Vec<(usize, Option<ObjId>)> {
    let mut v: Vec<_> = st.msg(name).map(|t| t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0) as usize, e.ref_id("object"))).collect()).unwrap_or_default();
    v.sort_by_key(|e| e.0);
    v
}

fn at(t: &[(usize, Option<ObjId>)], i: usize) -> Option<ObjId> {
    t.iter().take_while(|e| e.0 <= i).last().and_then(|e| e.1)
}

/// A new variation of `base` in the stylesheet, changed by `f` (gets the whole style message).
fn variation(doc: &mut Document, h: &mut History, base: ObjId, f: &dyn Fn(&mut Msg)) -> Option<ObjId> {
    let sheet = doc.file.stylesheet_id();
    let touch: Vec<ObjId> = sheet.into_iter().collect();
    let v = doc.structural(h, &touch, |k| k.new_style_variation(base)).ok()?;
    doc.touch(h, v);
    if let Some(m) = doc.file.obj_mut(v) {
        f(m);
        kn_format::objects::fix_override_count(m);
    }
    Some(v)
}

fn finish(doc: &mut Document, storage: ObjId) {
    doc.file.refresh_refs(storage);
    // (only the storage changed: its references, not the whole file's)
    if let Some(f) = doc.file.file_of(storage).map(str::to_string) {
        doc.file.ensure_external_refs_of(&f, &[storage]);
    }
}

/// Change character properties of bytes `start..end`: each run gets a variation of its
/// character style (or of "None") changed by `f` (gets the `char_properties` message).
pub fn set_char_props(doc: &mut Document, h: &mut History, storage: ObjId, start: usize, end: usize, f: &dyn Fn(&mut Msg)) -> bool {
    let text = doc.storage_text(storage);
    let (s16, e16) = (byte_to_u16(&text, start.min(text.len())), byte_to_u16(&text, end.min(text.len())));
    if s16 >= e16 {
        return false;
    }
    let Some(st) = doc.file.obj(storage) else { return false };
    let table = object_table(st, "table_char_style");
    let mut cuts: Vec<usize> = vec![s16];
    cuts.extend(table.iter().map(|e| e.0).filter(|c| *c > s16 && *c < e16));
    let segments: Vec<(usize, Option<ObjId>)> = cuts.iter().map(|c| (*c, at(&table, *c))).collect();
    let after = at(&table, e16);
    let null = null_char_style(doc);
    let mut map: Vec<(Option<ObjId>, ObjId)> = Vec::new();
    for (_, cur) in &segments {
        if map.iter().any(|(k, _)| k == cur) {
            continue;
        }
        let Some(base) = cur.or(null) else { return false };
        let Some(v) = variation(doc, h, base, &|m| f(m.msg_mut_or_create("char_properties"))) else { return false };
        map.push((*cur, v));
    }
    let mut new: Vec<(usize, Option<ObjId>)> = table.iter().copied().filter(|e| e.0 < s16 || e.0 > e16).collect();
    for (c, cur) in &segments {
        new.push((*c, map.iter().find(|(k, _)| k == cur).map(|(_, v)| *v)));
    }
    let total16 = text.encode_utf16().count();
    if e16 < total16 || table.iter().any(|e| e.0 == e16) {
        new.push((e16, after));
    }
    new.sort_by_key(|e| e.0);
    new.dedup_by_key(|e| e.0);
    if !new.iter().any(|e| e.0 == 0) {
        new.insert(0, (0, None));
    }
    doc.touch(h, storage);
    if let Some(st) = doc.file.obj_mut(storage) {
        write_table(st, "table_char_style", &new);
    }
    finish(doc, storage);
    true
}

/// The paragraph starts (UTF-16) touched by bytes `start..end` (the caret's paragraph when
/// start == end), and the start of the paragraph after them.
fn touched_paras(text: &str, start: usize, end: usize) -> (Vec<usize>, Option<usize>) {
    let starts = para_starts16(text);
    let (s16, e16) = (byte_to_u16(text, start.min(text.len())), byte_to_u16(text, end.min(text.len())));
    let next = |p: usize| starts.iter().copied().find(|q| *q > p);
    let touched: Vec<usize> = starts.iter().copied().filter(|&p| p <= e16.max(s16) && next(p).map(|n| n > s16).unwrap_or(true)).collect();
    let after = touched.last().and_then(|l| next(*l));
    (touched, after)
}

/// Point the touched paragraphs' entries of paragraph table `name` at `pick(current)`.
fn set_para_table(doc: &mut Document, h: &mut History, storage: ObjId, start: usize, end: usize, name: &str, pick: &mut dyn FnMut(&mut Document, &mut History, Option<ObjId>) -> Option<ObjId>) -> bool {
    let text = doc.storage_text(storage);
    let (touched, after) = touched_paras(&text, start, end);
    if touched.is_empty() {
        return false;
    }
    let Some(st) = doc.file.obj(storage) else { return false };
    let mut table = object_table(st, name);
    // drop caps are per paragraph: an entry is its own paragraph's and an empty one means none
    // (the other tables carry a style on until the next entry)
    let exact = name == "table_drop_cap_style";
    if !exact {
        let mut last = None;
        for e in &mut table {
            if e.1.is_some() {
                last = e.1;
            } else {
                e.1 = last;
            }
        }
    }
    let mut cuts = touched.clone();
    cuts.extend(after);
    for c in cuts {
        if !table.iter().any(|e| e.0 == c) {
            let s = if exact { None } else { at(&table, c) };
            table.push((c, s));
            table.sort_by_key(|e| e.0);
        }
    }
    let mut made: Vec<(Option<ObjId>, Option<ObjId>)> = Vec::new();
    for p in &touched {
        let Some(i) = table.iter().position(|e| e.0 == *p) else { continue };
        let cur = table[i].1;
        let new = match made.iter().find(|m| m.0 == cur) {
            Some(m) => m.1,
            None => {
                let n = pick(doc, h, cur);
                made.push((cur, n));
                n
            }
        };
        table[i].1 = new;
    }
    doc.touch(h, storage);
    if let Some(st) = doc.file.obj_mut(storage) {
        write_table(st, name, &table);
    }
    finish(doc, storage);
    true
}

/// Give the touched paragraphs paragraph style `style` (their overrides go, as when a style is
/// picked in the paragraph style popover).
pub fn set_para_style(doc: &mut Document, h: &mut History, storage: ObjId, start: usize, end: usize, style: ObjId) -> bool {
    let list = doc.file.obj(style).and_then(|m| m.msg("para_properties")).and_then(|p| p.ref_id("list_style"));
    let ok = set_para_table(doc, h, storage, start, end, "table_para_style", &mut |_, _, _| Some(style));
    if ok {
        if let Some(l) = list {
            set_para_table(doc, h, storage, start, end, "table_list_style", &mut |_, _, _| Some(l));
        }
    }
    ok
}

/// Change paragraph properties of the touched paragraphs: each gets a variation of its style
/// changed by `f` (gets the `para_properties` message).
/// A solid paragraph border (TSD.StrokeArchive as Pages writes one).
pub fn para_stroke_msg(s: &kn_model::format::StrokeSpec) -> Msg {
    let mut m = Msg::new("TSD.StrokeArchive");
    m.push_msg("color", kn_model::format::color_msg(s.color));
    m.set_f32("width", s.width);
    m.set_enum("cap", "ButtCap");
    m.set_enum("join", "MiterJoin");
    m.set_f32("miter_limit", 4.0);
    let p = m.msg_mut_or_create("pattern");
    p.set_enum("type", "TSDSolidPattern");
    p.set_f32("phase", 0.0);
    p.set_u64("count", 0);
    let num = p.field_def("pattern").unwrap().num;
    for _ in 0..6 {
        p.fields.push(kn_format::msg::Field { num, val: kn_format::Val::I32(0f32.to_bits()) });
    }
    m
}

pub fn set_para_props(doc: &mut Document, h: &mut History, storage: ObjId, start: usize, end: usize, f: &dyn Fn(&mut Msg)) -> bool {
    set_para_table(doc, h, storage, start, end, "table_para_style", &mut |doc, h, cur| {
        let base = cur?;
        variation(doc, h, base, &|m| f(m.msg_mut_or_create("para_properties")))
    })
}

/// Change the paragraph style's character properties of the touched paragraphs.
pub fn set_para_char_props(doc: &mut Document, h: &mut History, storage: ObjId, start: usize, end: usize, f: &dyn Fn(&mut Msg)) -> bool {
    set_para_table(doc, h, storage, start, end, "table_para_style", &mut |doc, h, cur| {
        let base = cur?;
        variation(doc, h, base, &|m| f(m.msg_mut_or_create("char_properties")))
    })
}

/// List style for the touched paragraphs (None: the stylesheet's "None" list style).
pub fn set_list_style(doc: &mut Document, h: &mut History, storage: ObjId, start: usize, end: usize, list: Option<ObjId>) -> bool {
    let none = list_styles(doc).into_iter().find(|(_, n)| n == "None").map(|(id, _)| id);
    let target = list.or(none);
    set_para_table(doc, h, storage, start, end, "table_list_style", &mut |_, _, _| target)
}

/// Drop cap style for the touched paragraphs (None: no drop cap).
pub fn set_drop_cap(doc: &mut Document, h: &mut History, storage: ObjId, start: usize, end: usize, style: Option<ObjId>) -> bool {
    set_para_table(doc, h, storage, start, end, "table_drop_cap_style", &mut |_, _, _| style)
}

/// Named list styles (Bullets & Lists popover): (id, name), in stylesheet order.
pub fn list_styles(doc: &Document) -> Vec<(ObjId, String)> {
    let mut v: Vec<(ObjId, String)> = doc
        .file
        .objects_of("TSWP.ListStyleArchive")
        .filter_map(|(id, m)| {
            let s = m.find_super("TSS.StyleArchive")?;
            if s.bool("is_variation") == Some(true) {
                return None;
            }
            Some((id, s.str("name").filter(|n| !n.is_empty())?.to_string()))
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    v.retain(|x| seen.insert(x.1.clone()));
    v
}

/// Named drop cap styles.
pub fn drop_cap_styles(doc: &Document) -> Vec<(ObjId, String)> {
    doc.file
        .objects_of("TSWP.DropCapStyleArchive")
        .filter_map(|(id, m)| {
            let s = m.find_super("TSS.StyleArchive")?;
            (s.bool("is_variation") != Some(true)).then(|| (id, s.str("name").unwrap_or("Drop Cap").to_string()))
        })
        .filter(|(id, _)| has_drop_cap(doc, *id))
        .collect()
}

/// List level of the touched paragraphs += delta (0..=8).
pub fn change_level(doc: &mut Document, h: &mut History, storage: ObjId, start: usize, end: usize, delta: i32) -> bool {
    let text = doc.storage_text(storage);
    let (touched, _) = touched_paras(&text, start, end);
    let starts = para_starts16(&text);
    if touched.is_empty() {
        return false;
    }
    doc.touch(h, storage);
    let Some(st) = doc.file.obj_mut(storage) else { return false };
    let t = st.msg_mut_or_create("table_para_data");
    let mut entries: Vec<(usize, u32, u32)> = t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0) as usize, e.u32("first").unwrap_or(0), e.u32("second").unwrap_or(0))).collect();
    entries.sort_by_key(|e| e.0);
    let level_at = |es: &[(usize, u32, u32)], i: usize| es.iter().take_while(|e| e.0 <= i).last().map(|e| (e.1, e.2)).unwrap_or((0, 0));
    let mut new: Vec<(usize, u32, u32)> = starts
        .iter()
        .map(|&p| {
            let (f, s2) = level_at(&entries, p);
            (p, f, s2)
        })
        .collect();
    for e in &mut new {
        if touched.contains(&e.0) {
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

/// The paragraph style of the paragraph holding stored byte `at_byte`.
pub fn para_style_at(doc: &Document, storage: ObjId, at_byte: usize) -> Option<ObjId> {
    let text = doc.storage_text(storage);
    let u = byte_to_u16(&text, at_byte.min(text.len()));
    let st = doc.file.obj(storage)?;
    let mut table = object_table(st, "table_para_style");
    let mut last = None;
    for e in &mut table {
        if e.1.is_some() {
            last = e.1;
        } else {
            e.1 = last;
        }
    }
    let starts = para_starts16(&text);
    let pstart = starts.iter().copied().take_while(|p| *p <= u).last().unwrap_or(0);
    at(&table, pstart)
}

/// The named style a (possibly varied) style comes from, its name, and whether it is overridden.
pub fn named_style(doc: &Document, style: Option<ObjId>) -> (Option<ObjId>, String, bool) {
    let mut cur = style;
    let mut overridden = false;
    let mut guard = 0;
    while let Some(id) = cur {
        guard += 1;
        if guard > 50 {
            break;
        }
        let Some(s) = doc.file.obj(id).and_then(|m| m.find_super("TSS.StyleArchive")) else { break };
        if s.bool("is_variation") == Some(true) {
            overridden = true;
            cur = s.ref_id("parent");
            continue;
        }
        return (Some(id), s.str("name").unwrap_or("").to_string(), overridden);
    }
    (None, String::new(), overridden)
}

/// The character style in force at stored byte `at_byte` (the character before the caret).
pub fn char_style_at(doc: &Document, storage: ObjId, at_byte: usize) -> Option<ObjId> {
    let text = doc.storage_text(storage);
    let u = byte_to_u16(&text, at_byte.min(text.len()));
    let st = doc.file.obj(storage)?;
    let t = object_table(st, "table_char_style");
    at(&t, u.saturating_sub(if at_byte > 0 { 1 } else { 0 }))
}

/// Paragraph style named `name`.
pub fn para_style_named(doc: &Document, name: &str) -> Option<ObjId> {
    doc.paragraph_styles().into_iter().find(|(_, n)| n == name).map(|(id, _)| id)
}

/// Update the named style from a variation's overrides (paragraph style popover > Update).
pub fn update_named_style(doc: &mut Document, h: &mut History, variation_id: ObjId) -> bool {
    let (Some(named), _, true) = named_style(doc, Some(variation_id)) else { return false };
    // gather the overrides of every variation level down to the named style
    let mut chain = Vec::new();
    let mut cur = Some(variation_id);
    while let Some(id) = cur {
        if id == named {
            break;
        }
        let Some(m) = doc.file.obj(id) else { break };
        chain.push(m.clone());
        cur = m.find_super("TSS.StyleArchive").and_then(|s| s.ref_id("parent"));
    }
    doc.touch(h, named);
    let Some(m) = doc.file.obj_mut(named) else { return false };
    for v in chain.iter().rev() {
        for part in ["char_properties", "para_properties"] {
            if let Some(src) = v.msg(part) {
                let dst = m.msg_mut_or_create(part);
                for f in &src.fields {
                    dst.fields.retain(|g| g.num != f.num);
                    dst.fields.push(f.clone());
                }
            }
        }
    }
    kn_format::objects::fix_override_count(m);
    true
}

/// Insert a page number / page count field at stored byte `at_byte`.
pub fn insert_page_field(doc: &mut Document, h: &mut History, storage: ObjId, at_byte: usize, count: bool) -> bool {
    let Some(file) = doc.file.file_of(storage).map(str::to_string) else { return false };
    let id = doc.structural(h, &[storage], |k| {
        let id = k.alloc_ids(1)[0];
        let mut m = Msg::new("TSWP.NumberAttachmentArchive");
        m.msg_mut_or_create("super").set_enum("kind", if count { "kKindPageCount" } else { "kKindPageNumber" });
        m.set_u64("number_format", 0);
        m.set_str("number_format_name", "decimal");
        k.insert_archive(&file, kn_format::iwa::Archive::new_object(id, m));
        id
    });
    insert_attachment(doc, h, storage, at_byte, id);
    true
}

/// The number style of the page number / page count fields in `storage`: 0 "1, 2, 3", 3 "I, II,
/// III", 6 "i, ii, iii", 9 "A, B, C", 12 "a, b, c" (kn_model::text::format_number's codes).
/// Returns how many fields changed.
pub fn set_page_field_format(doc: &mut Document, h: &mut History, storage: ObjId, format: u64) -> usize {
    let name = match format {
        3 => "upper-roman",
        6 => "lower-roman",
        9 => "upper-alpha",
        12 => "lower-alpha",
        _ => "decimal",
    };
    let ids: Vec<ObjId> = doc.file.obj(storage).map(|st| object_table(st, "table_attachment").into_iter().filter_map(|e| e.1).collect()).unwrap_or_default();
    let mut n = 0;
    for id in ids {
        if doc.file.obj(id).is_some_and(|m| m.type_name() == "TSWP.NumberAttachmentArchive") {
            doc.touch(h, id);
            if let Some(m) = doc.file.obj_mut(id) {
                m.set_u64("number_format", format);
                m.set_str("number_format_name", name);
                n += 1;
            }
        }
    }
    n
}

/// The number style of the first page number field in `storage` (see [`set_page_field_format`]).
pub fn page_field_format(doc: &Document, storage: ObjId) -> Option<u64> {
    let st = doc.file.obj(storage)?;
    object_table(st, "table_attachment").into_iter().filter_map(|e| e.1).filter_map(|id| doc.file.obj(id)).find(|m| m.type_name() == "TSWP.NumberAttachmentArchive").map(|m| m.u64("number_format").unwrap_or(0))
}

/// Put U+FFFC at stored byte `at_byte` with table_attachment pointing at `obj`.
pub fn insert_attachment(doc: &mut Document, h: &mut History, storage: ObjId, at_byte: usize, obj: ObjId) {
    replace(doc, h, storage, at_byte, at_byte, "\u{FFFC}");
    let text = doc.storage_text(storage);
    let u = byte_to_u16(&text, at_byte);
    doc.touch(h, storage);
    if let Some(st) = doc.file.obj_mut(storage) {
        let mut t = object_table(st, "table_attachment");
        t.retain(|e| e.0 != u);
        t.push((u, Some(obj)));
        t.sort_by_key(|e| e.0);
        write_table(st, "table_attachment", &t);
    }
    finish(doc, storage);
}

/// One level of a list style, resolved through its parents (Bullets & Lists details).
#[derive(Clone, Debug, PartialEq)]
pub struct ListLevel {
    /// 0 none, 1 image, 2 text bullet, 3 number.
    pub kind: u64,
    /// The bullet text (kind 2).
    pub string: String,
    /// TSWP.ListStyleArchive.NumberType (kind 3).
    pub number: u64,
    /// Where the label sits / where the text starts, points from the paragraph's indent.
    pub indent: f32,
    pub text_indent: f32,
    /// The label's size relative to the text.
    pub scale: f32,
    pub tiered: bool,
}

const LEVELS: usize = 9;

fn list_levels(doc: &Document, list: ObjId) -> Vec<ListLevel> {
    let ch = kn_model::style::chain(&doc.file, Some(list));
    let get_u = |name: &str| kn_model::style::lookup(&ch, |m| Some(m.u64s(name)).filter(|v| !v.is_empty())).unwrap_or_default();
    let get_f = |name: &str| kn_model::style::lookup(&ch, |m| Some(m.f32s(name)).filter(|v| !v.is_empty())).unwrap_or_default();
    let kinds = get_u("label_types");
    let numbers = get_u("number_types");
    let tiered = get_u("tiered_numbers");
    let indents = get_f("indents");
    let text_indents = get_f("text_indents");
    let strings: Vec<String> = kn_model::style::lookup(&ch, |m| Some(m.strs("strings")).filter(|v| !v.is_empty()).map(|v| v.into_iter().map(String::from).collect())).unwrap_or_default();
    let scales: Vec<f32> = kn_model::style::lookup(&ch, |m| Some(m.msgs("geometries").map(|g| g.f32("scale").unwrap_or(1.0)).collect::<Vec<_>>()).filter(|v| !v.is_empty())).unwrap_or_default();
    (0..LEVELS)
        .map(|l| ListLevel {
            kind: kinds.get(l).copied().unwrap_or(0),
            string: strings.get(l).cloned().unwrap_or_else(|| "•".into()),
            number: numbers.get(l).copied().unwrap_or(0),
            indent: indents.get(l).copied().unwrap_or(l as f32 * 18.0),
            text_indent: text_indents.get(l).copied().unwrap_or(18.0),
            scale: scales.get(l).copied().unwrap_or(1.0),
            tiered: tiered.get(l).is_some_and(|v| *v != 0),
        })
        .collect()
}

/// A list style's settings at `level`.
pub fn list_level(doc: &Document, list: ObjId, level: usize) -> ListLevel {
    list_levels(doc, list).swap_remove(level.min(LEVELS - 1))
}

/// What a list style's first-level label looks like ("•", "1.", "A.", "I.", "–", or "○" for
/// a picture bullet): the List Styles popover shows it beside the name.
pub fn list_marker(doc: &Document, list: ObjId) -> String {
    let l = list_level(doc, list, 0);
    match l.kind {
        1 => "○".into(),
        2 => l.string,
        3 => kn_model::text::format_number(1, l.number),
        _ => String::new(),
    }
}

fn write_f32s(m: &mut Msg, name: &str, vals: &[f32]) {
    let Some(num) = m.field_def(name).map(|d| d.num) else { return };
    m.fields.retain(|f| f.num != num);
    for v in vals {
        m.fields.push(kn_format::msg::Field { num, val: kn_format::Val::I32(v.to_bits()) });
    }
}

/// Change `level` of the touched paragraphs' list style (a variation of it, holding every
/// level so the parents' arrays don't mix in).
pub fn set_list_level(doc: &mut Document, h: &mut History, storage: ObjId, start: usize, end: usize, level: usize, f: &dyn Fn(&mut ListLevel)) -> bool {
    set_para_table(doc, h, storage, start, end, "table_list_style", &mut |doc, h, cur| {
        let base = cur?;
        let mut levels = list_levels(doc, base);
        f(&mut levels[level.min(LEVELS - 1)]);
        variation(doc, h, base, &|m| {
            m.set_u64s("label_types", &levels.iter().map(|l| l.kind).collect::<Vec<_>>());
            m.set_u64s("number_types", &levels.iter().map(|l| l.number).collect::<Vec<_>>());
            m.set_u64s("tiered_numbers", &levels.iter().map(|l| u64::from(l.tiered)).collect::<Vec<_>>());
            m.set_strs("strings", &levels.iter().map(|l| l.string.as_str()).collect::<Vec<_>>());
            write_f32s(m, "indents", &levels.iter().map(|l| l.indent).collect::<Vec<_>>());
            write_f32s(m, "text_indents", &levels.iter().map(|l| l.text_indent).collect::<Vec<_>>());
            m.remove("geometries");
            for l in &levels {
                let g = m.push_new("geometries");
                g.set_f32("scale", l.scale);
                g.set_f32("baseline_offset", 0.0);
                g.set_bool("scale_with_text", true);
            }
        })
    })
}

/// A drop cap style's settings, resolved through its parents.
#[derive(Clone, Debug, PartialEq)]
pub struct DropCapSettings {
    pub lines: u32,
    pub raised: u32,
    pub chars: u32,
    pub shape: bool,
    pub fill: Option<kn_model::Color>,
    pub corner_radius: f32,
    pub padding: f32,
    pub scale: f32,
    /// 0 rectangular, 1 contour, 2 none.
    pub wrap: u64,
    /// The characters' own colour and boldness, if the style sets them.
    pub color: Option<kn_model::Color>,
    pub bold: Option<bool>,
}

pub fn drop_cap_settings(doc: &Document, style: ObjId) -> DropCapSettings {
    let ch = kn_model::style::chain(&doc.file, Some(style));
    let dc = |f: &dyn Fn(&Msg) -> Option<f64>| kn_model::style::lookup(&ch, |m| m.msg("drop_cap_properties").and_then(|p| p.msg("drop_cap")).and_then(f));
    let fill = kn_model::style::lookup(&ch, |m| m.msg("drop_cap_properties").and_then(|p| p.msg("drop_cap_shape_fill")).map(kn_model::style::fill));
    DropCapSettings {
        lines: dc(&|c| c.u32("number_of_lines").map(f64::from)).unwrap_or(3.0) as u32,
        raised: dc(&|c| c.u32("number_of_raised_lines").map(f64::from)).unwrap_or(0.0) as u32,
        chars: dc(&|c| c.u32("number_of_characters").map(f64::from)).unwrap_or(1.0) as u32,
        shape: dc(&|c| c.bool("shape_enabled").map(|b| if b { 1.0 } else { 0.0 })).unwrap_or(0.0) > 0.5,
        fill: match fill {
            Some(kn_model::Fill::Color(c)) => Some(c),
            _ => None,
        },
        corner_radius: dc(&|c| c.f64("corner_radius")).unwrap_or(0.0) as f32,
        padding: dc(&|c| c.f64("padding")).unwrap_or(0.0) as f32,
        scale: dc(&|c| c.f64("character_scale")).filter(|v| *v > 0.05).unwrap_or(1.0) as f32,
        wrap: dc(&|c| c.u64("wrap_type").map(|v| v as f64)).unwrap_or(0.0) as u64,
        color: kn_model::style::lookup(&ch, |m| m.msg("char_properties").and_then(|c| c.msg("font_color")).map(kn_model::style::color)),
        bold: kn_model::style::lookup(&ch, |m| m.msg("char_properties").and_then(|c| c.bool("bold"))),
    }
}

/// Change the touched paragraphs' drop cap (a variation of its style; a paragraph without one
/// starts from `default_style`).
pub fn set_drop_cap_settings(doc: &mut Document, h: &mut History, storage: ObjId, start: usize, end: usize, default_style: ObjId, f: &dyn Fn(&mut DropCapSettings)) -> bool {
    set_para_table(doc, h, storage, start, end, "table_drop_cap_style", &mut |doc, h, cur| {
        let base = cur.unwrap_or(default_style);
        let mut s = drop_cap_settings(doc, base);
        f(&mut s);
        variation(doc, h, base, &|m| {
            let p = m.msg_mut_or_create("drop_cap_properties");
            let d = p.msg_mut_or_create("drop_cap");
            d.set_u64("number_of_lines", u64::from(s.lines.max(1)));
            d.set_u64("number_of_raised_lines", u64::from(s.raised.min(s.lines.max(1))));
            d.set_u64("number_of_characters", u64::from(s.chars.max(1)));
            d.set_bool("shape_enabled", s.shape);
            d.set_f64("corner_radius", f64::from(s.corner_radius));
            d.set_f64("padding", f64::from(s.padding));
            d.set_f64("character_scale", f64::from(s.scale));
            d.set_u64("wrap_type", s.wrap.min(2));
            p.remove("drop_cap_shape_fill");
            if let (true, Some(c)) = (s.shape, s.fill) {
                p.msg_mut_or_create("drop_cap_shape_fill").push_msg("color", kn_model::format::color_msg(c));
            }
        })
    })
}

/// The list style and list level of the paragraph holding stored byte `at_byte`.
pub fn list_at(doc: &Document, storage: ObjId, at_byte: usize) -> (Option<ObjId>, usize) {
    let text = doc.storage_text(storage);
    let u = byte_to_u16(&text, at_byte.min(text.len()));
    let Some(st) = doc.file.obj(storage) else { return (None, 0) };
    let list = at(&object_table(st, "table_list_style"), u);
    let level = st
        .msg("table_para_data")
        .map(|t| {
            let mut es: Vec<(usize, u32)> = t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0) as usize, e.u32("first").unwrap_or(0))).collect();
            es.sort_by_key(|e| e.0);
            es.iter().take_while(|e| e.0 <= u).last().map(|e| e.1).unwrap_or(0)
        })
        .unwrap_or(0);
    (list, level as usize)
}

/// The drop cap style of the paragraph holding stored byte `at_byte` (an entry at that
/// paragraph's start; the table's entries aren't carried on to later paragraphs).
pub fn drop_cap_at(doc: &Document, storage: ObjId, at_byte: usize) -> Option<ObjId> {
    let text = doc.storage_text(storage);
    let at_byte = at_byte.min(text.len());
    let start = text[..at_byte].rfind(|c: char| crate::body::is_para_end(c)).map(|i| i + text[i..].chars().next().map_or(1, char::len_utf8)).unwrap_or(0);
    let u = byte_to_u16(&text, start);
    let st = doc.file.obj(storage)?;
    object_table(st, "table_drop_cap_style").into_iter().find(|e| e.0 == u).and_then(|e| e.1).filter(|id| has_drop_cap(doc, *id))
}

/// Whether a drop cap style defines a drop cap (the stylesheet's default one doesn't: "none").
fn has_drop_cap(doc: &Document, style: ObjId) -> bool {
    let ch = kn_model::style::chain(&doc.file, Some(style));
    kn_model::style::lookup(&ch, |m| m.msg("drop_cap_properties").and_then(|p| p.msg("drop_cap")).map(|_| ())).is_some()
}
