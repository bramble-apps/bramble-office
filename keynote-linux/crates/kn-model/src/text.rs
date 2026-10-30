//! TSWP.StorageArchive -> TextBox (paragraphs, runs, list labels), styles resolved.
//!
//! Storage tables are keyed by UTF-16 character index (NSString); we convert to byte offsets.

use kn_format::{KeyFile, Msg, ObjId};

use crate::style::{self, ParaProps};
use crate::types::*;

/// (utf16 index, value) entries of an attribute table, sorted.
fn object_table(storage: &Msg, name: &str) -> Vec<(usize, Option<ObjId>)> {
    let mut v: Vec<_> = storage
        .msg(name)
        .map(|t| {
            t.msgs("entries")
                .map(|e| (e.u64("character_index").unwrap_or(0) as usize, e.ref_id("object")))
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|e| e.0);
    v
}

fn para_data_table(storage: &Msg, name: &str) -> Vec<(usize, u32)> {
    let mut v: Vec<_> = storage
        .msg(name)
        .map(|t| {
            t.msgs("entries")
                .map(|e| (e.u64("character_index").unwrap_or(0) as usize, e.u32("first").unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|e| e.0);
    v
}

fn at<T: Copy>(table: &[(usize, T)], idx: usize) -> Option<T> {
    table.iter().take_while(|e| e.0 <= idx).last().map(|e| e.1)
}

/// Context for resolving dynamic text (slide numbers).
#[derive(Clone, Copy, Default)]
pub struct TextContext {
    pub slide_number: usize,
}

pub struct TextDefaults {
    pub para_style: Option<ObjId>,
}

pub fn text_box(
    k: &KeyFile,
    storage_id: ObjId,
    defaults: &TextDefaults,
    layout: style::TextLayout,
    ctx: TextContext,
) -> Option<TextBox> {
    let st = k.obj(storage_id)?;
    let raw: String = st.strs("text").concat();

    // paragraph/list tables: an entry without an object continues the previous style
    let carry = |mut t: Vec<(usize, Option<ObjId>)>| {
        let mut last = None;
        for e in &mut t {
            if e.1.is_some() {
                last = e.1;
            } else {
                e.1 = last;
            }
        }
        t
    };
    let para_styles = carry(object_table(st, "table_para_style"));
    let char_styles = object_table(st, "table_char_style");
    let list_styles = carry(object_table(st, "table_list_style"));
    let levels = para_data_table(st, "table_para_data");
    let attachments = object_table(st, "table_attachment");

    // Build output text, mapping utf16 indices -> byte offsets in the output (attachments expand).
    let mut text = String::with_capacity(raw.len());
    let mut u16_to_byte: Vec<usize> = Vec::with_capacity(raw.len() + 1);
    let mut u16i = 0usize;
    let mut fields = Vec::new();
    for (raw_at, ch) in raw.char_indices() {
        let n16 = ch.len_utf16();
        for _ in 0..n16 {
            u16_to_byte.push(text.len());
        }
        if ch == '\u{FFFC}' {
            let shown_at = text.len();
            let att = attachments.iter().find(|a| a.0 == u16i).and_then(|a| a.1);
            match att.and_then(|id| k.obj(id)) {
                Some(m) if m.type_name() == "TSWP.NumberAttachmentArchive" => {
                    text.push_str(&ctx.slide_number.to_string())
                }
                Some(m) if m.type_name() == "TSWP.DateTimeFieldArchive" => {}
                _ => {} // inline drawables are not rendered inline yet
            }
            fields.push((shown_at, text.len() - shown_at, raw_at));
        } else {
            text.push(ch);
        }
        u16i += n16;
    }
    u16_to_byte.push(text.len());
    let to_byte = |i: usize| u16_to_byte[i.min(u16_to_byte.len() - 1)];
    let to_u16 = |b: usize| u16_to_byte.iter().position(|&x| x >= b).unwrap_or(u16_to_byte.len() - 1);

    // Paragraph ranges (byte offsets in `text`).
    let mut ranges = Vec::new();
    let mut start = 0;
    for (i, ch) in text.char_indices() {
        if ch == '\n' || ch == '\r' || ch == '\u{2029}' {
            ranges.push((start, i));
            start = i + ch.len_utf8();
        }
    }
    ranges.push((start, text.len()));

    let mut counters = [0u32; 10];
    let mut last_list: Option<ObjId> = None;
    let mut paragraphs = Vec::new();
    for (ps, pe) in ranges {
        let u_start = to_u16(ps);
        let pstyle = at(&para_styles, u_start).flatten().or(defaults.para_style);
        let pp: ParaProps = style::para_props(k, pstyle);
        let level = at(&levels, u_start).unwrap_or(0).min(8);
        let list = at(&list_styles, u_start).flatten().or(pp.list_style);

        // runs: split at char-style table boundaries
        let mut cuts: Vec<usize> = vec![ps, pe];
        for (ci, _) in &char_styles {
            let b = to_byte(*ci);
            if b > ps && b < pe {
                cuts.push(b);
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        let mut runs = Vec::new();
        for w in cuts.windows(2) {
            let cstyle = at(&char_styles, to_u16(w[0])).flatten();
            runs.push(Run { start: w[0], end: w[1], style: style::char_style(k, pstyle, cstyle) });
        }
        if runs.is_empty() {
            let cstyle = at(&char_styles, u_start).flatten();
            runs.push(Run { start: ps, end: pe, style: style::char_style(k, pstyle, cstyle) });
        }

        // list label
        let mut label = None;
        let (mut list_indent, mut label_indent, mut label_scale, mut label_color) = (0.0, 0.0, 1.0, None);
        if list.and_then(|id| k.obj(id)).is_some() {
            // every list property may come from the style's parent chain
            let ch = style::chain(k, list);
            let lvl = level as usize;
            let kind = style::lookup(&ch, |m| m.u64s("label_types").get(lvl).copied()).unwrap_or(0);
            list_indent = style::lookup(&ch, |m| m.f32s("indents").get(lvl).copied()).unwrap_or(0.0);
            label_indent = style::lookup(&ch, |m| m.f32s("text_indents").get(lvl).copied()).unwrap_or(0.0);
            label_scale = style::lookup(&ch, |m| m.msgs("geometries").nth(lvl).and_then(|g| g.f32("scale"))).unwrap_or(1.0);
            label_color = style::lookup(&ch, |m| m.msg("font_color").map(style::color));
            let strings: Vec<String> = style::lookup(&ch, |m| {
                let v = m.strs("strings");
                (!v.is_empty()).then(|| v.into_iter().map(String::from).collect())
            })
            .unwrap_or_default();
            let number_types: Vec<u64> = style::lookup(&ch, |m| {
                let v = m.u64s("number_types");
                (!v.is_empty()).then_some(v)
            })
            .unwrap_or_default();
            if last_list != list {
                counters = [0; 10];
                last_list = list;
            }
            match kind {
                2 => {
                    let s = strings.get(lvl).cloned().unwrap_or_else(|| "•".into());
                    label = Some(ListLabel::Bullet(s));
                }
                3 => {
                    counters[lvl] += 1;
                    for c in counters.iter_mut().skip(lvl + 1) {
                        *c = 0;
                    }
                    let nt = number_types.get(lvl).copied().unwrap_or(0);
                    label = Some(ListLabel::Number(format_number(counters[lvl], nt)));
                }
                1 => {
                    if let Some(id) = style::lookup(&ch, |m| m.msgs("images").nth(lvl).cloned()).as_ref().and_then(|i| i.msg("image")).and_then(|d| d.u64("identifier")) {
                        label = Some(ListLabel::Image(id));
                    }
                }
                _ => {}
            }
        }

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
            first_text_indent: None,
            tabs: Vec::new(),
            default_tab: 0.0,
            rtl: pp.rtl,
        });
    }

    // review highlights: TSWP.HighlightArchive -> comment storage -> author colour
    let mut highlights = Vec::new();
    let ht = object_table(st, "table_highlight");
    for (i, (at, obj)) in ht.iter().enumerate() {
        let Some(h) = obj.and_then(|o| k.obj(o)) else { continue };
        let end = ht.get(i + 1).map(|e| e.0).unwrap_or(u16_to_byte.len() - 1);
        let color = h
            .ref_id("commentStorage")
            .and_then(|c| k.obj(c))
            .and_then(|c| c.ref_id("author"))
            .and_then(|a| k.obj(a))
            .and_then(|a| a.msg("color"))
            .map(style::color)
            .unwrap_or(Color { r: 0.98, g: 0.94, b: 0.35, a: 1.0 });
        highlights.push((to_byte(*at), to_byte(end), color));
    }
    let _ = to_u16;
    Some(TextBox {
        storage: storage_id,
        text,
        paragraphs,
        highlights,
        fields,
        valign: layout.valign,
        padding: layout.padding,
        shrink_to_fit: layout.shrink_to_fit,
        columns: layout.columns,
        column_gap: layout.column_gap,
        autogrow: false,
    })
}

/// Plain text of a storage (e.g. presenter notes).
pub fn plain_text(k: &KeyFile, storage_id: ObjId) -> String {
    k.obj(storage_id).map(|s| s.strs("text").concat()).unwrap_or_default().replace('\u{FFFC}', "")
}

fn roman(mut n: u32) -> String {
    const T: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for (v, r) in T {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

fn alpha(n: u32) -> String {
    let mut n = n;
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push((b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    s.iter().rev().collect()
}

/// TSWP.ListStyleArchive.NumberType -> label text.
pub fn format_number(n: u32, number_type: u64) -> String {
    let (body, style) = match number_type {
        0..=2 => (n.to_string(), number_type),
        3..=5 => (roman(n), number_type - 3),
        6..=8 => (roman(n).to_lowercase(), number_type - 6),
        9..=11 => (alpha(n), number_type - 9),
        12..=14 => (alpha(n).to_lowercase(), number_type - 12),
        48 => (char::from_u32(0x2460 + n.saturating_sub(1)).map(String::from).unwrap_or_default(), 3),
        _ => (n.to_string(), 0),
    };
    match style {
        0 => format!("{body}."),
        1 => format!("({body})"),
        2 => format!("{body})"),
        _ => body,
    }
}
