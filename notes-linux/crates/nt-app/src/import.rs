//! File > Import to Notes… with files that aren't a Mac library: plain text, Markdown, HTML, RTF and
//! Evernote's .enex (each of its notes, with their dates), or a folder of them (its tree kept). Read
//! into notes here; dialogs.rs puts them in the account's "Imported Notes" folder.

use nt_core::model::{Body, ParaStyle, Paragraph, Span};
use std::path::Path;

/// One note read from a file.
#[derive(Debug, Default)]
pub struct Item {
    pub body: Body,
    pub created: Option<f64>,
    pub modified: Option<f64>,
}

/// What a folder holds: its notes and its folders (by name), for a folder picked to import.
#[derive(Debug, Default)]
pub struct Tree {
    pub name: String,
    pub items: Vec<Item>,
    pub folders: Vec<Tree>,
}

/// The kinds Import to Notes reads (and the file panel offers).
pub const EXTENSIONS: &[&str] = &["txt", "text", "md", "markdown", "html", "htm", "rtf", "enex"];

pub fn readable(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// The notes in file `p` (a .enex holds many; the others, one).
pub fn read_file(p: &Path) -> Result<Vec<Item>, String> {
    let bytes = std::fs::read(p).map_err(|e| format!("“{}” couldn’t be read ({e}).", p.display()))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let modified = std::fs::metadata(p).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64());
    let mut items = match ext.as_str() {
        "enex" => return Ok(enex(&text)),
        "md" | "markdown" => vec![Item { body: titled(markdown(&text), &stem), ..Item::default() }],
        "html" | "htm" => vec![Item { body: titled(html(&text), &stem), ..Item::default() }],
        "rtf" => vec![Item { body: titled(rtf(&text), &stem), ..Item::default() }],
        _ => vec![Item { body: plain(&text), ..Item::default() }],
    };
    for it in &mut items {
        it.modified = modified;
        it.created = modified;
    }
    Ok(items)
}

/// A folder's readable files and folders (hidden ones left out), sorted by name.
pub fn read_dir(p: &Path) -> Tree {
    let mut t = Tree { name: p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), ..Tree::default() };
    let mut entries: Vec<_> = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).collect();
    entries.sort();
    for e in entries {
        if e.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            continue;
        }
        if e.is_dir() {
            let sub = read_dir(&e);
            if !sub.items.is_empty() || !sub.folders.is_empty() {
                t.folders.push(sub);
            }
        } else if readable(&e) {
            t.items.extend(read_file(&e).unwrap_or_default());
        }
    }
    t
}

/// Plain text: the first line is the title.
pub fn plain(text: &str) -> Body {
    let paragraphs = text.lines().enumerate().map(|(k, l)| Paragraph::plain(if k == 0 { ParaStyle::Title } else { ParaStyle::Body }, l)).collect();
    Body { paragraphs }
}

/// A body that starts with a title: its first line is (Notes' own rule); an empty file gets its name.
fn titled(mut b: Body, name: &str) -> Body {
    while b.paragraphs.first().is_some_and(|p| p.text().trim().is_empty()) {
        b.paragraphs.remove(0);
    }
    match b.paragraphs.first_mut() {
        Some(p) if !matches!(p.style, ParaStyle::Checklist | ParaStyle::Monostyled) => {
            p.style = ParaStyle::Title;
            p.indent = 0;
            p.block_quote = false;
        }
        _ => b.paragraphs.insert(0, Paragraph::plain(ParaStyle::Title, name)),
    }
    b
}

/// Markdown: headings, lists, checklists, block quotes, code blocks, **bold**, *italic*, ~~strike~~
/// and [links](…).
pub fn markdown(text: &str) -> Body {
    let mut paras = Vec::new();
    let mut code = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            code = !code;
            continue;
        }
        if code {
            paras.push(Paragraph::plain(ParaStyle::Monostyled, line));
            continue;
        }
        let lead = line.len() - line.trim_start().len();
        let indent = (line[..lead].replace('\t', "    ").len() / 2).min(8) as u8;
        let mut t = line.trim_start();
        let mut quote = false;
        if let Some(r) = t.strip_prefix("> ").or(t.strip_prefix(">").filter(|r| r.is_empty())) {
            quote = true;
            t = r;
        }
        let mut checked = false;
        let (style, rest) = if let Some(r) = t.strip_prefix("# ") {
            (ParaStyle::Title, r)
        } else if let Some(r) = t.strip_prefix("## ") {
            (ParaStyle::Heading, r)
        } else if let Some(r) = t.strip_prefix("### ").or(t.strip_prefix("#### ")).or(t.strip_prefix("##### ")).or(t.strip_prefix("###### ")) {
            (ParaStyle::Subheading, r)
        } else if let Some(r) = ["- [ ] ", "* [ ] ", "- [x] ", "* [x] ", "- [X] ", "* [X] "].iter().find_map(|p| t.strip_prefix(p)) {
            checked = !t[..5].contains("[ ]");
            (ParaStyle::Checklist, r)
        } else if let Some(r) = t.strip_prefix("* ").or(t.strip_prefix("+ ")) {
            (ParaStyle::Bulleted, r)
        } else if let Some(r) = t.strip_prefix("- ") {
            (ParaStyle::Dashed, r)
        } else if let Some(r) = numbered(t) {
            (ParaStyle::Numbered, r)
        } else if t.trim() == "---" || t.trim() == "***" {
            continue;
        } else {
            (ParaStyle::Body, t)
        };
        let list = matches!(style, ParaStyle::Bulleted | ParaStyle::Dashed | ParaStyle::Numbered | ParaStyle::Checklist);
        paras.push(Paragraph { style, checked, block_quote: quote, indent: if list { indent } else { 0 }, spans: inline_md(rest), ..Paragraph::default() });
    }
    Body { paragraphs: paras }
}

/// "12. rest" → "rest"
fn numbered(t: &str) -> Option<&str> {
    let d = t.bytes().take_while(|b| b.is_ascii_digit()).count();
    (d > 0).then(|| t[d..].strip_prefix(". ").or(t[d..].strip_prefix(") "))).flatten()
}

/// Markdown's inline styles as spans.
fn inline_md(t: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let (mut bold, mut italic, mut strike, mut mono) = (false, false, false, false);
    let mut cur = String::new();
    let flush = |out: &mut Vec<Span>, cur: &mut String, b: bool, i: bool, s: bool, link: Option<String>| {
        if !cur.is_empty() {
            out.push(Span { text: std::mem::take(cur), bold: b, italic: i, strike: s, link, ..Span::default() });
        }
    };
    let c: Vec<char> = t.chars().collect();
    let mut k = 0;
    while k < c.len() {
        let rest: String = c[k..].iter().take(2).collect();
        if mono {
            if c[k] == '`' {
                flush(&mut out, &mut cur, bold, italic, strike, None);
                mono = false;
            } else {
                cur.push(c[k]);
            }
            k += 1;
            continue;
        }
        if c[k] == '\\' && k + 1 < c.len() {
            cur.push(c[k + 1]);
            k += 2;
        } else if rest == "**" || rest == "__" {
            flush(&mut out, &mut cur, bold, italic, strike, None);
            bold = !bold;
            k += 2;
        } else if rest == "~~" {
            flush(&mut out, &mut cur, bold, italic, strike, None);
            strike = !strike;
            k += 2;
        } else if (c[k] == '*' || c[k] == '_') && (italic || c.get(k + 1).is_some_and(|n| !n.is_whitespace())) && (c[k] == '*' || k == 0 || !c[k - 1].is_alphanumeric()) {
            flush(&mut out, &mut cur, bold, italic, strike, None);
            italic = !italic;
            k += 1;
        } else if c[k] == '`' {
            flush(&mut out, &mut cur, bold, italic, strike, None);
            mono = true;
            k += 1;
        } else if c[k] == '[' {
            // [text](url)
            let close = c[k..].iter().position(|&x| x == ']').map(|p| p + k);
            match close.filter(|&e| c.get(e + 1) == Some(&'(')).and_then(|e| c[e..].iter().position(|&x| x == ')').map(|p| (e, p + e))) {
                Some((e, end)) => {
                    flush(&mut out, &mut cur, bold, italic, strike, None);
                    let mut label: String = c[k + 1..e].iter().collect();
                    let url: String = c[e + 2..end].iter().collect();
                    flush(&mut out, &mut label, bold, italic, strike, Some(url));
                    k = end + 1;
                }
                None => {
                    cur.push('[');
                    k += 1;
                }
            }
        } else {
            cur.push(c[k]);
            k += 1;
        }
    }
    flush(&mut out, &mut cur, bold, italic, strike, None);
    out
}

/// HTML (and Evernote's ENML): headings, paragraphs and line breaks, lists, checklists
/// (en-todo and checkbox inputs), block quotes, pre, bold, italic, underline, strike and links.
pub fn html(src: &str) -> Body {
    #[derive(Clone, Copy, PartialEq)]
    enum ListKind {
        Bullet,
        Number,
    }
    let mut paras: Vec<Paragraph> = Vec::new();
    let mut cur = Paragraph::default();
    let mut text = String::new();
    let (mut bold, mut italic, mut under, mut strike) = (0, 0, 0, 0);
    let mut link: Option<String> = None;
    let mut lists: Vec<ListKind> = Vec::new();
    let (mut quote, mut pre, mut skip) = (0, 0, 0);
    let mut style_next: Option<ParaStyle> = None;
    let mut todo: Option<bool> = None;

    let flush_span = |cur: &mut Paragraph, text: &mut String, b: i32, i: i32, u: i32, s: i32, link: &Option<String>| {
        if !text.is_empty() {
            cur.spans.push(Span { text: std::mem::take(text), bold: b > 0, italic: i > 0, underline: u > 0, strike: s > 0, link: link.clone(), ..Span::default() });
        }
    };
    // (`end_para!(true)` keeps an empty line: a <br>)
    macro_rules! end_para {
        ($force:expr) => {{
            flush_span(&mut cur, &mut text, bold, italic, under, strike, &link);
            // (runs of spaces from the source collapse; a paragraph doesn't start or end with them)
            if let Some(f) = cur.spans.first_mut() {
                f.text = f.text.trim_start().to_string();
            }
            if let Some(l) = cur.spans.last_mut() {
                l.text = l.text.trim_end().to_string();
            }
            cur.spans.retain(|s| !s.text.is_empty());
            if !cur.spans.is_empty() || todo.is_some() || $force {
                let mut p = std::mem::take(&mut cur);
                if let Some(t) = todo.take() {
                    p.style = ParaStyle::Checklist;
                    p.checked = t;
                } else if let Some(st) = style_next {
                    p.style = st;
                }
                if matches!(p.style, ParaStyle::Bulleted | ParaStyle::Dashed | ParaStyle::Numbered | ParaStyle::Checklist) {
                    p.indent = lists.len().saturating_sub(1).min(8) as u8;
                }
                p.block_quote = quote > 0;
                if pre > 0 {
                    p.style = ParaStyle::Monostyled;
                }
                paras.push(p);
            }
            cur = Paragraph::default();
        }};
    }

    let b = src.as_bytes();
    let mut k = 0;
    while k < b.len() {
        if b[k] == b'<' {
            if src[k..].starts_with("<!--") {
                k = src[k..].find("-->").map(|e| k + e + 3).unwrap_or(b.len());
                continue;
            }
            if src[k..].starts_with("<![CDATA[") {
                k += 9;
                continue;
            }
            let Some(end) = src[k..].find('>').map(|e| k + e) else { break };
            let tag = &src[k + 1..end];
            k = end + 1;
            let closing = tag.starts_with('/');
            let tag = tag.trim_start_matches('/');
            let name: String = tag.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>().to_ascii_lowercase();
            let attr = |a: &str| -> Option<String> {
                let low = tag.to_ascii_lowercase();
                let at = low.find(&format!("{a}="))? + a.len() + 1;
                let v = &tag[at..];
                let (q, v) = match v.chars().next() {
                    Some(q @ ('"' | '\'')) => (q, &v[1..]),
                    _ => (' ', v),
                };
                Some(decode(v.split(|c| c == q || (q == ' ' && c == '/')).next().unwrap_or("")))
            };
            match name.as_str() {
                "script" | "style" | "head" | "title" => {
                    if closing {
                        skip = (skip - 1).max(0);
                    } else if !tag.ends_with('/') {
                        skip += 1;
                    }
                }
                _ if skip > 0 => {}
                "br" => end_para!(true),
                "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "tr" | "dt" | "dd" | "hr" | "en-note" | "body" | "section" | "article" | "header" | "footer" | "table" => {
                    end_para!(false);
                    let in_item = !lists.is_empty() && name != "li" && style_next.is_some_and(|st| matches!(st, ParaStyle::Bulleted | ParaStyle::Numbered));
                    if closing {
                        // (a <div> closing inside a list item: the rest still belongs to it)
                        if !in_item {
                            style_next = None;
                        }
                    } else if !in_item {
                        style_next = match name.as_str() {
                            "h1" => Some(ParaStyle::Title),
                            "h2" => Some(ParaStyle::Heading),
                            "h3" | "h4" | "h5" | "h6" => Some(ParaStyle::Subheading),
                            "li" => Some(match lists.last() {
                                Some(ListKind::Number) => ParaStyle::Numbered,
                                _ => ParaStyle::Bulleted,
                            }),
                            _ => None,
                        };
                    }
                }
                "ul" | "ol" => {
                    end_para!(false);
                    style_next = None;
                    if closing {
                        lists.pop();
                    } else {
                        lists.push(if name == "ol" { ListKind::Number } else { ListKind::Bullet });
                    }
                }
                "blockquote" => {
                    end_para!(false);
                    quote += if closing { -1 } else { 1 };
                    quote = quote.max(0);
                }
                "pre" => {
                    end_para!(false);
                    pre += if closing { -1 } else { 1 };
                    pre = pre.max(0);
                }
                "td" | "th" if closing => text.push('\t'),
                "b" | "strong" => {
                    flush_span(&mut cur, &mut text, bold, italic, under, strike, &link);
                    bold += if closing { -1 } else { 1 };
                }
                "i" | "em" => {
                    flush_span(&mut cur, &mut text, bold, italic, under, strike, &link);
                    italic += if closing { -1 } else { 1 };
                }
                "u" => {
                    flush_span(&mut cur, &mut text, bold, italic, under, strike, &link);
                    under += if closing { -1 } else { 1 };
                }
                "s" | "strike" | "del" => {
                    flush_span(&mut cur, &mut text, bold, italic, under, strike, &link);
                    strike += if closing { -1 } else { 1 };
                }
                "a" => {
                    flush_span(&mut cur, &mut text, bold, italic, under, strike, &link);
                    link = if closing { None } else { attr("href") };
                }
                "en-todo" => todo = Some(attr("checked").is_some_and(|v| v == "true")),
                "input" if attr("type").is_some_and(|t| t.eq_ignore_ascii_case("checkbox")) => todo = Some(tag.to_ascii_lowercase().contains("checked")),
                _ => {}
            }
            continue;
        }
        // text up to the next tag
        let next = src[k..].find('<').map(|e| k + e).unwrap_or(b.len());
        let raw = &src[k..next];
        k = next;
        if skip > 0 {
            continue;
        }
        let raw = raw.strip_suffix("]]>").unwrap_or(raw);
        let t = decode(raw);
        if pre > 0 {
            for (n, line) in t.split('\n').enumerate() {
                if n > 0 {
                    end_para!(true);
                }
                text.push_str(line);
            }
        } else {
            for ch in t.chars() {
                let ch = if ch.is_whitespace() && ch != '\u{a0}' { ' ' } else { ch };
                if ch == ' ' && (text.ends_with(' ') || (text.is_empty() && cur.spans.last().is_none_or(|s| s.text.ends_with(' ')))) {
                    continue;
                }
                text.push(ch);
            }
        }
    }
    end_para!(false);
    while paras.last().is_some_and(|p| p.spans.is_empty()) {
        paras.pop();
    }
    Body { paragraphs: paras }
}

/// HTML character references.
fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find('&') {
        out.push_str(&rest[..a]);
        rest = &rest[a..];
        let Some(e) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..e];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            "lsquo" => Some('‘'),
            "rsquo" => Some('’'),
            "ldquo" => Some('“'),
            "rdquo" => Some('”'),
            "bull" => Some('•'),
            "copy" => Some('©'),
            _ => name.strip_prefix("#x").or(name.strip_prefix("#X")).and_then(|h| u32::from_str_radix(h, 16).ok()).or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok())).and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Evernote's export: each <note> with its title, ENML content and dates.
pub fn enex(src: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(a) = rest.find("<note>") {
        let Some(e) = rest[a..].find("</note>").map(|e| a + e) else { break };
        let note = &rest[a + 6..e];
        rest = &rest[e + 7..];
        let field = |n: &str| -> Option<&str> {
            let s = note.find(&format!("<{n}>"))? + n.len() + 2;
            let e = note[s..].find(&format!("</{n}>"))? + s;
            Some(&note[s..e])
        };
        let title = decode(field("title").unwrap_or("").trim());
        let content = field("content").unwrap_or("");
        let content = content.trim().strip_prefix("<![CDATA[").unwrap_or(content);
        let content = content.trim_end().strip_suffix("]]>").unwrap_or(content);
        let mut body = html(content);
        if body.paragraphs.first().is_none_or(|p| p.text().trim() != title.trim()) {
            body.paragraphs.insert(0, Paragraph::plain(ParaStyle::Title, &title));
        } else {
            body.paragraphs[0].style = ParaStyle::Title;
        }
        let created = field("created").and_then(enex_date);
        let modified = field("updated").and_then(enex_date).or(created);
        out.push(Item { body, created, modified });
    }
    out
}

/// "20231105T142233Z" → Unix seconds.
fn enex_date(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.len() < 15 || s.as_bytes()[8] != b'T' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<i64>().ok());
    let (y, m, d) = (n(0..4)?, n(4..6)?, n(6..8)?);
    let (hh, mm, ss) = (n(9..11)?, n(11..13)?, n(13..15)?);
    // days from 1970-01-01 (Howard Hinnant's civil algorithm)
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days * 86400 + hh * 3600 + mm * 60 + ss) as f64)
}

/// RTF: its text, paragraphs, bold, italic, underline and strike (fonts, colours and pictures left out).
pub fn rtf(src: &str) -> Body {
    #[derive(Clone, Copy, Default)]
    struct St {
        b: bool,
        i: bool,
        u: bool,
        s: bool,
        skip: bool,
        uc: usize,
    }
    let mut stack: Vec<St> = Vec::new();
    let mut st = St { uc: 1, ..St::default() };
    let mut paras = Vec::new();
    let mut cur: Vec<Span> = Vec::new();
    let mut text = String::new();
    let mut last = St::default();
    let mut skip_chars = 0usize;
    let c: Vec<char> = src.chars().collect();
    let mut k = 0;
    let push = |cur: &mut Vec<Span>, text: &mut String, s: &St| {
        if !text.is_empty() {
            cur.push(Span { text: std::mem::take(text), bold: s.b, italic: s.i, underline: s.u, strike: s.s, ..Span::default() });
        }
    };
    let mut emit = |ch: char, st: &St, last: &mut St, cur: &mut Vec<Span>, text: &mut String| {
        if (st.b, st.i, st.u, st.s) != (last.b, last.i, last.u, last.s) {
            push(cur, text, last);
            *last = *st;
        }
        text.push(ch);
    };
    while k < c.len() {
        match c[k] {
            '{' => {
                stack.push(st);
                k += 1;
                if c.get(k) == Some(&'\\') && c.get(k + 1) == Some(&'*') {
                    st.skip = true;
                }
            }
            '}' => {
                st = stack.pop().unwrap_or_default();
                k += 1;
            }
            '\\' => {
                k += 1;
                let Some(&n) = c.get(k) else { break };
                if !n.is_ascii_alphabetic() {
                    k += 1;
                    match n {
                        '\'' => {
                            let h: String = c[k..(k + 2).min(c.len())].iter().collect();
                            k += 2;
                            if skip_chars > 0 {
                                skip_chars -= 1;
                            } else if !st.skip {
                                if let Ok(v) = u8::from_str_radix(&h, 16) {
                                    emit(cp1252(v), &st, &mut last, &mut cur, &mut text);
                                }
                            }
                        }
                        '\\' | '{' | '}' if !st.skip => emit(n, &st, &mut last, &mut cur, &mut text),
                        '~' if !st.skip => emit('\u{a0}', &st, &mut last, &mut cur, &mut text),
                        '\n' | '\r' if !st.skip => {
                            push(&mut cur, &mut text, &last);
                            paras.push(std::mem::take(&mut cur));
                        }
                        _ => {}
                    }
                    continue;
                }
                let w0 = k;
                while k < c.len() && c[k].is_ascii_alphabetic() {
                    k += 1;
                }
                let word: String = c[w0..k].iter().collect();
                let n0 = k;
                if k < c.len() && (c[k] == '-' || c[k].is_ascii_digit()) {
                    k += 1;
                    while k < c.len() && c[k].is_ascii_digit() {
                        k += 1;
                    }
                }
                let num: Option<i64> = (k > n0).then(|| c[n0..k].iter().collect::<String>().parse().ok()).flatten();
                if c.get(k) == Some(&' ') {
                    k += 1;
                }
                let on = num != Some(0);
                match word.as_str() {
                    "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict" | "header" | "footer" | "headerl" | "headerr" | "footerl" | "footerr" | "listtable" | "listoverridetable" | "rsidtbl" | "generator" | "xmlnstbl" | "themedata" | "colorschememapping" | "datastore" | "latentstyles" | "expandedcolortbl" | "object" | "fldinst" => st.skip = true,
                    _ if st.skip => {}
                    "par" | "line" | "row" => {
                        push(&mut cur, &mut text, &last);
                        paras.push(std::mem::take(&mut cur));
                    }
                    "tab" | "cell" => emit('\t', &st, &mut last, &mut cur, &mut text),
                    "b" => st.b = on,
                    "i" => st.i = on,
                    "ul" => st.u = on,
                    "ulnone" => st.u = false,
                    "strike" => st.s = on,
                    "plain" => (st.b, st.i, st.u, st.s) = (false, false, false, false),
                    "uc" => st.uc = num.unwrap_or(1).max(0) as usize,
                    "u" => {
                        if let Some(v) = num {
                            let v = if v < 0 { v + 65536 } else { v } as u32;
                            if let Some(ch) = char::from_u32(v) {
                                emit(ch, &st, &mut last, &mut cur, &mut text);
                            }
                            skip_chars = st.uc;
                        }
                    }
                    "emdash" => emit('—', &st, &mut last, &mut cur, &mut text),
                    "endash" => emit('–', &st, &mut last, &mut cur, &mut text),
                    "bullet" => emit('•', &st, &mut last, &mut cur, &mut text),
                    "lquote" => emit('‘', &st, &mut last, &mut cur, &mut text),
                    "rquote" => emit('’', &st, &mut last, &mut cur, &mut text),
                    "ldblquote" => emit('“', &st, &mut last, &mut cur, &mut text),
                    "rdblquote" => emit('”', &st, &mut last, &mut cur, &mut text),
                    _ => {}
                }
            }
            '\n' | '\r' => k += 1,
            ch => {
                k += 1;
                if skip_chars > 0 {
                    skip_chars -= 1;
                } else if !st.skip {
                    emit(ch, &st, &mut last, &mut cur, &mut text);
                }
            }
        }
    }
    push(&mut cur, &mut text, &last);
    if !cur.is_empty() {
        paras.push(cur);
    }
    while paras.last().is_some_and(|p| p.is_empty()) {
        paras.pop();
    }
    Body { paragraphs: paras.into_iter().map(|spans| Paragraph { spans, ..Paragraph::default() }).collect() }
}

/// Windows-1252, which RTF's \'hh escapes mostly are.
fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = ['€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ'];
    if (0x80..0xa0).contains(&b) {
        HIGH[(b - 0x80) as usize]
    } else {
        b as char
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(b: &Body) -> Vec<(ParaStyle, String)> {
        b.paragraphs.iter().map(|p| (p.style, p.text())).collect()
    }

    #[test]
    fn reads_html() {
        let b = html("<html><head><title>x</title><style>p{}</style></head><body><h1>Trip</h1><p>Pack <b>light</b> &amp; early</p><ul><li>Socks</li><li>Hat<ul><li>Wool</li></ul></li></ul><ol><li>One</li></ol><p><input type=checkbox checked> Passport</p><blockquote>Go</blockquote><p>See <a href=\"https://x.org\">this</a></p></body></html>");
        let t = texts(&b);
        assert_eq!(t[0], (ParaStyle::Title, "Trip".into()));
        assert_eq!(t[1], (ParaStyle::Body, "Pack light & early".into()));
        assert!(b.paragraphs[1].spans.iter().any(|s| s.bold && s.text == "light"));
        assert_eq!(t[2], (ParaStyle::Bulleted, "Socks".into()));
        assert_eq!(t[3], (ParaStyle::Bulleted, "Hat".into()));
        assert_eq!((t[4].clone(), b.paragraphs[4].indent), ((ParaStyle::Bulleted, "Wool".into()), 1));
        assert_eq!(t[5], (ParaStyle::Numbered, "One".into()));
        assert_eq!(t[6], (ParaStyle::Checklist, "Passport".into()));
        assert!(b.paragraphs[6].checked);
        assert!(b.paragraphs[7].block_quote);
        assert_eq!(b.paragraphs[8].spans.last().and_then(|s| s.link.clone()).as_deref(), Some("https://x.org"));
    }

    #[test]
    fn reads_enex() {
        let src = r#"<?xml version="1.0"?><en-export><note><title>Groceries</title><content><![CDATA[<?xml version="1.0"?><!DOCTYPE en-note><en-note><div><en-todo checked="true"/>Milk</div><div><en-todo/>Eggs</div></en-note>]]></content><created>20231105T142233Z</created><updated>20231106T000000Z</updated><tag>food</tag></note><note><title>Two</title><content><![CDATA[<en-note>Hi</en-note>]]></content></note></en-export>"#;
        let n = enex(src);
        assert_eq!(n.len(), 2);
        let t = texts(&n[0].body);
        assert_eq!(t, vec![(ParaStyle::Title, "Groceries".into()), (ParaStyle::Checklist, "Milk".into()), (ParaStyle::Checklist, "Eggs".into())]);
        assert!(n[0].body.paragraphs[1].checked && !n[0].body.paragraphs[2].checked);
        assert_eq!(n[0].created, Some(1699194153.0));
        assert_eq!(n[0].modified, Some(1699228800.0));
        assert_eq!(texts(&n[1].body)[1].1, "Hi");
    }

    #[test]
    fn reads_rtf() {
        let b = rtf(r"{\rtf1\ansi\ansicpg1252{\fonttbl\f0\fswiss Helvetica;}{\colortbl;\red255\green255\blue255;}\f0\fs24 Hello \b bold\b0  caf\'e9\par Second \u8212? line\par}");
        let t = texts(&b);
        assert_eq!(t[0].1, "Hello bold café");
        assert!(b.paragraphs[0].spans.iter().any(|s| s.bold && s.text == "bold"));
        assert_eq!(t[1].1, "Second — line");
    }

    #[test]
    fn reads_markdown() {
        let b = markdown("# Plan\nSome **bold** and *it* and [a link](https://a.b)\n- [x] done\n  * nested\n1. first\n```\ncode\n```");
        let t = texts(&b);
        assert_eq!(t[0], (ParaStyle::Title, "Plan".into()));
        assert_eq!(t[1].1, "Some bold and it and a link");
        assert!(b.paragraphs[1].spans.iter().any(|s| s.italic && s.text == "it"));
        assert!(b.paragraphs[2].checked && b.paragraphs[2].style == ParaStyle::Checklist);
        assert_eq!((b.paragraphs[3].style, b.paragraphs[3].indent), (ParaStyle::Bulleted, 1));
        assert_eq!(t[4], (ParaStyle::Numbered, "first".into()));
        assert_eq!(t[5], (ParaStyle::Monostyled, "code".into()));
    }
}
