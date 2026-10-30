//! Opening other documents as new Pages documents: Word (.docx), Rich Text (.rtf), Markdown,
//! HTML and plain text. Each reader produces paragraphs (style, list, runs, breaks, pictures)
//! that are written into a Blank document with Pages' own styles (Title, Heading, Heading 2/3,
//! Body) and list styles (Bullet, Numbered).

use kn_model::edit::History;

use crate::Pages;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunFmt {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub size: Option<f32>,
    pub color: Option<kn_model::Color>,
    pub font: Option<String>,
    pub script: i8,
    pub link: Option<String>,
    /// 1 all caps, 2 small caps (shown so; the text keeps its own case).
    pub caps: u8,
    /// Extra space after each character (Word's w:spacing), points.
    pub spacing: Option<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct InPara {
    /// Pages paragraph style name ("Body", "Heading"…).
    pub style: String,
    /// Bullet (false) or numbered (true) list, and level.
    pub list: Option<(bool, u32)>,
    pub runs: Vec<(String, RunFmt)>,
    pub page_break_after: bool,
    pub align: Option<kn_model::Align>,
    /// Pictures (bytes, name) shown in this paragraph.
    pub images: Vec<(Vec<u8>, String)>,
}

impl InPara {
    fn text(&self) -> String {
        self.runs.iter().map(|r| r.0.as_str()).collect()
    }
}

/// Write paragraphs into a new Blank document.
pub fn build(paras: &[InPara]) -> Pages {
    let mut p = Pages::new_blank(false);
    let Some(body) = p.body_storage() else { return p };
    let mut h = History::default();
    p.doc.begin(&mut h, "Import");
    // the whole text first
    let mut text = String::new();
    let mut starts = Vec::new();
    for (i, para) in paras.iter().enumerate() {
        starts.push(text.len());
        text.push_str(&para.text().replace(['\n', '\r'], "\u{2028}"));
        if i + 1 < paras.len() {
            text.push(if para.page_break_after { '\u{5}' } else { '\n' });
        }
    }
    crate::edit::replace(&mut p.doc, &mut h, body, 0, 0, &text);
    let lists = crate::edit::list_styles(&p.doc);
    let bullet = lists.iter().find(|l| l.1 == "Bullet").map(|l| l.0);
    let numbered = lists.iter().find(|l| l.1 == "Numbered").map(|l| l.0);
    for (i, para) in paras.iter().enumerate() {
        let at = starts[i];
        if para.style != "Body" && !para.style.is_empty() {
            if let Some(s) = crate::edit::para_style_named(&p.doc, &para.style) {
                crate::edit::set_para_style(&mut p.doc, &mut h, body, at, at, s);
            }
        }
        if let Some((num, level)) = para.list {
            let l = if num { numbered } else { bullet };
            if l.is_some() {
                crate::edit::set_list_style(&mut p.doc, &mut h, body, at, at, l);
                if level > 0 {
                    crate::edit::change_level(&mut p.doc, &mut h, body, at, at, level as i32);
                }
            }
        }
        if let Some(a) = para.align {
            let v = match a {
                kn_model::Align::Right => "TATvalue1",
                kn_model::Align::Center => "TATvalue2",
                kn_model::Align::Justify => "TATvalue3",
                _ => "TATvalue0",
            };
            crate::edit::set_para_props(&mut p.doc, &mut h, body, at, at, &|pp| pp.set_enum("alignment", v));
        }
        let mut off = at;
        for (t, f) in &para.runs {
            let len = t.replace(['\n', '\r'], "\u{2028}").len();
            if *f != RunFmt::default() && len > 0 {
                let f2 = f.clone();
                crate::edit::set_char_props(&mut p.doc, &mut h, body, off, off + len, &|cp| {
                    if f2.bold {
                        cp.set_bool("bold", true);
                    }
                    if f2.italic {
                        cp.set_bool("italic", true);
                    }
                    if f2.underline {
                        cp.set_enum("underline", "kSingleUnderline");
                    }
                    if f2.strike {
                        cp.set_enum("strikethru", "kSingleStrikethru");
                    }
                    if let Some(s) = f2.size {
                        cp.set_f32("font_size", s.clamp(1.0, 500.0));
                    }
                    if let Some(c) = f2.color {
                        cp.push_msg("font_color", kn_model::format::color_msg(c));
                    }
                    if let Some(fnt) = &f2.font {
                        cp.set_str("font_name", &fnt.replace(' ', ""));
                    }
                    if f2.script != 0 {
                        cp.set_enum("superscript", if f2.script > 0 { "kSuperscript" } else { "kSubscript" });
                    }
                });
                if let Some(url) = &f.link {
                    p.add_link(&mut h, body, off, off + len, url);
                }
            }
            off += len;
        }
    }
    // pictures, from the end so offsets stay right
    let width = p.setup().text_width();
    for (i, para) in paras.iter().enumerate().rev() {
        let end = starts[i] + para.text().replace(['\n', '\r'], "\u{2028}").len();
        for (bytes, name) in para.images.iter().rev() {
            let _ = p.insert_image(&mut h, bytes.clone(), name, crate::objects::Placement::Inline { at: end }, width);
        }
    }
    p.doc.end(&mut h);
    p.doc.dirty = true;
    p
}

/// Plain text: one paragraph per line; form feeds are page breaks.
pub fn from_text(s: &str) -> Vec<InPara> {
    let mut out = Vec::new();
    for page in s.replace("\r\n", "\n").split('\u{c}') {
        for line in page.split('\n') {
            out.push(InPara { style: "Body".into(), runs: vec![(line.to_string(), RunFmt::default())], ..Default::default() });
        }
        if let Some(l) = out.last_mut() {
            l.page_break_after = true;
        }
    }
    if let Some(l) = out.last_mut() {
        l.page_break_after = false;
    }
    out
}

/// Inline Markdown: **bold**, *italic* / _italic_, `code`, [text](url).
fn md_inline(s: &str) -> Vec<(String, RunFmt)> {
    let mut out: Vec<(String, RunFmt)> = Vec::new();
    let mut f = RunFmt::default();
    let mut cur = String::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    let flush = |out: &mut Vec<(String, RunFmt)>, cur: &mut String, f: &RunFmt| {
        if !cur.is_empty() {
            out.push((std::mem::take(cur), f.clone()));
        }
    };
    while i < cs.len() {
        let c = cs[i];
        if c == '*' && cs.get(i + 1) == Some(&'*') {
            flush(&mut out, &mut cur, &f);
            f.bold = !f.bold;
            i += 2;
            continue;
        }
        if (c == '*' || c == '_') && !(i > 0 && cs[i - 1].is_alphanumeric() && c == '_') {
            flush(&mut out, &mut cur, &f);
            f.italic = !f.italic;
            i += 1;
            continue;
        }
        if c == '`' {
            flush(&mut out, &mut cur, &f);
            f.font = if f.font.is_some() { None } else { Some("Courier".into()) };
            i += 1;
            continue;
        }
        if c == '[' {
            if let Some(close) = cs[i..].iter().position(|x| *x == ']').map(|p| i + p) {
                if cs.get(close + 1) == Some(&'(') {
                    if let Some(end) = cs[close..].iter().position(|x| *x == ')').map(|p| close + p) {
                        flush(&mut out, &mut cur, &f);
                        let text: String = cs[i + 1..close].iter().collect();
                        let url: String = cs[close + 2..end].iter().collect();
                        out.push((text, RunFmt { link: Some(url), underline: false, ..f.clone() }));
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        cur.push(c);
        i += 1;
    }
    flush(&mut out, &mut cur, &f);
    if out.is_empty() {
        out.push((String::new(), RunFmt::default()));
    }
    out
}

pub fn from_markdown(s: &str) -> Vec<InPara> {
    let mut out: Vec<InPara> = Vec::new();
    let mut para: Option<InPara> = None;
    let flush = |out: &mut Vec<InPara>, para: &mut Option<InPara>| {
        if let Some(p) = para.take() {
            out.push(p);
        }
    };
    for raw in s.replace("\r\n", "\n").lines() {
        let indent = raw.len() - raw.trim_start().len();
        let line = raw.trim();
        if line.is_empty() {
            flush(&mut out, &mut para);
            continue;
        }
        let heading = [("### ", "Heading 3"), ("## ", "Heading 2"), ("# ", "Heading")].iter().find(|(p, _)| line.starts_with(p)).map(|(p, s)| (p.len(), *s));
        if let Some((n, style)) = heading {
            flush(&mut out, &mut para);
            out.push(InPara { style: style.into(), runs: md_inline(&line[n..]), ..Default::default() });
            continue;
        }
        if line == "---" || line == "***" {
            flush(&mut out, &mut para);
            if let Some(l) = out.last_mut() {
                l.page_break_after = true;
            }
            continue;
        }
        let bullet = ["- ", "* ", "+ "].iter().any(|b| line.starts_with(b));
        let num = line.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if bullet || num {
            flush(&mut out, &mut para);
            let rest = if bullet { &line[2..] } else { line.split_once(". ").unwrap().1 };
            out.push(InPara { style: "Body".into(), list: Some((num, (indent / 2) as u32)), runs: md_inline(rest), ..Default::default() });
            continue;
        }
        match para.as_mut() {
            Some(p) => {
                p.runs.push((" ".into(), RunFmt::default()));
                p.runs.extend(md_inline(line));
            }
            None => para = Some(InPara { style: "Body".into(), runs: md_inline(line), ..Default::default() }),
        }
    }
    flush(&mut out, &mut para);
    out
}

/// A small RTF reader: text, paragraphs, line / page breaks, bold, italic, underline,
/// strikethrough, sizes, colours, super/subscript; tables become tab-separated lines.
pub fn from_rtf(s: &str) -> Vec<InPara> {
    #[derive(Clone)]
    struct St {
        f: RunFmt,
        skip: bool,
    }
    let mut out = Vec::new();
    let mut cur = InPara { style: "Body".into(), ..Default::default() };
    let mut stack: Vec<St> = vec![St { f: RunFmt::default(), skip: false }];
    let mut colors: Vec<kn_model::Color> = Vec::new();
    let mut in_colortbl = false;
    let mut col = (0u8, 0u8, 0u8);
    let b = s.as_bytes();
    let mut i = 0;
    let mut text = String::new();
    let mut uc_skip = 0usize;
    macro_rules! push_text {
        () => {
            if !text.is_empty() {
                let f = stack.last().unwrap().f.clone();
                cur.runs.push((std::mem::take(&mut text), f));
            }
        };
    }
    while i < b.len() {
        let c = b[i];
        match c {
            b'{' => {
                push_text!();
                let top = stack.last().cloned().unwrap();
                stack.push(top);
                i += 1;
                // destinations we skip
                let rest = &s[i..];
                if rest.starts_with("\\*") || ["\\fonttbl", "\\stylesheet", "\\info", "\\pict", "\\header", "\\footer", "\\listtable", "\\listoverridetable", "\\generator"].iter().any(|d| rest.starts_with(d)) {
                    stack.last_mut().unwrap().skip = true;
                }
                if rest.starts_with("\\colortbl") {
                    in_colortbl = true;
                }
            }
            b'}' => {
                push_text!();
                if stack.len() > 1 {
                    stack.pop();
                }
                in_colortbl = false;
                i += 1;
            }
            b'\\' => {
                i += 1;
                if i >= b.len() {
                    break;
                }
                let n = b[i];
                if n == b'\'' {
                    if let Some(h) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                        if !stack.last().unwrap().skip && uc_skip == 0 {
                            text.push(if h < 0x80 { h as char } else { char::from_u32(match h { 0x91 => 0x2018, 0x92 => 0x2019, 0x93 => 0x201C, 0x94 => 0x201D, 0x96 => 0x2013, 0x97 => 0x2014, 0x85 => 0x2026, v => v as u32 }).unwrap_or('?') });
                        }
                        uc_skip = uc_skip.saturating_sub(1);
                    }
                    i += 3;
                    continue;
                }
                if !n.is_ascii_alphabetic() {
                    if !stack.last().unwrap().skip && matches!(n, b'\\' | b'{' | b'}') {
                        text.push(n as char);
                    }
                    if n == b'~' && !stack.last().unwrap().skip {
                        text.push('\u{a0}');
                    }
                    i += 1;
                    continue;
                }
                let start = i;
                while i < b.len() && b[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let word = &s[start..i];
                let nstart = i;
                if i < b.len() && (b[i] == b'-' || b[i].is_ascii_digit()) {
                    i += 1;
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let num: Option<i32> = s[nstart..i].parse().ok();
                if i < b.len() && b[i] == b' ' {
                    i += 1;
                }
                let skip = stack.last().unwrap().skip;
                if in_colortbl {
                    match word {
                        "red" => col.0 = num.unwrap_or(0) as u8,
                        "green" => col.1 = num.unwrap_or(0) as u8,
                        "blue" => {
                            col.2 = num.unwrap_or(0) as u8;
                            colors.push(kn_model::Color { r: f32::from(col.0) / 255.0, g: f32::from(col.1) / 255.0, b: f32::from(col.2) / 255.0, a: 1.0 });
                        }
                        _ => {}
                    }
                    continue;
                }
                if skip {
                    continue;
                }
                let on = num != Some(0);
                match word {
                    "par" | "row" => {
                        push_text!();
                        out.push(std::mem::replace(&mut cur, InPara { style: "Body".into(), ..Default::default() }));
                    }
                    "line" => text.push('\n'),
                    "tab" | "cell" => text.push('\t'),
                    "page" => {
                        push_text!();
                        cur.page_break_after = true;
                    }
                    "b" => {
                        push_text!();
                        stack.last_mut().unwrap().f.bold = on;
                    }
                    "i" => {
                        push_text!();
                        stack.last_mut().unwrap().f.italic = on;
                    }
                    "ul" => {
                        push_text!();
                        stack.last_mut().unwrap().f.underline = on;
                    }
                    "ulnone" => {
                        push_text!();
                        stack.last_mut().unwrap().f.underline = false;
                    }
                    "strike" => {
                        push_text!();
                        stack.last_mut().unwrap().f.strike = on;
                    }
                    "super" => {
                        push_text!();
                        stack.last_mut().unwrap().f.script = 1;
                    }
                    "sub" => {
                        push_text!();
                        stack.last_mut().unwrap().f.script = -1;
                    }
                    "nosupersub" => {
                        push_text!();
                        stack.last_mut().unwrap().f.script = 0;
                    }
                    "fs" => {
                        push_text!();
                        stack.last_mut().unwrap().f.size = num.map(|n| n as f32 / 2.0);
                    }
                    "cf" => {
                        push_text!();
                        stack.last_mut().unwrap().f.color = num.and_then(|n| colors.get((n as usize).wrapping_sub(1)).copied());
                    }
                    "plain" => {
                        push_text!();
                        stack.last_mut().unwrap().f = RunFmt::default();
                    }
                    "qc" => cur.align = Some(kn_model::Align::Center),
                    "qr" => cur.align = Some(kn_model::Align::Right),
                    "qj" => cur.align = Some(kn_model::Align::Justify),
                    "u" => {
                        if let Some(v) = num {
                            let v = if v < 0 { v + 65536 } else { v } as u32;
                            if let Some(ch) = char::from_u32(v) {
                                text.push(ch);
                            }
                            uc_skip = 1;
                        }
                    }
                    _ => {}
                }
            }
            b'\r' | b'\n' => i += 1,
            _ => {
                let ch = s[i..].chars().next().unwrap();
                if !stack.last().unwrap().skip {
                    if uc_skip > 0 && ch == '?' {
                        uc_skip -= 1;
                    } else {
                        text.push(ch);
                    }
                }
                i += ch.len_utf8();
            }
        }
    }
    push_text!();
    if !cur.runs.is_empty() {
        out.push(cur);
    }
    out
}

fn xml_attr<'a>(n: &roxmltree::Node<'a, '_>, local: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == local).map(|a| a.value())
}

/// Word (.docx): paragraphs with their styles (Title / Heading n), lists, runs (bold, italic,
/// underline, strike, size, colour, font, super/subscript), links, page breaks, tables (as
/// tab-separated lines) and inline pictures.
pub fn from_docx(path: &std::path::Path) -> Result<Vec<InPara>, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut read = |name: &str| -> Option<Vec<u8>> {
        let mut e = z.by_name(name).ok()?;
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut e, &mut b).ok()?;
        Some(b)
    };
    let doc = String::from_utf8(read("word/document.xml").ok_or("not a Word document")?).map_err(|e| e.to_string())?;
    let rels = read("word/_rels/document.xml.rels").and_then(|b| String::from_utf8(b).ok()).unwrap_or_default();
    let styles = read("word/styles.xml").and_then(|b| String::from_utf8(b).ok()).unwrap_or_default();
    // relationship id -> (target, external)
    let mut rel_map = std::collections::HashMap::new();
    if let Ok(x) = roxmltree::Document::parse(&rels) {
        for r in x.descendants().filter(|n| n.tag_name().name() == "Relationship") {
            if let (Some(id), Some(t)) = (r.attribute("Id"), r.attribute("Target")) {
                rel_map.insert(id.to_string(), t.to_string());
            }
        }
    }
    // style id -> name (headings by name)
    let mut style_names = std::collections::HashMap::new();
    if let Ok(x) = roxmltree::Document::parse(&styles) {
        for s in x.descendants().filter(|n| n.tag_name().name() == "style") {
            let id = xml_attr(&s, "styleId").unwrap_or("");
            let name = s.children().find(|c| c.tag_name().name() == "name").and_then(|n| xml_attr(&n, "val")).unwrap_or("");
            style_names.insert(id.to_string(), name.to_lowercase());
        }
    }
    let map_style = |id: &str| -> String {
        let n = style_names.get(id).cloned().unwrap_or_else(|| id.to_lowercase());
        match n.as_str() {
            "title" => "Title".into(),
            "subtitle" => "Subtitle".into(),
            "heading 1" | "heading1" | "heading" => "Heading".into(),
            "heading 2" | "heading2" => "Heading 2".into(),
            "heading 3" | "heading3" | "heading 4" | "heading4" => "Heading 3".into(),
            "caption" => "Caption".into(),
            _ => "Body".into(),
        }
    };
    let x = roxmltree::Document::parse(&doc).map_err(|e| e.to_string())?;
    let body = x.descendants().find(|n| n.tag_name().name() == "body").ok_or("no body")?;
    let mut out: Vec<InPara> = Vec::new();
    let mut media: Vec<(String, String)> = Vec::new(); // (paragraph index marker, target)
    let run_fmt = |r: &roxmltree::Node| -> RunFmt {
        let mut f = RunFmt::default();
        if let Some(rpr) = r.children().find(|c| c.tag_name().name() == "rPr") {
            for c in rpr.children() {
                let val = xml_attr(&c, "val");
                let on = val.is_none_or(|v| v != "0" && v != "false" && v != "none");
                match c.tag_name().name() {
                    "b" => f.bold = on,
                    "i" => f.italic = on,
                    "u" => f.underline = on,
                    "strike" | "dstrike" => f.strike = on,
                    "sz" => f.size = val.and_then(|v| v.parse::<f32>().ok()).map(|v| v / 2.0),
                    "color" => {
                        if let Some(v) = val.filter(|v| v.len() == 6) {
                            let h = |i: usize| u8::from_str_radix(&v[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
                            f.color = Some(kn_model::Color { r: h(0), g: h(2), b: h(4), a: 1.0 });
                        }
                    }
                    "rFonts" => f.font = xml_attr(&c, "ascii").map(str::to_string),
                    "vertAlign" => f.script = match val {
                        Some("superscript") => 1,
                        Some("subscript") => -1,
                        _ => 0,
                    },
                    _ => {}
                }
            }
        }
        f
    };
    fn walk_runs(n: roxmltree::Node, link: Option<String>, out: &mut InPara, run_fmt: &dyn Fn(&roxmltree::Node) -> RunFmt, rel_map: &std::collections::HashMap<String, String>, pics: &mut Vec<String>) {
        for c in n.children() {
            match c.tag_name().name() {
                "r" => {
                    let mut f = run_fmt(&c);
                    f.link = link.clone();
                    let mut t = String::new();
                    for e in c.descendants() {
                        match e.tag_name().name() {
                            "t" => t.push_str(e.text().unwrap_or("")),
                            "tab" => t.push('\t'),
                            "br" => {
                                if xml_attr(&e, "type") == Some("page") {
                                    out.page_break_after = true;
                                } else {
                                    t.push('\n');
                                }
                            }
                            "blip" => {
                                if let Some(id) = e.attributes().find(|a| a.name() == "embed").map(|a| a.value()) {
                                    if let Some(tg) = rel_map.get(id) {
                                        pics.push(tg.clone());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    if !t.is_empty() {
                        out.runs.push((t, f));
                    }
                }
                "hyperlink" => {
                    let url = c.attributes().find(|a| a.name() == "id").and_then(|a| rel_map.get(a.value())).cloned();
                    walk_runs(c, url, out, run_fmt, rel_map, pics);
                }
                "ins" | "smartTag" | "sdt" | "sdtContent" | "fldSimple" => walk_runs(c, link.clone(), out, run_fmt, rel_map, pics),
                _ => {}
            }
        }
    }
    let mut para_from = |p: roxmltree::Node, out: &mut Vec<InPara>| {
        let mut ip = InPara { style: "Body".into(), ..Default::default() };
        if let Some(ppr) = p.children().find(|c| c.tag_name().name() == "pPr") {
            for c in ppr.children() {
                match c.tag_name().name() {
                    "pStyle" => ip.style = map_style(xml_attr(&c, "val").unwrap_or("")),
                    "numPr" => {
                        let lvl = c.children().find(|x| x.tag_name().name() == "ilvl").and_then(|x| xml_attr(&x, "val")).and_then(|v| v.parse().ok()).unwrap_or(0);
                        let num_id = c.children().find(|x| x.tag_name().name() == "numId").and_then(|x| xml_attr(&x, "val")).unwrap_or("1");
                        // without numbering.xml details: even ids numbered (a guess), the rest bullets
                        ip.list = Some((num_id != "1" && num_id.parse::<u32>().map(|v| v % 2 == 0).unwrap_or(false), lvl));
                    }
                    "jc" => {
                        ip.align = match xml_attr(&c, "val") {
                            Some("center") => Some(kn_model::Align::Center),
                            Some("right") | Some("end") => Some(kn_model::Align::Right),
                            Some("both") => Some(kn_model::Align::Justify),
                            _ => None,
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut pics = Vec::new();
        walk_runs(p, None, &mut ip, &run_fmt, &rel_map, &mut pics);
        let idx = out.len();
        for pic in pics {
            media.push((idx.to_string(), pic));
        }
        out.push(ip);
    };
    for n in body.children() {
        match n.tag_name().name() {
            "p" => para_from(n, &mut out),
            "tbl" => {
                for tr in n.children().filter(|c| c.tag_name().name() == "tr") {
                    let cells: Vec<String> = tr.children().filter(|c| c.tag_name().name() == "tc").map(|tc| tc.descendants().filter(|e| e.tag_name().name() == "t").filter_map(|e| e.text()).collect::<String>()).collect();
                    out.push(InPara { style: "Body".into(), runs: vec![(cells.join("\t"), RunFmt::default())], ..Default::default() });
                }
            }
            _ => {}
        }
    }
    drop(para_from);
    for (idx, target) in media {
        let name = format!("word/{}", target.trim_start_matches("../").trim_start_matches('/'));
        if let Some(bytes) = read(&name) {
            if let Ok(i) = idx.parse::<usize>() {
                if let Some(p) = out.get_mut(i) {
                    p.images.push((bytes, target.rsplit('/').next().unwrap_or("image.png").to_string()));
                }
            }
        }
    }
    Ok(out)
}

/// A small HTML reader: paragraphs, headings, lists, line breaks, bold / italic / underline,
/// links.
pub fn from_html(s: &str) -> Vec<InPara> {
    let mut out = Vec::new();
    let mut cur = InPara { style: "Body".into(), ..Default::default() };
    let mut f = RunFmt::default();
    let mut lists: Vec<bool> = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    let b = s;
    let mut skip = 0;
    let flush_text = |cur: &mut InPara, text: &mut String, f: &RunFmt| {
        let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let lead = text.starts_with(char::is_whitespace) && !cur.runs.is_empty();
        let trail = text.ends_with(char::is_whitespace) && !t.is_empty();
        if !t.is_empty() {
            cur.runs.push((format!("{}{}{}", if lead { " " } else { "" }, t, if trail { " " } else { "" }), f.clone()));
        }
        text.clear();
    };
    let ent = |t: &str| t.replace("&nbsp;", "\u{a0}").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&");
    while i < b.len() {
        if b[i..].starts_with('<') {
            let Some(end) = b[i..].find('>').map(|e| i + e) else { break };
            let tag = &b[i + 1..end];
            let closing = tag.starts_with('/');
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
            i = end + 1;
            if matches!(name.as_str(), "script" | "style" | "head" | "title") {
                if closing { skip = 0 } else { skip = 1 }
                continue;
            }
            if skip > 0 {
                continue;
            }
            match (name.as_str(), closing) {
                ("p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "tr", _) => {
                    flush_text(&mut cur, &mut text, &f);
                    if !cur.runs.is_empty() {
                        out.push(std::mem::replace(&mut cur, InPara { style: "Body".into(), ..Default::default() }));
                    }
                    if !closing {
                        cur.style = match name.as_str() {
                            "h1" => "Title".into(),
                            "h2" => "Heading".into(),
                            "h3" => "Heading 2".into(),
                            "h4" | "h5" | "h6" => "Heading 3".into(),
                            _ => "Body".into(),
                        };
                        if name == "li" {
                            cur.list = lists.last().map(|n| (*n, lists.len().saturating_sub(1) as u32));
                        }
                    }
                }
                ("br", _) => text.push('\n'),
                ("ul", false) => lists.push(false),
                ("ol", false) => lists.push(true),
                ("ul" | "ol", true) => {
                    lists.pop();
                }
                ("b" | "strong", c) => {
                    flush_text(&mut cur, &mut text, &f);
                    f.bold = !c;
                }
                ("i" | "em", c) => {
                    flush_text(&mut cur, &mut text, &f);
                    f.italic = !c;
                }
                ("u", c) => {
                    flush_text(&mut cur, &mut text, &f);
                    f.underline = !c;
                }
                ("s" | "strike" | "del", c) => {
                    flush_text(&mut cur, &mut text, &f);
                    f.strike = !c;
                }
                ("td" | "th", true) => text.push('\t'),
                ("a", false) => {
                    flush_text(&mut cur, &mut text, &f);
                    f.link = tag.split("href=").nth(1).map(|h| h.trim_start_matches(['"', '\'']).split(['"', '\'']).next().unwrap_or("").to_string());
                }
                ("a", true) => {
                    flush_text(&mut cur, &mut text, &f);
                    f.link = None;
                }
                _ => {}
            }
        } else {
            let next = b[i..].find('<').map(|n| i + n).unwrap_or(b.len());
            if skip == 0 {
                text.push_str(&ent(&b[i..next]));
            }
            i = next;
        }
    }
    flush_text(&mut cur, &mut text, &f);
    if !cur.runs.is_empty() {
        out.push(cur);
    }
    out
}

/// Open a .txt / .md / .rtf / .docx / .html file as a new (untitled) document.
/// A font's PostScript-style name as Pages stores it ("Times New Roman" -> "TimesNewRoman").
pub fn ps_name(family: &str) -> String {
    family.replace(' ', "")
}

pub fn import_file(path: &std::path::Path) -> Result<Pages, kn_format::Error> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let bad = |e: String| kn_format::Error::Invalid(e);
    if ext == "docx" {
        let w = crate::docx::read(path).map_err(bad)?;
        let mut p = crate::docx::build(&w);
        p.doc.path = None;
        return Ok(p);
    }
    let paras = match ext.as_str() {
        "docx" => from_docx(path).map_err(bad)?,
        _ => {
            let bytes = std::fs::read(path)?;
            let s = String::from_utf8_lossy(&bytes).to_string();
            match ext.as_str() {
                "md" | "markdown" => from_markdown(&s),
                "rtf" => from_rtf(&s),
                "html" | "htm" => from_html(&s),
                _ => from_text(&s),
            }
        }
    };
    let mut p = build(&paras);
    p.doc.path = None;
    Ok(p)
}
