//! Note bodies: ZICNOTEDATA.ZDATA = gzip(protobuf) holding a topotext string with attribute runs
//! (format section 3) <-> [`model::Body`]. Also table cells (same topotext) and the collapsed-heading
//! state (ICNote.outlineStateData, format 2.5).
//!
//! Field numbers follow apple_cloud_notes_parser's notestore.proto (MIT, see THIRD-PARTY.md) and the
//! NO1 research.

use kn_format::Msg;

use crate::model::{Align, Body, Font, ParaStyle, Paragraph, Span, Token, OBJ};
use crate::pb::{self, Result};

/// Seconds between 1970-01-01 and 2001-01-01 (Core Data's reference date).
pub const APPLE_EPOCH: f64 = 978_307_200.0;

pub fn now_unix() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

// ------------------------------------------------------------------ decoding

/// ZICNOTEDATA.ZDATA (gzipped or raw) -> body.
pub fn decode_note(data: &[u8]) -> Result<Body> {
    let root = pb::parse(&pb::gunzip(data)?)?;
    let doc = pb::sub(&root, 2).ok_or_else(|| crate::Error::Format("note data has no document".into()))?;
    let note = pb::sub(&doc, 3).ok_or_else(|| crate::Error::Format("note document has no text".into()))?;
    Ok(decode_topotext(&note))
}

/// The plain text (U+FFFC for attachments) of a note document, without decoding runs.
pub fn note_text(data: &[u8]) -> Result<String> {
    let root = pb::parse(&pb::gunzip(data)?)?;
    Ok(pb::sub(&root, 2).and_then(|d| pb::sub(&d, 3)).and_then(|n| pb::string(&n, 2)).unwrap_or_default())
}

/// One topotext String message (note or table cell) -> body.
pub fn decode_topotext(note: &Msg) -> Body {
    let text = pb::string(note, 2).unwrap_or_default();
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut paras: Vec<Paragraph> = Vec::new();
    let mut cur = Paragraph::default();
    // the paragraph style of the last run that touched the current paragraph
    let mut cur_style: Option<Msg> = None;
    let mut pos = 0usize;

    let feed = |seg: &str, run: Option<&Msg>, cur: &mut Paragraph, cur_style: &mut Option<Msg>, paras: &mut Vec<Paragraph>| {
        let para_style = run.and_then(|r| pb::sub(r, 2)).unwrap_or_default();
        let mut parts = seg.split('\n').peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                *cur_style = Some(para_style.clone());
                push_text(cur, part, run);
            }
            if parts.peek().is_some() {
                // a '\n': the paragraph ends and takes this run's paragraph style
                apply_para_style(cur, &para_style);
                paras.push(std::mem::take(cur));
                *cur_style = None;
            }
        }
    };

    for run in pb::subs(note, 5) {
        let len = pb::uint(&run, 1).unwrap_or(0) as usize;
        let end = (pos + len).min(units.len());
        let seg = String::from_utf16_lossy(&units[pos..end]);
        pos = end;
        feed(&seg, Some(&run), &mut cur, &mut cur_style, &mut paras);
    }
    if pos < units.len() {
        // runs that don't cover the whole string (shouldn't happen): the rest is plain text
        let seg = String::from_utf16_lossy(&units[pos..]);
        feed(&seg, None, &mut cur, &mut cur_style, &mut paras);
    }
    if let Some(st) = cur_style {
        apply_para_style(&mut cur, &st);
    }
    paras.push(cur);
    Body { paragraphs: paras }
}

fn push_text(p: &mut Paragraph, text: &str, run: Option<&Msg>) {
    let base = run.map(span_attrs).unwrap_or_default();
    let info = run.and_then(|r| pb::sub(r, 12));
    if let Some(info) = info {
        // an attachment / inline token run: one span per U+FFFC
        let tok = Token { id: pb::string(&info, 1).unwrap_or_default(), uti: pb::string(&info, 2).unwrap_or_default(), ..Token::default() };
        let mut plain = String::new();
        for c in text.chars() {
            if c == OBJ {
                if !plain.is_empty() {
                    push_span(p, Span { text: std::mem::take(&mut plain), ..base.clone() });
                }
                p.spans.push(Span { text: OBJ.to_string(), token: Some(tok.clone()), ..base.clone() });
            } else {
                plain.push(c);
            }
        }
        if !plain.is_empty() {
            push_span(p, Span { text: plain, ..base });
        }
    } else {
        push_span(p, Span { text: text.to_string(), ..base });
    }
}

/// Append, merging with the previous span when the style is the same.
fn push_span(p: &mut Paragraph, s: Span) {
    if let Some(last) = p.spans.last_mut() {
        if last.same_style(&s) {
            last.text.push_str(&s.text);
            return;
        }
    }
    p.spans.push(s);
}

fn span_attrs(run: &Msg) -> Span {
    let weight = pb::uint(run, 5).unwrap_or(0);
    let font = pb::sub(run, 3).map(|f| Font {
        name: pb::string(&f, 1),
        size: pb::f32_(&f, 2),
        hints: pb::uint(&f, 3).map(|v| v as u32),
    });
    let color = pb::sub(run, 10).map(|c| {
        [pb::f32_(&c, 1).unwrap_or(0.0), pb::f32_(&c, 2).unwrap_or(0.0), pb::f32_(&c, 3).unwrap_or(0.0), pb::f32_(&c, 4).unwrap_or(1.0)]
    });
    Span {
        text: String::new(),
        bold: weight == 1 || weight == 3,
        italic: weight == 2 || weight == 3,
        underline: pb::uint(run, 6).unwrap_or(0) != 0,
        strike: pb::uint(run, 7).unwrap_or(0) != 0,
        script: (pb::int(run, 8).unwrap_or(0) as i32).signum() as i8,
        link: pb::string(run, 9),
        highlight: pb::uint(run, 14).filter(|&v| v > 0).map(|v| v as u8),
        color,
        font,
        token: None,
    }
}

fn apply_para_style(p: &mut Paragraph, st: &Msg) {
    p.style = match pb::uint(st, 1) {
        Some(0) => ParaStyle::Title,
        Some(1) => ParaStyle::Heading,
        Some(2) => ParaStyle::Subheading,
        Some(4) => ParaStyle::Monostyled,
        Some(100) => ParaStyle::Bulleted,
        Some(101) => ParaStyle::Dashed,
        Some(102) => ParaStyle::Numbered,
        Some(103) => ParaStyle::Checklist,
        _ => ParaStyle::Body,
    };
    p.align = match pb::uint(st, 2) {
        Some(0) => Align::Left,
        Some(1) => Align::Center,
        Some(2) => Align::Right,
        Some(3) => Align::Justified,
        _ => Align::Natural,
    };
    p.direction = pb::uint(st, 3).map(|v| v as u32);
    p.indent = pb::uint(st, 4).unwrap_or(0).min(u8::MAX as u64) as u8;
    if let Some(cl) = pb::sub(st, 5) {
        p.check_id = pb::bytes(&cl, 1).map(|b| pb::uuid_str(&b)).unwrap_or_default();
        p.checked = pb::uint(&cl, 2).unwrap_or(0) != 0;
    }
    p.start_number = pb::uint(st, 7).map(|v| v as u32);
    p.block_quote = pb::uint(st, 8).unwrap_or(0) != 0;
    p.id = pb::bytes(st, 9).map(|b| pb::uuid_str(&b)).unwrap_or_default();
}

// ------------------------------------------------------------------ encoding

/// Body -> ZICNOTEDATA.ZDATA (gzipped). Paragraphs without an id are written without one (as Notes
/// does for imported text); call [`assign_ids`] first for new paragraphs.
pub fn encode_note(body: &Body) -> Vec<u8> {
    pb::gzip(&encode_note_raw(body))
}

/// Body -> the note document protobuf (not gzipped).
pub fn encode_note_raw(body: &Body) -> Vec<u8> {
    let replica = pb::new_uuid_bytes();
    let note = encode_topotext(body, &replica, 1, 0, true);
    let mut doc = pb::of_u(&[(1, 0), (2, 0)]);
    pb::put_m(&mut doc, 3, note);
    let mut root = pb::of_u(&[(1, 0)]);
    pb::put_m(&mut root, 2, doc);
    root.encode()
}

fn char_id(replica: u64, clock: u64) -> Msg {
    pb::of_u(&[(1, replica), (2, clock)])
}

/// The topotext CRDT substrings for text of `len` UTF-16 units typed in one go by replica index
/// `replica` (1-based) starting at `clock`: start sentinel, one live substring, end sentinel.
pub fn substrings(len: u64, replica: u64, clock: u64) -> Vec<Msg> {
    let mut start = pb::new();
    pb::put_m(&mut start, 1, char_id(0, 0));
    pb::put_u(&mut start, 2, 0);
    pb::put_m(&mut start, 3, char_id(0, 0));
    let mut out = Vec::new();
    if len > 0 {
        pb::put_u(&mut start, 5, 1);
        out.push(start);
        let mut live = pb::new();
        pb::put_m(&mut live, 1, char_id(replica, clock));
        pb::put_u(&mut live, 2, len);
        pb::put_m(&mut live, 3, char_id(replica, 0));
        pb::put_u(&mut live, 5, 2);
        out.push(live);
    } else {
        pb::put_u(&mut start, 5, 1);
        out.push(start);
    }
    let mut end = pb::new();
    pb::put_m(&mut end, 1, char_id(0, 0xFFFF_FFFF));
    pb::put_u(&mut end, 2, 0);
    pb::put_m(&mut end, 3, char_id(0, 0xFFFF_FFFF));
    out.push(end);
    out
}

/// A VectorTimestamp with one replica at `clock`.
pub fn vector_timestamp(replica: &[u8; 16], clock: u64) -> Msg {
    let mut r = pb::new();
    pb::put_b(&mut r, 1, replica);
    pb::put_m(&mut r, 2, pb::of_u(&[(1, clock)]));
    pb::put_m(&mut r, 2, pb::of_u(&[(1, 1)]));
    let mut v = pb::new();
    pb::put_m(&mut v, 1, r);
    v
}

/// Body -> topotext String message. `with_clock` adds the VectorTimestamp (notes have one, table
/// cells use the table's).
pub fn encode_topotext(body: &Body, replica: &[u8; 16], replica_index: u64, clock: u64, with_clock: bool) -> Msg {
    let text = body.text();
    let len = text.encode_utf16().count() as u64;
    let ts = now_unix() as u64;
    let mut m = pb::new();
    pb::put_s(&mut m, 2, &text);
    for s in substrings(len, replica_index, clock) {
        pb::put_m(&mut m, 3, s);
    }
    if with_clock {
        pb::put_m(&mut m, 4, vector_timestamp(replica, clock + len));
    }
    // (attributes without the length, length) — merged when equal
    let mut runs: Vec<(Msg, u64)> = Vec::new();
    let mut push = |attrs: Msg, n: u64| {
        if n == 0 {
            return;
        }
        if let Some((last, ln)) = runs.last_mut() {
            if *last == attrs {
                *ln += n;
                return;
            }
        }
        runs.push((attrs, n));
    };
    let count = body.paragraphs.len();
    for (i, p) in body.paragraphs.iter().enumerate() {
        let ps = para_style_msg(p);
        for s in &p.spans {
            push(run_attrs(&ps, s, ts), s.text.encode_utf16().count() as u64);
        }
        if i + 1 < count {
            // the '\n' carries the paragraph's style and the last span's character style (not its link)
            let nl = match p.spans.last() {
                Some(s) if s.token.is_none() => Span { text: String::new(), link: None, ..s.clone() },
                _ => Span::default(),
            };
            push(run_attrs(&ps, &nl, ts), 1);
        }
    }
    for (attrs, n) in runs {
        let mut r = pb::of_u(&[(1, n)]);
        r.fields.extend(attrs.fields);
        pb::put_m(&mut m, 5, r);
    }
    m
}

fn para_style_msg(p: &Paragraph) -> Msg {
    let mut m = pb::new();
    let ty = match p.style {
        ParaStyle::Title => Some(0),
        ParaStyle::Heading => Some(1),
        ParaStyle::Subheading => Some(2),
        ParaStyle::Body => None,
        ParaStyle::Monostyled => Some(4),
        ParaStyle::Bulleted => Some(100),
        ParaStyle::Dashed => Some(101),
        ParaStyle::Numbered => Some(102),
        ParaStyle::Checklist => Some(103),
    };
    if let Some(t) = ty {
        pb::put_u(&mut m, 1, t);
    }
    let al = match p.align {
        Align::Natural => None,
        Align::Left => Some(0),
        Align::Center => Some(1),
        Align::Right => Some(2),
        Align::Justified => Some(3),
    };
    if let Some(a) = al {
        pb::put_u(&mut m, 2, a);
    }
    if let Some(d) = p.direction {
        pb::put_u(&mut m, 3, d.into());
    }
    if p.indent > 0 {
        pb::put_u(&mut m, 4, p.indent.into());
    }
    if p.style == ParaStyle::Checklist {
        let mut cl = pb::new();
        let id = pb::uuid_bytes(&p.check_id).unwrap_or_else(pb::new_uuid_bytes);
        pb::put_b(&mut cl, 1, &id);
        pb::put_u(&mut cl, 2, p.checked.into());
        pb::put_m(&mut m, 5, cl);
    }
    if let Some(n) = p.start_number {
        pb::put_u(&mut m, 7, n.into());
    }
    if p.block_quote {
        pb::put_u(&mut m, 8, 1);
    }
    if let Some(id) = pb::uuid_bytes(&p.id) {
        pb::put_b(&mut m, 9, &id);
    }
    m
}

fn run_attrs(ps: &Msg, s: &Span, ts: u64) -> Msg {
    let mut m = pb::new();
    if !ps.fields.is_empty() {
        pb::put_m(&mut m, 2, ps.clone());
    }
    if let Some(f) = &s.font {
        let mut fm = pb::new();
        if let Some(n) = &f.name {
            pb::put_s(&mut fm, 1, n);
        }
        if let Some(z) = f.size {
            pb::put_f32(&mut fm, 2, z);
        }
        if let Some(h) = f.hints {
            pb::put_u(&mut fm, 3, h.into());
        }
        pb::put_m(&mut m, 3, fm);
    }
    let weight = match (s.bold, s.italic) {
        (true, true) => 3,
        (true, false) => 1,
        (false, true) => 2,
        _ => 0,
    };
    if weight > 0 {
        pb::put_u(&mut m, 5, weight);
    }
    if s.underline {
        pb::put_u(&mut m, 6, 1);
    }
    if s.strike {
        pb::put_u(&mut m, 7, 1);
    }
    if s.script != 0 {
        pb::put_i(&mut m, 8, s.script.into());
    }
    if let Some(l) = &s.link {
        pb::put_s(&mut m, 9, l);
    }
    if let Some(c) = s.color {
        let mut cm = pb::new();
        for (i, v) in c.iter().enumerate() {
            pb::put_f32(&mut cm, i as u32 + 1, *v);
        }
        pb::put_m(&mut m, 10, cm);
    }
    if let Some(t) = &s.token {
        let mut info = pb::new();
        pb::put_s(&mut info, 1, &t.id);
        pb::put_s(&mut info, 2, &t.uti);
        pb::put_m(&mut m, 12, info);
    }
    pb::put_u(&mut m, 13, ts);
    if let Some(h) = s.highlight {
        pb::put_u(&mut m, 14, h.into());
    }
    m
}

/// Make sure every paragraph (and checklist item) has an id, so the encoder writes stable ones.
/// An empty last paragraph is left alone: it has no text to carry attributes.
pub fn assign_ids(body: &mut Body) {
    let n = body.paragraphs.len();
    for (i, p) in body.paragraphs.iter_mut().enumerate() {
        if i + 1 == n && p.spans.iter().all(|s| s.text.is_empty()) && n > 1 {
            continue;
        }
        if pb::uuid_bytes(&p.id).is_none() {
            p.id = pb::new_uuid();
        }
        if p.style == ParaStyle::Checklist && pb::uuid_bytes(&p.check_id).is_none() {
            p.check_id = pb::new_uuid();
        }
    }
}

// ------------------------------------------------------------------ collapsed headings

/// ICNote.outlineStateData -> the collapsed paragraphs' uuids (upper-case).
pub fn decode_outline_state(data: &[u8]) -> Vec<String> {
    let Ok(m) = pb::gunzip(data).and_then(|b| pb::parse(&b)) else { return Vec::new() };
    let json = pb::sub(&m, 2).and_then(|s| pb::string(&s, 7)).or_else(|| pb::string(&m, 7));
    json.and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok()).unwrap_or_default().into_iter().map(|s| s.to_uppercase()).collect()
}

/// The collapsed headings of a body -> ICNote.outlineStateData (None when nothing is collapsed).
pub fn encode_outline_state(body: &Body) -> Option<Vec<u8>> {
    let ids: Vec<&str> = body.paragraphs.iter().filter(|p| p.collapsed && !p.id.is_empty()).map(|p| p.id.as_str()).collect();
    if ids.is_empty() {
        return None;
    }
    let mut inner = pb::new();
    pb::put_s(&mut inner, 7, &serde_json::to_string(&ids).unwrap_or_default());
    let mut m = pb::new();
    pb::put_f64(&mut m, 1, now_unix() - APPLE_EPOCH);
    pb::put_m(&mut m, 2, inner);
    Some(m.encode())
}

/// Mark the paragraphs whose ids are in `collapsed`.
pub fn apply_outline_state(body: &mut Body, collapsed: &[String]) {
    for p in &mut body.paragraphs {
        p.collapsed = !p.id.is_empty() && collapsed.iter().any(|c| c.eq_ignore_ascii_case(&p.id));
    }
}

// ------------------------------------------------------------------ title / snippet

/// Notes' title (first line) and snippet (the next non-empty line), with tokens as their alt text.
pub fn title_and_snippet(body: &Body) -> (String, String) {
    let line = |p: &Paragraph| -> String {
        p.spans
            .iter()
            .map(|s| match &s.token {
                Some(t) if t.is_inline() => t.alt.clone(),
                Some(_) => String::new(),
                None => s.text.clone(),
            })
            .collect::<String>()
            .trim()
            .to_string()
    };
    let mut lines = body.paragraphs.iter().map(line);
    let title = lines.next().unwrap_or_default();
    let snippet = lines.find(|l| !l.is_empty()).unwrap_or_default();
    (title, snippet)
}
