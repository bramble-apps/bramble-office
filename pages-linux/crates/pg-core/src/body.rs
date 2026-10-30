//! A Pages text storage (the body, a header, a footnote…) -> the paragraphs to lay out.
//!
//! Like kn-model's `text::text_box`, with what Pages adds: page / column / section breaks end a
//! paragraph (U+0005, U+000C, U+0004; Pages puts a paragraph-table entry after each), footnote
//! marks (U+000E) show their number, page number / page count attachments show their value,
//! inline drawables and tables of contents are blocks of their own, and the style lookups are
//! memoised and index maps precomputed so a long document builds in linear time.
//!
//! Positions: the *stored* text is the storage's string (UTF-8 bytes here; the tables use
//! UTF-16 indices); the *shown* text is [`Body::tb`]`.text`. [`Body::to_shown`] and
//! [`Body::to_stored`] map byte offsets between the two.

use std::collections::HashMap;

use kn_format::{KeyFile, Msg, ObjId};
use kn_model::style::{self, ParaProps};
use kn_model::text::format_number;
use kn_model::*;

/// What ends a paragraph besides a Return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Break {
    Page,
    Column,
    Section,
}

/// Pages-only paragraph properties and what the paragraph holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParaExtra {
    /// The paragraph style (TSWP.ParagraphStyleArchive) and list style in force.
    pub style: Option<ObjId>,
    pub list: Option<ObjId>,
    /// The break character that ends this paragraph (it is not part of the shown text).
    pub brk: Option<Break>,
    pub keep_with_next: bool,
    pub keep_together: bool,
    pub page_break_before: bool,
    pub widow_control: bool,
    /// Paragraph background colour and border (Layout > Paragraph Borders).
    pub fill: Option<Color>,
    pub border: Option<(Color, f32)>,
    /// Which edges the border has (Pages' border_positions: 1 the top, 2 the bottom; 15 Box).
    pub border_edges: u64,
    /// The border's distance from the text (Word's w:space), when the document gives one.
    pub border_offset: Option<f32>,
    /// Drop cap: (characters, lines).
    pub drop_cap: Option<(u32, u32)>,
    /// A drop cap drawn on a shape (Pages: Drop Cap > Shape): fill colour, corner radius (a
    /// fraction of the shape's side), padding (points) and the characters' scale within it.
    pub drop_cap_shape: Option<(kn_model::Color, f32, f32, f32)>,
    /// The paragraph is one inline/anchored drawable (U+FFFC + TSWP.DrawableAttachmentArchive).
    pub drawables: Vec<ObjId>,
    /// The paragraph holds a table of contents (TSWP.TOCAttachmentArchive -> TOCInfoArchive).
    pub toc: Option<ObjId>,
    /// Outline level for the table of contents (headings), when the style is listed in the TOC.
    pub show_in_toc: bool,
    /// Section starting with this paragraph (TP.SectionArchive).
    pub section: Option<ObjId>,
}

/// A run of text with a meaning beyond its style.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// Shown byte range.
    pub start: usize,
    pub end: usize,
    pub kind: SpanKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpanKind {
    Link(String),
    /// Comment or highlight: the TSWP.HighlightArchive, its comment storage and the author colour.
    Comment { highlight: ObjId, storage: Option<ObjId>, color: Color },
    Bookmark(String),
    /// Tracked change (TSWP.ChangeArchive): insertion or deletion, author colour.
    Change { change: ObjId, deletion: bool, color: Color },
    /// Footnote mark: the footnote's storage and its number.
    Footnote { storage: ObjId, number: usize },
    /// Placeholder text (tinted, replaced when typed into).
    Placeholder,
    /// Page number / page count attachment.
    PageField { count: bool },
    /// An inline object's place in the text (a figure space standing in for it): the drawable.
    Object(ObjId),
}

/// An inline (wrap 0) object's size (w, h), or None for an anchored one.
pub fn inline_size(k: &KeyFile, drawable: ObjId) -> Option<(f32, f32)> {
    let d = k.obj(drawable)?.find_super("TSD.DrawableArchive")?;
    if d.msg("exterior_text_wrap").and_then(|w| w.u64("type")).unwrap_or(0) != 0 {
        return None;
    }
    let g = kn_model::scene::geometry(d);
    Some((g.w, g.h))
}

/// The character standing in for an inline object in the shown text.
pub const OBJECT_CHAR: char = '\u{2007}';

pub struct Body {
    pub storage: ObjId,
    pub tb: TextBox,
    pub extra: Vec<ParaExtra>,
    pub spans: Vec<Span>,
    /// shown byte -> stored byte (len = shown len + 1)
    shown_to_stored: Vec<u32>,
    /// stored byte -> shown byte (len = stored len + 1)
    stored_to_shown: Vec<u32>,
    /// The stored text.
    pub stored: String,
}

impl Body {
    pub fn to_shown(&self, stored: usize) -> usize {
        self.stored_to_shown.get(stored.min(self.stored_to_shown.len().saturating_sub(1))).copied().unwrap_or(0) as usize
    }
    pub fn to_stored(&self, shown: usize) -> usize {
        self.shown_to_stored.get(shown.min(self.shown_to_stored.len().saturating_sub(1))).copied().unwrap_or(0) as usize
    }
    /// The paragraph holding shown byte `shown` (a paragraph's end belongs to it).
    pub fn para_at(&self, shown: usize) -> usize {
        let p = &self.tb.paragraphs;
        match p.binary_search_by(|q| if q.end < shown { std::cmp::Ordering::Less } else if q.start > shown { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal }) {
            Ok(i) => i,
            Err(i) => i.min(p.len().saturating_sub(1)),
        }
    }
    /// The paragraph holding stored byte `stored`.
    pub fn para_at_stored(&self, stored: usize) -> usize {
        self.para_at(self.to_shown(stored))
    }
}

/// Options for building a storage.
#[derive(Clone, Copy)]
pub struct BuildCtx {
    /// Page number and count for page-number attachments (headers/footers); 0 shows "#".
    pub page: usize,
    pub page_count: usize,
    /// Number of the first footnote in this storage.
    pub first_footnote: usize,
    /// Show tracked deletions (struck through) or hide them.
    pub show_deletions: bool,
    /// Show change-tracking markup (colours) at all.
    pub show_markup: bool,
    /// A Word document: a list label takes the paragraph style's character formatting (Word's
    /// paragraph mark), not the first run's as in Pages and Keynote.
    pub word_labels: bool,
}

impl Default for BuildCtx {
    fn default() -> Self {
        BuildCtx { page: 0, page_count: 0, first_footnote: 1, show_deletions: true, show_markup: true, word_labels: false }
    }
}

fn object_table(st: &Msg, name: &str) -> Vec<(usize, Option<ObjId>)> {
    let mut v: Vec<_> = st
        .msg(name)
        .map(|t| t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0) as usize, e.ref_id("object"))).collect())
        .unwrap_or_default();
    v.sort_by_key(|e| e.0);
    v
}

fn para_data_table(st: &Msg, name: &str) -> Vec<(usize, u32)> {
    let mut v: Vec<_> = st
        .msg(name)
        .map(|t| t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0) as usize, e.u32("first").unwrap_or(0))).collect())
        .unwrap_or_default();
    v.sort_by_key(|e| e.0);
    v
}

/// The entry in force at `idx` (binary search; tables are sorted).
fn at<T: Copy>(t: &[(usize, T)], idx: usize) -> Option<T> {
    let n = t.partition_point(|e| e.0 <= idx);
    (n > 0).then(|| t[n - 1].1)
}

/// Paragraph-style tables: an entry without an object continues the previous one.
fn carry(mut t: Vec<(usize, Option<ObjId>)>) -> Vec<(usize, Option<ObjId>)> {
    let mut last = None;
    for e in &mut t {
        if e.1.is_some() {
            last = e.1;
        } else {
            e.1 = last;
        }
    }
    t
}

fn is_break(c: char) -> Option<Break> {
    match c {
        '\u{5}' => Some(Break::Page),
        '\u{c}' => Some(Break::Column),
        '\u{4}' => Some(Break::Section),
        _ => None,
    }
}

pub fn is_para_end(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2029}' | '\u{5}' | '\u{c}' | '\u{4}')
}

/// The author colour of a TSWP.ChangeArchive (session -> author).
fn change_color(k: &KeyFile, change: &Msg) -> Color {
    change
        .ref_id("session")
        .and_then(|s| k.obj(s))
        .and_then(|s| s.ref_id("author"))
        .and_then(|a| k.obj(a))
        .and_then(|a| a.msg("color"))
        .map(style::color)
        .unwrap_or(Color { r: 0.16, g: 0.6, b: 0.25, a: 1.0 })
}

pub fn build(k: &KeyFile, storage: ObjId, default_para_style: Option<ObjId>, ctx: BuildCtx) -> Option<Body> {
    let st = k.obj(storage)?;
    let stored: String = st.strs("text").concat();

    let para_styles = carry(object_table(st, "table_para_style"));
    let char_styles = object_table(st, "table_char_style");
    let list_styles = carry(object_table(st, "table_list_style"));
    let levels = para_data_table(st, "table_para_data");
    let attachments = object_table(st, "table_attachment");
    let footnotes = object_table(st, "table_footnote");
    let smartfields = object_table(st, "table_smartfield");
    let highlights = object_table(st, "table_highlight");
    let bookmarks = object_table(st, "table_bookmark");
    let insertions = object_table(st, "table_insertion");
    let deletions = object_table(st, "table_deletion");
    let sections = object_table(st, "table_section");
    let dropcaps = object_table(st, "table_drop_cap_style");

    // stored UTF-16 index -> stored byte
    let mut u16_to_stored: Vec<u32> = Vec::with_capacity(stored.len() + 1);
    for (b, ch) in stored.char_indices() {
        for _ in 0..ch.len_utf16() {
            u16_to_stored.push(b as u32);
        }
    }
    u16_to_stored.push(stored.len() as u32);
    let s16 = |i: usize| u16_to_stored[i.min(u16_to_stored.len() - 1)] as usize;

    // hidden deleted ranges (stored bytes)
    let mut hidden: Vec<(usize, usize)> = Vec::new();
    if !ctx.show_deletions {
        for (i, (ci, obj)) in deletions.iter().enumerate() {
            if obj.is_some() {
                let end = deletions.get(i + 1).map(|e| e.0).unwrap_or(usize::MAX);
                hidden.push((s16(*ci), s16(end)));
            }
        }
    }
    let is_hidden = |b: usize| hidden.iter().any(|&(a, e)| b >= a && b < e);

    // Pass 1: the shown text, the maps, the paragraph ranges and breaks.
    let mut text = String::with_capacity(stored.len());
    let mut stored_to_shown: Vec<u32> = vec![0; stored.len() + 1];
    let mut shown_to_stored: Vec<u32> = Vec::with_capacity(stored.len() + 1);
    let mut ranges: Vec<(usize, usize, Option<Break>, usize)> = Vec::new(); // shown start, end, break, stored start
    let mut pstart = 0usize;
    let mut pstart_stored = 0usize;
    let mut spans: Vec<Span> = Vec::new();
    let mut footnote_no = ctx.first_footnote;
    let mut para_attachments: Vec<(usize, ObjId)> = Vec::new(); // (shown offset, attachment)
    let mut u16i = 0usize;
    for (b, ch) in stored.char_indices() {
        let here = u16i;
        u16i += ch.len_utf16();
        let shown_at = text.len();
        for i in 0..ch.len_utf8() {
            stored_to_shown[b + i] = shown_at as u32;
        }
        if is_hidden(b) {
            continue;
        }
        let push_map = |text: &String, shown_to_stored: &mut Vec<u32>| {
            while shown_to_stored.len() < text.len() {
                shown_to_stored.push(b as u32);
            }
        };
        if is_para_end(ch) {
            ranges.push((pstart, text.len(), is_break(ch), pstart_stored));
            // the separator stays in the shown text as one byte ('\n') so offsets stay simple
            text.push('\n');
            push_map(&text, &mut shown_to_stored);
            pstart = text.len();
            pstart_stored = b + ch.len_utf8();
            continue;
        }
        match ch {
            '\u{E}' => {
                // footnote mark: its number, superscript (style applied below)
                let fstorage = at(&footnotes, here).flatten().and_then(|f| k.obj(f)).and_then(|f| f.ref_id("contained_storage"));
                let s = footnote_no.to_string();
                text.push_str(&s);
                if let Some(fs) = fstorage {
                    spans.push(Span { start: shown_at, end: text.len(), kind: SpanKind::Footnote { storage: fs, number: footnote_no } });
                }
                footnote_no += 1;
            }
            '\u{FFFC}' => {
                let att = attachments.iter().find(|a| a.0 == here).and_then(|a| a.1);
                match att.and_then(|id| k.obj(id).map(|m| (id, m))) {
                    Some((_, m)) if m.type_name() == "TSWP.NumberAttachmentArchive" => {
                        let count = m.msg("super").and_then(|s| s.enum_name("kind")) == Some("kKindPageCount");
                        let v = if count { ctx.page_count } else { ctx.page };
                        let s = if v == 0 { "#".to_string() } else { format_number(v as u32, m.u64("number_format").unwrap_or(0)).trim_start_matches('(').trim_end_matches(['.', ')']).to_string() };
                        text.push_str(&s);
                        spans.push(Span { start: shown_at, end: text.len(), kind: SpanKind::PageField { count } });
                    }
                    Some((_, m)) if m.type_name() == "TSWP.TextualAttachmentArchive" && m.enum_name("kind") == Some("kKindFootnoteMark") => {
                        let s = ctx.first_footnote.to_string();
                        text.push_str(&s);
                    }
                    Some((id, m)) if m.type_name() == "TSWP.TOCAttachmentArchive" => {
                        para_attachments.push((shown_at, id));
                    }
                    Some((id, m)) if m.type_name() == "TSWP.DrawableAttachmentArchive" => {
                        // a figure space holds the object's place on its line
                        para_attachments.push((shown_at, id));
                        text.push(OBJECT_CHAR);
                        if let Some(d) = m.ref_id("drawable").or_else(|| m.msg("super").and_then(|s| s.ref_id("drawable"))) {
                            spans.push(Span { start: shown_at, end: text.len(), kind: SpanKind::Object(d) });
                        }
                    }
                    Some((_, m)) if m.type_name() == "TSWP.TSWPTOCPageNumberAttachmentArchive" => {
                        let s = m.u64("page_number").map(|n| n.to_string()).or_else(|| m.str("page_number_string").map(String::from)).unwrap_or_else(|| "#".into());
                        text.push_str(&s);
                    }
                    _ => {}
                }
            }
            _ => text.push(ch),
        }
        push_map(&text, &mut shown_to_stored);
    }
    ranges.push((pstart, text.len(), None, pstart_stored));
    shown_to_stored.push(stored.len() as u32);
    stored_to_shown[stored.len()] = text.len() as u32;
    // a shown position after the last char of a field maps past it
    let shown_to_stored_fn = |s: usize| shown_to_stored[s.min(shown_to_stored.len() - 1)] as usize;
    let to_shown = |sb: usize| stored_to_shown[sb.min(stored_to_shown.len() - 1)] as usize;
    // stored u16 -> shown byte
    let u2s = |i: usize| to_shown(s16(i));

    // Memoised style lookups.
    let mut pp_cache: HashMap<Option<ObjId>, ParaProps> = HashMap::new();
    let mut cs_cache: HashMap<(Option<ObjId>, Option<ObjId>), CharStyle> = HashMap::new();
    let stored_u16_of = |sb: usize| -> usize { u16_to_stored.partition_point(|&x| (x as usize) < sb) };

    // Pass 2: paragraphs.
    // numbering per list style: it continues over paragraphs in between (other lists, "None"),
    // as Pages numbers a list through the document
    let mut counters_of: HashMap<ObjId, [u32; 10]> = HashMap::new();
    let mut paragraphs = Vec::with_capacity(ranges.len());
    let mut extra = Vec::with_capacity(ranges.len());
    // character-run cut points (shown bytes) from every char-indexed table
    let mut cuts_all: Vec<usize> = Vec::new();
    for t in [&char_styles, &insertions, &deletions] {
        cuts_all.extend(t.iter().map(|e| u2s(e.0)));
    }
    for sp in &spans {
        cuts_all.push(sp.start);
        cuts_all.push(sp.end);
    }
    cuts_all.sort_unstable();
    cuts_all.dedup();

    for (ps, pe, brk, pstored) in ranges {
        let u_start = stored_u16_of(pstored);
        let pstyle = at(&para_styles, u_start).flatten().or(default_para_style);
        let pp = pp_cache.entry(pstyle).or_insert_with(|| style::para_props(k, pstyle));
        let level = at(&levels, u_start).unwrap_or(0).min(8);
        let list = at(&list_styles, u_start).flatten().or(pp.list_style);

        let lo = cuts_all.partition_point(|&c| c <= ps);
        let hi = cuts_all.partition_point(|&c| c < pe);
        let mut cuts: Vec<usize> = Vec::with_capacity(hi.saturating_sub(lo) + 2);
        cuts.push(ps);
        cuts.extend_from_slice(&cuts_all[lo..hi.max(lo)]);
        cuts.push(pe);
        let mut runs = Vec::with_capacity(cuts.len());
        for w in cuts.windows(2) {
            if w[1] <= w[0] && !(w[0] == ps && w[1] == pe) {
                continue;
            }
            let su = stored_u16_of(shown_to_stored_fn(w[0]));
            let cstyle = at(&char_styles, su).flatten();
            let mut cs = cs_cache.entry((pstyle, cstyle)).or_insert_with(|| style::char_style(k, pstyle, cstyle)).clone();
            if ctx.show_markup {
                if let Some(Some(c)) = at(&insertions, su).map(|o| o.and_then(|id| k.obj(id))) {
                    cs.underline = true;
                    cs.color = change_color(k, c);
                }
                if let Some(Some(c)) = at(&deletions, su).map(|o| o.and_then(|id| k.obj(id))) {
                    cs.strike = true;
                    cs.color = change_color(k, c);
                }
            }
            if spans.iter().any(|s| matches!(s.kind, SpanKind::Footnote { .. }) && s.start <= w[0] && w[0] < s.end) {
                cs.script = 1;
            }
            // an inline object among text: its stand-in as tall and as wide as the object
            if let Some(d) = spans.iter().find_map(|s| match s.kind {
                SpanKind::Object(d) if s.start == w[0] && s.end == w[1] => Some(d),
                _ => None,
            }) {
                let has_text = text[ps..pe].chars().any(|c| !c.is_whitespace());
                if let Some(g) = has_text.then(|| inline_size(k, d)).flatten() {
                    cs.size = g.1.max(1.0);
                    // the figure space is about 0.56 em; letter spacing makes up the width
                    cs.tracking = ((g.0 + 2.0) / cs.size - 0.56).max(0.0);
                    cs.underline = false;
                    cs.strike = false;
                    cs.background = None;
                }
            }
            runs.push(Run { start: w[0], end: w[1], style: cs });
        }
        if runs.is_empty() {
            let cstyle = at(&char_styles, u_start).flatten();
            runs.push(Run { start: ps, end: pe, style: style::char_style(k, pstyle, cstyle) });
        } else if ctx.word_labels && text[ps..pe].ends_with('\u{2028}') {
            // a Word paragraph ending in a line break: its empty last line holds only the
            // paragraph mark, which sizes it (the break's formatting: a zero-length last run)
            let cstyle = at(&char_styles, stored_u16_of(shown_to_stored_fn(pe))).flatten();
            runs.push(Run { start: pe, end: pe, style: style::char_style(k, pstyle, cstyle) });
        }

        // list label (as kn-model's text_box)
        let mut label = None;
        let (mut list_indent, mut label_indent, mut label_scale, mut label_color) = (0.0, 0.0, 1.0, None);
        if list.and_then(|id| k.obj(id)).is_some() {
            let ch = style::chain(k, list);
            let lvl = level as usize;
            let kind = style::lookup(&ch, |m| m.u64s("label_types").get(lvl).copied()).unwrap_or(0);
            list_indent = style::lookup(&ch, |m| m.f32s("indents").get(lvl).copied()).unwrap_or(0.0);
            label_indent = style::lookup(&ch, |m| m.f32s("text_indents").get(lvl).copied()).unwrap_or(0.0);
            label_scale = style::lookup(&ch, |m| m.msgs("geometries").nth(lvl).and_then(|g| g.f32("scale"))).unwrap_or(1.0);
            label_color = style::lookup(&ch, |m| m.msg("font_color").map(style::color));
            let counters = counters_of.entry(list.unwrap_or(0)).or_insert([0; 10]);
            match kind {
                2 => {
                    let strings = style::lookup(&ch, |m| {
                        let v = m.strs("strings");
                        (!v.is_empty()).then(|| v.into_iter().map(String::from).collect::<Vec<_>>())
                    })
                    .unwrap_or_default();
                    label = Some(ListLabel::Bullet(strings.get(lvl).cloned().unwrap_or_else(|| "•".into())));
                }
                3 => {
                    counters[lvl] += 1;
                    for c in counters.iter_mut().skip(lvl + 1) {
                        *c = 0;
                    }
                    let number_types = style::lookup(&ch, |m| {
                        let v = m.u64s("number_types");
                        (!v.is_empty()).then_some(v)
                    })
                    .unwrap_or_default();
                    let nt = number_types.get(lvl).copied().unwrap_or(0);
                    // a tiered level (Format > Bullets & Lists > Tiered Numbers): the higher
                    // levels' numbers before its own ("1.2.")
                    let tiered = style::lookup(&ch, |m| m.u64s("tiered_numbers").get(lvl).copied()).unwrap_or(0) != 0;
                    label = Some(ListLabel::Number(if tiered && lvl > 0 {
                        let bare = |i: usize| format_number(counters[i].max(1), number_types.get(i).copied().unwrap_or(0) / 3 * 3).trim_end_matches('.').to_string();
                        let body = (0..=lvl).map(bare).collect::<Vec<_>>().join(".");
                        match nt % 3 {
                            1 => format!("({body})"),
                            2 => format!("{body})"),
                            _ => format!("{body}."),
                        }
                    } else {
                        format_number(counters[lvl], nt)
                    }));
                }
                1 => {
                    if let Some(id) = style::lookup(&ch, |m| m.msgs("images").nth(lvl).cloned()).as_ref().and_then(|i| i.msg("image")).and_then(|d| d.u64("identifier")) {
                        label = Some(ListLabel::Image(id));
                    }
                }
                _ => {}
            }
        }

        // a Word list label in the paragraph mark's formatting: a zero-length first run (the
        // renderer draws the label, and sizes its gap, in the first run's style)
        if ctx.word_labels && label.is_some() {
            runs.insert(0, Run { start: ps, end: ps, style: style::char_style(k, pstyle, None) });
        }

        // Pages-only paragraph properties
        let ch = style::chain(k, pstyle);
        let props: Vec<&Msg> = ch.iter().filter_map(|m| m.msg("para_properties")).collect();
        let b = |n: &str| style::lookup(&props, |p| p.bool(n)).unwrap_or(false);
        let mut ex = ParaExtra {
            style: pstyle,
            list,
            brk,
            keep_with_next: b("keep_with_next"),
            keep_together: b("keep_lines_together"),
            page_break_before: b("page_break_before"),
            widow_control: style::lookup(&props, |p| p.bool("widow_control")).unwrap_or(true),
            fill: style::lookup(&props, |p| p.msg("fill")).map(style::color).filter(|c| c.a > 0.0),
            border: style::lookup(&props, |p| p.msg("stroke"))
                .and_then(style::stroke)
                .filter(|_| style::lookup(&props, |p| p.u64("border_positions")).unwrap_or(0) != 0)
                .map(|s| (s.color, s.width)),
            border_edges: style::lookup(&props, |p| p.u64("border_positions")).unwrap_or(0),
            border_offset: style::lookup(&props, |p| p.msg("historical_rule_offset").and_then(|o| o.f32("y"))),
            show_in_toc: b("show_in_toc"),
            ..Default::default()
        };
        // a drop cap entry is its own paragraph's (Pages lists one where a paragraph has a drop
        // cap; the paragraphs after it without an entry have none)
        if let Some(dc) = dropcaps.iter().find(|e| e.0 == u_start).and_then(|e| e.1).filter(|id| k.obj(*id).is_some()) {
            let d = style::chain(k, Some(dc));
            let cap = |f: &dyn Fn(&Msg) -> Option<f64>| style::lookup(&d, |m| m.msg("drop_cap_properties").and_then(|p| p.msg("drop_cap")).and_then(f));
            // a style without drop cap properties is the "no drop cap" one (Pages points every
            // other paragraph at it)
            let defined = style::lookup(&d, |m| m.msg("drop_cap_properties").and_then(|p| p.msg("drop_cap")).map(|_| ())).is_some();
            let lines = if defined { cap(&|c| c.u32("number_of_lines").map(f64::from)).unwrap_or(3.0) as u32 } else { 0 };
            let chars = cap(&|c| c.u32("number_of_characters").map(f64::from)).unwrap_or(1.0) as u32;
            if lines > 0 && chars > 0 {
                ex.drop_cap = Some((chars, lines));
                let shaped = cap(&|c| c.bool("shape_enabled").map(|b| if b { 1.0 } else { 0.0 })).unwrap_or(0.0) > 0.5;
                let fill = style::lookup(&d, |m| m.msg("drop_cap_properties").and_then(|p| p.msg("drop_cap_shape_fill")).map(style::fill));
                if let (true, Some(kn_model::Fill::Color(c))) = (shaped, fill) {
                    let radius = cap(&|c| c.f64("corner_radius")).unwrap_or(0.0) as f32;
                    let padding = cap(&|c| c.f64("padding")).unwrap_or(0.0) as f32;
                    let scale = cap(&|c| c.f64("character_scale")).filter(|v| *v > 0.05).unwrap_or(0.9) as f32;
                    ex.drop_cap_shape = Some((c, radius, padding, scale));
                }
            }
        }
        if let Some((_, Some(sec))) = sections.iter().find(|e| e.0 == u_start).copied() {
            ex.section = Some(sec);
        }
        for (off, att) in &para_attachments {
            if *off >= ps && *off <= pe {
                match k.obj(*att) {
                    Some(m) if m.type_name() == "TSWP.TOCAttachmentArchive" => ex.toc = m.msg("super").and_then(|s| s.ref_id("drawable")),
                    Some(m) => {
                        if let Some(d) = m.ref_id("drawable").or_else(|| m.msg("super").and_then(|s| s.ref_id("drawable"))) {
                            ex.drawables.push(d);
                        }
                    }
                    None => {}
                }
            }
        }
        // tab stops (Format > Tabs: the stops, the default interval; Pages' 36 pt by default)
        let tab_props: Vec<&Msg> = style::chain(k, pstyle).iter().filter_map(|m| m.msg("para_properties")).collect();
        let default_tab = style::lookup(&tab_props, |p| p.f32("default_tab_stops")).filter(|v| *v > 0.0).unwrap_or(36.0);
        let tabs: Vec<kn_model::TabStop> = style::lookup(&tab_props, |p| p.msg("tabs").map(|t| {
            t.msgs("tabs")
                .map(|e| kn_model::TabStop {
                    pos: e.f32("position").unwrap_or(0.0),
                    align: match e.enum_name("alignment") {
                        Some("kTabAlignmentCenter") => kn_model::TabAlign::Center,
                        Some("kTabAlignmentRight") => kn_model::TabAlign::Right,
                        Some("kTabAlignmentDecimal") => kn_model::TabAlign::Decimal,
                        _ => kn_model::TabAlign::Left,
                    },
                    leader: e.str("leader").and_then(|l| l.chars().next()),
                })
                .collect()
        }))
        .unwrap_or_default();
        let pp = &pp_cache[&pstyle];
        // a Word list whose first line's text starts elsewhere than its other lines' (the tab
        // after the label went to a tab stop): carried in the first-line indent, from the edge
        let first_text_indent = (ctx.word_labels && label.is_some() && (pp.first_line_indent - pp.left_indent).abs() > 0.01).then(|| pp.first_line_indent - pp.left_indent - list_indent);
        paragraphs.push(Paragraph {
            start: ps,
            end: pe,
            runs,
            align: pp.align,
            line_spacing: pp.line_spacing,
            space_before: pp.space_before,
            space_after: pp.space_after,
            left_indent: pp.left_indent,
            right_indent: pp.right_indent,
            first_line_indent: pp.first_line_indent,
            level,
            label,
            list_indent,
            label_indent,
            label_color,
            label_scale,
            first_text_indent,
            tabs,
            default_tab,
            rtl: pp.rtl,
        });
        extra.push(ex);
    }

    // Spans from the field tables.
    let span_table = |t: &[(usize, Option<ObjId>)], f: &mut dyn FnMut(usize, usize, ObjId, &Msg)| {
        for (i, (ci, obj)) in t.iter().enumerate() {
            let Some(id) = *obj else { continue };
            let Some(m) = k.obj(id) else { continue };
            let end = t.get(i + 1).map(|e| e.0).unwrap_or(usize::MAX);
            f(u2s(*ci), u2s(end), id, m);
        }
    };
    span_table(&smartfields, &mut |a, e, _, m| match m.type_name() {
        "TSWP.HyperlinkFieldArchive" => spans.push(Span { start: a, end: e, kind: SpanKind::Link(m.str("url_ref").unwrap_or("").to_string()) }),
        "TSWP.PlaceholderSmartFieldArchive" => spans.push(Span { start: a, end: e, kind: SpanKind::Placeholder }),
        _ => {}
    });
    span_table(&highlights, &mut |a, e, id, m| {
        let storage = m.ref_id("commentStorage");
        let color = storage
            .and_then(|c| k.obj(c))
            .and_then(|c| c.ref_id("author"))
            .and_then(|a| k.obj(a))
            .and_then(|a| a.msg("color"))
            .map(style::color)
            .unwrap_or(Color { r: 0.98, g: 0.86, b: 0.25, a: 1.0 });
        spans.push(Span { start: a, end: e, kind: SpanKind::Comment { highlight: id, storage, color } });
    });
    span_table(&bookmarks, &mut |a, e, _, m| spans.push(Span { start: a, end: e, kind: SpanKind::Bookmark(m.str("name").unwrap_or("").to_string()) }));
    for (t, deletion) in [(&insertions, false), (&deletions, true)] {
        span_table(t, &mut |a, e, id, m| spans.push(Span { start: a, end: e, kind: SpanKind::Change { change: id, deletion, color: change_color(k, m) } }));
    }
    spans.sort_by_key(|s| s.start);

    // comment highlights (hidden with the comments)
    let review: Vec<(usize, usize, Color)> = spans
        .iter()
        .filter(|_| ctx.show_markup)
        .filter_map(|s| match &s.kind {
            SpanKind::Comment { color, .. } => Some((s.start, s.end, Color { a: 0.45, ..*color })),
            _ => None,
        })
        .collect();

    let tb = TextBox {
        storage,
        text,
        paragraphs,
        valign: VAlign::Top,
        padding: [0.0; 4],
        shrink_to_fit: false,
        columns: 1,
        column_gap: 0.0,
        autogrow: false,
        highlights: review,
        fields: Vec::new(),
    };
    Some(Body { storage, tb, extra, spans, shown_to_stored, stored_to_shown, stored })
}
