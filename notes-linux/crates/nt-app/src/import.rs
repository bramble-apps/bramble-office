//! File > Import to Notes… with files that aren't a Mac library: plain text, Markdown, HTML, RTF and
//! Evernote's .enex (each of its notes, with their dates), or a folder of them (its tree kept). Read
//! into notes here, each with its own dates (an .enex's, Markdown front matter's, an HTML export's
//! <meta>, else the file's creation and change times); dialogs.rs puts them in the account's
//! "Imported Notes" folder.

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
    if ext == "enex" {
        return Ok(enex(&text));
    }
    // the dates: the file's own (front matter, an HTML export's <meta>), else the file's
    let (mut created, mut modified) = (None, None);
    let body = match ext.as_str() {
        "md" | "markdown" => {
            let (meta, rest) = front_matter(&text);
            let get = |keys: &[&str]| meta.iter().find(|(k, _)| keys.contains(&k.as_str())).and_then(|(_, v)| parse_date(v));
            created = get(&["created", "date", "created_at", "createdat", "creation_date", "date_created", "created_time"]);
            modified = get(&["updated", "modified", "updated_at", "updatedat", "last_modified", "lastmod", "date_modified", "modified_at", "updated_time"]);
            let mut b = markdown(rest);
            // (a front-matter title when the text has none of its own)
            if let Some((_, t)) = meta.iter().find(|(k, _)| k == "title").filter(|(_, t)| !t.is_empty()) {
                if b.paragraphs.iter().find(|p| !p.text().trim().is_empty()).is_none_or(|p| p.style != ParaStyle::Title) {
                    b.paragraphs.insert(0, Paragraph::plain(ParaStyle::Title, t));
                }
            }
            titled(b, &stem)
        }
        "html" | "htm" => {
            created = html_meta(&text, &["created", "date", "dcterms.created"]).and_then(|v| parse_date(&v));
            modified = html_meta(&text, &["updated", "modified", "dcterms.modified", "last-modified"]).and_then(|v| parse_date(&v));
            titled(html(&text), &stem)
        }
        "rtf" => titled(rtf(&text), &stem),
        _ => plain(&text),
    };
    let (born, changed) = file_times(p);
    let created = created.or(born).or(changed);
    let modified = modified.or(changed).or(created).map(|m| created.map_or(m, |c| m.max(c)));
    Ok(vec![Item { body, created, modified }])
}

/// A file's creation (birth) time where the system keeps one, and its last change, in Unix seconds.
pub fn file_times(p: &Path) -> (Option<f64>, Option<f64>) {
    let Ok(m) = std::fs::metadata(p) else { return (None, None) };
    let secs = |t: std::io::Result<std::time::SystemTime>| t.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64());
    let changed = secs(m.modified());
    // (a copied file's birth can be after its last change: then the change is the older date)
    let born = secs(m.created()).map(|b| changed.map_or(b, |c| b.min(c)));
    (born, changed)
}

/// Markdown's front matter (a leading `---` … `---` block of `key: value` lines, as Obsidian, Bear,
/// Joplin and static-site exports write it): its keys (lower case) and values, and the text after it.
pub fn front_matter(text: &str) -> (Vec<(String, String)>, &str) {
    let t = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(after) = t.strip_prefix("---\n").or_else(|| t.strip_prefix("---\r\n")) else { return (Vec::new(), text) };
    let mut meta = Vec::new();
    let mut at = 0;
    for line in after.split_inclusive('\n') {
        at += line.len();
        let l = line.trim_end();
        if l == "---" || l == "..." {
            return (meta, &after[at..]);
        }
        if let Some((k, v)) = l.split_once(':').filter(|(k, _)| !k.starts_with(' ') && !k.starts_with('-')) {
            let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
            meta.push((k.trim().to_ascii_lowercase(), v.to_string()));
        }
    }
    // (no closing line: not front matter)
    (Vec::new(), text)
}

/// `<meta name="created" content="…">` in an HTML file (Evernote's and other apps' HTML exports).
fn html_meta(src: &str, names: &[&str]) -> Option<String> {
    let lower = src.to_ascii_lowercase();
    let head = &lower[..lower.find("</head>").unwrap_or(lower.len())];
    let mut rest = 0;
    while let Some(a) = head[rest..].find("<meta") {
        let a = rest + a;
        let e = head[a..].find('>').map_or(head.len(), |e| a + e);
        rest = e;
        let attr = |n: &str| -> Option<(usize, usize)> {
            let k = head[a..e].find(&format!("{n}="))? + a + n.len() + 1;
            let q = head.as_bytes().get(k).copied()?;
            if q == b'"' || q == b'\'' {
                let end = head[k + 1..e].find(q as char)? + k + 1;
                Some((k + 1, end))
            } else {
                let end = head[k..e].find(|c: char| c.is_whitespace() || c == '/').map_or(e, |x| k + x);
                Some((k, end))
            }
        };
        let name = attr("name").or_else(|| attr("itemprop")).map(|(s, e)| &head[s..e]);
        if name.is_some_and(|n| names.contains(&n)) {
            // (the value from the original text: its case kept)
            return attr("content").map(|(s, e)| decode(&src[s..e]));
        }
    }
    None
}

/// A date as notes apps write them, in Unix seconds: ISO 8601 ("2023-11-05", "2023-11-05T14:22:33Z",
/// "2023-11-05 14:22:33 +0000", with or without seconds, fractions or a zone: none = local time),
/// Evernote's "20231105T142233Z", or Unix seconds or milliseconds.
pub fn parse_date(s: &str) -> Option<f64> {
    let s = s.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'.') && !s.contains('-') {
        if s.len() == 8 {
            // (a bare "20231105")
            return parse_date(&format!("{}-{}-{}", &s[0..4], &s[4..6], &s[6..8]));
        }
        let n: f64 = s.parse().ok()?;
        return (n > 1e8).then(|| if n > 1e11 { n / 1000.0 } else { n });
    }
    if s.len() >= 15 && s.as_bytes()[8] == b'T' && s.get(..8).is_some_and(|x| x.bytes().all(|b| b.is_ascii_digit())) {
        return enex_date(s);
    }
    let b = s.as_bytes();
    let num = |r: std::ops::Range<usize>| s.get(r).filter(|x| x.bytes().all(|c| c.is_ascii_digit())).and_then(|x| x.parse::<i64>().ok());
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' || !s.is_char_boundary(10) {
        return None;
    }
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut rest = &s[10..];
    let (mut hh, mut mm, mut ss, mut frac) = (0, 0, 0, 0.0);
    if let Some(r) = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' ')).filter(|r| r.len() >= 5 && r.as_bytes()[2] == b':') {
        hh = r.get(0..2)?.parse().ok()?;
        mm = r.get(3..5)?.parse().ok()?;
        rest = r.get(5..)?;
        if rest.len() >= 3 && rest.as_bytes()[0] == b':' {
            ss = rest.get(1..3)?.parse().ok()?;
            rest = rest.get(3..)?;
            if let Some(f) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(',')) {
                let n = f.bytes().take_while(|c| c.is_ascii_digit()).count();
                frac = format!("0.{}", &f[..n]).parse().unwrap_or(0.0);
                rest = &f[n..];
            }
        }
    }
    let local = (days_from_civil(y, m, d) * 86400 + hh * 3600 + mm * 60 + ss) as f64 + frac;
    let z = rest.trim();
    let zone = if z.is_empty() {
        None
    } else if z.eq_ignore_ascii_case("z") || z.eq_ignore_ascii_case("utc") || z.eq_ignore_ascii_case("gmt") {
        Some(0)
    } else {
        let sign = match z.as_bytes()[0] {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let digits: String = z[1..].chars().filter(|c| c.is_ascii_digit()).collect();
        let (h, mi) = match digits.len() {
            2 => (digits.parse::<i64>().ok()?, 0),
            4 => (digits[0..2].parse::<i64>().ok()?, digits[2..4].parse::<i64>().ok()?),
            _ => return None,
        };
        Some(sign * (h * 3600 + mi * 60))
    };
    // (no zone: the computer's own, as the notes app that wrote it would have meant)
    let off = zone.unwrap_or_else(|| crate::dates::offset(local) as i64);
    Some(local - off as f64)
}

/// Days from 1970-01-01 (Howard Hinnant's civil algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
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

/// The character style while reading Markdown's inline text (with the inline HTML that apps
/// such as UpNote write for colours and underlines).
#[derive(Clone, Default)]
struct MdStyle {
    bold: bool,
    italic: bool,
    strike: bool,
    underline: bool,
    color: Option<[f32; 4]>,
    highlight: Option<u8>,
    link: Option<String>,
}

/// A CSS colour ("#25b7c4", "#abc", "rgb(1, 2, 3)" or a basic name) as RGBA.
fn css_color(v: &str) -> Option<[f32; 4]> {
    let v = v.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
    if let Some(h) = v.strip_prefix('#') {
        let h: String = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect() } else { h.chars().take(6).collect() };
        if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
            let n = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
            return Some([n(0), n(2), n(4), 1.0]);
        }
        return None;
    }
    if let Some(inner) = v.strip_prefix("rgba(").or(v.strip_prefix("rgb(")).and_then(|r| r.strip_suffix(')')) {
        let p: Vec<f32> = inner.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        return (p.len() >= 3).then(|| [p[0] / 255.0, p[1] / 255.0, p[2] / 255.0, 1.0]);
    }
    Some(match v.as_str() {
        "red" => [0.93, 0.26, 0.21, 1.0],
        "orange" => [0.96, 0.56, 0.13, 1.0],
        "yellow" => [0.85, 0.68, 0.0, 1.0],
        "green" => [0.2, 0.66, 0.33, 1.0],
        "blue" => [0.16, 0.47, 0.96, 1.0],
        "purple" => [0.6, 0.33, 0.83, 1.0],
        "gray" | "grey" => [0.55, 0.55, 0.57, 1.0],
        _ => return None,
    })
}

/// One CSS property's value out of a style attribute.
fn css_prop(style: &str, name: &str) -> Option<String> {
    style.split(';').find_map(|d| {
        let (k, v) = d.split_once(':')?;
        (k.trim().eq_ignore_ascii_case(name)).then(|| v.trim().to_string())
    })
}

/// An attribute's value in an HTML tag's text (`a href="…"`).
fn html_attr(tag: &str, name: &str) -> Option<String> {
    let low = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = low[from..].find(name).map(|i| i + from) {
        let before_ok = i == 0 || low.as_bytes()[i - 1].is_ascii_whitespace();
        let after = low[i + name.len()..].trim_start();
        if before_ok && after.starts_with('=') {
            let v = tag[tag.len() - after.len() + 1..].trim_start();
            return Some(match v.chars().next() {
                Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or("").to_string(),
                _ => v.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or("").to_string(),
            });
        }
        from = i + name.len();
    }
    None
}

/// Markdown's inline styles as spans. Inline HTML (`<span style="color:#25b7c4">`, `<b>`,
/// `<u>`, `<mark>`, `<a href>`, `<br>`…) becomes formatting: tags never stay as text, so a
/// colour code inside one can't turn into a #tag.
fn inline_md(t: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut st = MdStyle::default();
    let mut mono = false;
    // the styles in effect before each open HTML tag, to restore at its closing tag
    let mut open: Vec<(String, MdStyle)> = Vec::new();
    let mut cur = String::new();
    let flush = |out: &mut Vec<Span>, cur: &mut String, st: &MdStyle, link: Option<String>| {
        if !cur.is_empty() {
            let text = decode(&std::mem::take(cur));
            out.push(Span { text, bold: st.bold, italic: st.italic, strike: st.strike, underline: st.underline, color: st.color, highlight: st.highlight, link: link.or(st.link.clone()), ..Span::default() });
        }
    };
    let c: Vec<char> = t.chars().collect();
    let mut k = 0;
    while k < c.len() {
        let rest: String = c[k..].iter().take(2).collect();
        if mono {
            if c[k] == '`' {
                flush(&mut out, &mut cur, &st, None);
                mono = false;
            } else {
                cur.push(c[k]);
            }
            k += 1;
            continue;
        }
        if c[k] == '<' && c.get(k + 1).is_some_and(|n| n.is_ascii_alphabetic() || *n == '/' || *n == '!') {
            // an HTML tag (or comment): never shown as text
            let Some(close) = c[k..].iter().position(|&x| x == '>').map(|p| p + k) else {
                cur.push('<');
                k += 1;
                continue;
            };
            let tag: String = c[k + 1..close].iter().collect();
            k = close + 1;
            if tag.starts_with('!') {
                continue;
            }
            let closing = tag.starts_with('/');
            let body = tag.trim_start_matches('/').trim_end_matches('/').trim();
            let name: String = body.chars().take_while(|ch| ch.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
            if name == "br" {
                cur.push(' ');
                continue;
            }
            flush(&mut out, &mut cur, &st, None);
            if closing {
                if let Some(i) = open.iter().rposition(|(n, _)| *n == name) {
                    st = open[i].1.clone();
                    open.truncate(i);
                }
                continue;
            }
            if tag.ends_with('/') {
                continue;
            }
            open.push((name.clone(), st.clone()));
            match name.as_str() {
                "b" | "strong" => st.bold = true,
                "i" | "em" => st.italic = true,
                "u" | "ins" => st.underline = true,
                "s" | "del" | "strike" => st.strike = true,
                "mark" => st.highlight = Some(3),
                "code" => {}
                "a" => st.link = html_attr(body, "href"),
                _ => {}
            }
            if let Some(style) = html_attr(body, "style") {
                if let Some(col) = css_prop(&style, "color").and_then(|v| css_color(&v)) {
                    st.color = Some(col);
                }
                if css_prop(&style, "background-color").or(css_prop(&style, "background")).is_some() {
                    st.highlight = Some(3);
                }
                if css_prop(&style, "font-weight").is_some_and(|w| w.contains("bold") || w.parse::<u32>().is_ok_and(|n| n >= 600)) {
                    st.bold = true;
                }
                if css_prop(&style, "font-style").is_some_and(|v| v.contains("italic")) {
                    st.italic = true;
                }
                if let Some(d) = css_prop(&style, "text-decoration") {
                    st.underline |= d.contains("underline");
                    st.strike |= d.contains("line-through");
                }
            }
            if name == "font" {
                if let Some(col) = html_attr(body, "color").and_then(|v| css_color(&v)) {
                    st.color = Some(col);
                }
            }
            continue;
        }
        if c[k] == '\\' && k + 1 < c.len() {
            cur.push(c[k + 1]);
            k += 2;
        } else if rest == "**" || rest == "__" {
            flush(&mut out, &mut cur, &st, None);
            st.bold = !st.bold;
            k += 2;
        } else if rest == "~~" {
            flush(&mut out, &mut cur, &st, None);
            st.strike = !st.strike;
            k += 2;
        } else if (c[k] == '*' || c[k] == '_') && (st.italic || c.get(k + 1).is_some_and(|n| !n.is_whitespace())) && (c[k] == '*' || k == 0 || !c[k - 1].is_alphanumeric()) {
            flush(&mut out, &mut cur, &st, None);
            st.italic = !st.italic;
            k += 1;
        } else if c[k] == '`' {
            flush(&mut out, &mut cur, &st, None);
            mono = true;
            k += 1;
        } else if c[k] == '[' {
            // [text](url)
            let close = c[k..].iter().position(|&x| x == ']').map(|p| p + k);
            match close.filter(|&e| c.get(e + 1) == Some(&'(')).and_then(|e| c[e..].iter().position(|&x| x == ')').map(|p| (e, p + e))) {
                Some((e, end)) => {
                    flush(&mut out, &mut cur, &st, None);
                    let mut label: String = c[k + 1..e].iter().collect();
                    let url: String = c[e + 2..end].iter().collect();
                    flush(&mut out, &mut label, &st, Some(url));
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
    flush(&mut out, &mut cur, &st, None);
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
    Some((days_from_civil(y, m, d) * 86400 + hh * 3600 + mm * 60 + ss) as f64)
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

    /// A folder of its own for a test's files.
    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nt-import-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A file written with a given last-change time.
    fn write_at(p: &Path, text: &str, mtime: f64) {
        std::fs::write(p, text).unwrap();
        let f = std::fs::File::options().write(true).open(p).unwrap();
        f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(mtime)).unwrap();
    }

    #[test]
    fn dates_in_every_form() {
        assert_eq!(parse_date("2023-11-05T14:22:33Z"), Some(1699194153.0));
        assert_eq!(parse_date("2023-11-05 14:22:33 +0000"), Some(1699194153.0));
        assert_eq!(parse_date("2023-11-05T16:22:33+02:00"), Some(1699194153.0));
        assert_eq!(parse_date("2023-11-05T09:22:33.500-0500"), Some(1699194153.5));
        assert_eq!(parse_date("\"2023-11-05T14:22Z\""), Some(1699194120.0));
        assert_eq!(parse_date("20231105T142233Z"), Some(1699194153.0));
        assert_eq!(parse_date("1699194153"), Some(1699194153.0));
        assert_eq!(parse_date("1699194153000"), Some(1699194153.0));
        // (no zone: the computer's own time)
        let local = parse_date("2023-11-05 14:22:33").unwrap();
        assert!((local - 1699194153.0).abs() <= 14.0 * 3600.0);
        assert!(parse_date("2023-11-05").is_some());
        for bad in ["", "soon", "2023", "2023-13-01", "2023-11-05T25", "é2023-11-05"] {
            assert_eq!(parse_date(bad), None, "{bad}");
        }
    }

    #[test]
    fn markdown_inline_html_is_formatting_not_text() {
        let b = markdown("Some <span style=\"color: #25b7c4\">blue</span> and <span style=\"background-color:#5FC3E4\">lit</span>, <u>under</u> &amp; a real #todo");
        let p = &b.paragraphs[0];
        let text: String = p.spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "Some blue and lit, under & a real #todo");
        assert!(!text.contains("#25b7c4") && !text.contains("<span"));
        let blue = p.spans.iter().find(|s| s.text == "blue").unwrap();
        assert!(blue.color.is_some_and(|c| (c[2] - 0xc4 as f32 / 255.0).abs() < 0.01));
        assert!(p.spans.iter().find(|s| s.text == "lit").unwrap().highlight.is_some());
        assert!(p.spans.iter().find(|s| s.text == "under").unwrap().underline);
    }

    #[test]
    fn markdown_front_matter_gives_the_dates() {
        let d = scratch("fm");
        let p = d.join("trip.md");
        write_at(&p, "---\ntitle: Trip\ncreated: 2021-03-04T05:06:07Z\nupdated: '2022-01-02T03:04:05Z'\ntags: [a, b]\n---\nPack light\n", 1_700_000_000.0);
        let it = read_file(&p).unwrap().remove(0);
        assert_eq!(it.created, Some(1614834367.0));
        assert_eq!(it.modified, Some(1641092645.0));
        // (the front matter isn't text in the note; its title is the note's)
        let t = texts(&it.body);
        assert_eq!(t[0], (ParaStyle::Title, "Trip".into()));
        assert_eq!(t[1].1, "Pack light");
        assert_eq!(t.len(), 2);
        // `date:` (static sites, Obsidian) as the created date; no updated: the file's
        write_at(&p, "---\ndate: 2020-05-06\n---\n# Plan\n", 1_700_000_000.0);
        let it = read_file(&p).unwrap().remove(0);
        assert!(it.created.is_some_and(|c| (c - 1588723200.0).abs() <= 14.0 * 3600.0));
        assert_eq!(it.modified, Some(1_700_000_000.0));
        // a "---" rule that isn't front matter stays text
        let (meta, rest) = front_matter("---\nnot: closed\n");
        assert!(meta.is_empty() && rest.starts_with("---"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn files_without_dates_keep_the_files_times_in_order() {
        let d = scratch("times");
        // three notes written years apart, named so their name order isn't their date order
        for (name, t) in [("a.txt", 1_600_000_000.0), ("b.md", 1_400_000_000.0), ("c.html", 1_500_000_000.0), ("d.rtf", 1_300_000_000.0)] {
            let body = if name.ends_with(".rtf") { r"{\rtf1\ansi Hello\par}".to_string() } else { format!("Note {name}") };
            write_at(&d.join(name), &body, t);
        }
        let tree = read_dir(&d);
        assert_eq!(tree.items.len(), 4);
        for it in &tree.items {
            // (created: the file's birth time where there is one, never after its last change)
            let (c, m) = (it.created.unwrap(), it.modified.unwrap());
            assert!(c <= m, "{c} {m}");
        }
        let m: Vec<f64> = tree.items.iter().map(|i| i.modified.unwrap()).collect();
        assert_eq!(m, vec![1_600_000_000.0, 1_400_000_000.0, 1_500_000_000.0, 1_300_000_000.0]);
        // sorted by date created, the oldest file first
        let mut by: Vec<(f64, String)> = tree.items.iter().map(|i| (i.created.unwrap(), texts(&i.body)[0].1.clone())).collect();
        by.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(by.iter().map(|b| b.1.as_str()).collect::<Vec<_>>(), ["Hello", "Note b.md", "Note c.html", "Note a.txt"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn html_and_enex_files_keep_their_own_dates() {
        let d = scratch("meta");
        let p = d.join("page.html");
        write_at(&p, r#"<html><head><meta name="created" content="2019-02-03 04:05:06 +0000"/><META NAME='updated' CONTENT='2019-03-03T00:00:00Z'><title>x</title></head><body><h1>Hi</h1></body></html>"#, 1_700_000_000.0);
        let it = read_file(&p).unwrap().remove(0);
        assert_eq!(it.created, Some(1549166706.0));
        assert_eq!(it.modified, Some(1551571200.0));
        let p = d.join("book.enex");
        write_at(&p, r#"<en-export><note><title>Old</title><content><![CDATA[<en-note>x</en-note>]]></content><created>20100101T000000Z</created></note></en-export>"#, 1_700_000_000.0);
        let n = read_file(&p).unwrap();
        assert_eq!((n[0].created, n[0].modified), (Some(1262304000.0), Some(1262304000.0)));
        let _ = std::fs::remove_dir_all(&d);
    }
}
