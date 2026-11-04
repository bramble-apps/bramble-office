//! Edit > Copy as Markdown, with Notes' own export mapping (format section 8): # / ## / ###,
//! two-space line ends, "- [ ]" / "- [x]", "1.", "* " (4 spaces a level), "- ", ==highlight==,
//! ++underline++, **bold**, *italic*, ~~strike~~, pipe tables, "---", "> ", ``` fences.

use nt_core::model::ParaStyle;

use super::doc::{AttKind, Doc, Para, att_plain};
use super::ops::Frag;

fn inline(doc: &Doc, frag: &Frag, p: &Para) -> String {
    let mut out = String::new();
    for (r, f) in p.spans() {
        let t = &p.text[r];
        let att = f.att.as_deref().and_then(|id| frag.atts.iter().find(|a| a.id == id).or_else(|| doc.att(id)));
        if let Some(a) = att {
            out.push_str(&match &a.kind {
                AttKind::Table(_) => table_md(a),
                AttKind::Image { .. } => format!("![Image](Attachments/{}.png)", a.id),
                AttKind::Drawing { .. } => format!("![Sketch](Attachments/{}.png)", a.id),
                AttKind::Pdf { .. } => format!("[PDF](Attachments/{}.pdf)", a.id),
                AttKind::File { .. } => format!("[Attachment](Attachments/{})", a.id),
                _ => att_plain(a),
            });
            continue;
        }
        let mut s = t.to_string();
        if s.trim().is_empty() {
            out.push_str(&s);
            continue;
        }
        if f.strike {
            s = format!("~~{s}~~");
        }
        if f.underline {
            s = format!("++{s}++");
        }
        if f.highlight.is_some() {
            s = format!("=={s}==");
        }
        s = match (f.bold, f.italic) {
            (true, true) => format!("***{s}***"),
            (true, false) => format!("**{s}**"),
            (false, true) => format!("*{s}*"),
            _ => s,
        };
        if let Some(u) = &f.link {
            s = format!("[{s}]({u})");
        }
        out.push_str(&s);
    }
    out
}

fn table_md(a: &super::doc::Att) -> String {
    let AttKind::Table(t) = &a.kind else { return String::new() };
    let mut lines = Vec::new();
    for r in 0..t.rows() {
        let cells: Vec<String> = (0..t.cols()).map(|c| t.cell_text(r, c).replace('\n', " ")).collect();
        lines.push(format!("| {} |", cells.join(" | ")));
        if r == 0 {
            lines.push(format!("| {} |", vec!["--"; t.cols()].join(" | ")));
        }
    }
    lines.join("\n")
}

pub fn frag_to_markdown(doc: &Doc, frag: &Frag) -> String {
    let mut out = Vec::new();
    let mut in_code = false;
    for p in &frag.paras {
        let text = inline(doc, frag, p);
        if p.style == ParaStyle::Monostyled {
            if !in_code {
                out.push("```".to_string());
                in_code = true;
            }
            out.push(p.text.clone());
            continue;
        } else if in_code {
            out.push("```".to_string());
            in_code = false;
        }
        let pad = "    ".repeat(p.indent as usize);
        let line = match p.style {
            ParaStyle::Title => format!("# {text}"),
            ParaStyle::Heading => format!("## {text}"),
            ParaStyle::Subheading => format!("### {text}"),
            ParaStyle::Bulleted => format!("{pad}* {text}"),
            ParaStyle::Dashed => format!("{pad}- {text}"),
            ParaStyle::Numbered => format!("{pad}1. {text}"),
            ParaStyle::Checklist => format!("{pad}- [{}] {text}", if p.checked { "x" } else { " " }),
            _ => format!("{text}  "),
        };
        out.push(if p.quote { format!("> {line}") } else { line });
    }
    if in_code {
        out.push("```".to_string());
    }
    out.join("\n")
}

/// Edit > Paste as Markdown: Markdown read back with the same mapping (and the common
/// variants: "+ ", "_x_", "__x__", "1)", tab indents, <url>), as a fragment to paste.
pub fn markdown_to_frag(md: &str) -> Frag {
    use super::doc::{Att, Fmt, Table};
    let mut paras: Vec<Para> = Vec::new();
    let mut atts: Vec<Att> = Vec::new();
    let mut in_code = false;
    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;
    let block = |kind: AttKind, uti: &str, atts: &mut Vec<Att>| {
        let a = Att::new(uti, kind);
        let mut p = Para::default();
        p.insert(0, "\u{FFFC}", &Fmt { att: Some(a.id.clone()), ..Default::default() });
        atts.push(a);
        p
    };
    while i < lines.len() {
        let raw = lines[i];
        i += 1;
        if raw.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            paras.push(Para::new(raw, ParaStyle::Monostyled));
            continue;
        }
        // a pipe table: its header, the --- row, the rest
        if raw.trim_start().starts_with('|') && lines.get(i).is_some_and(|l| l.trim_start().starts_with('|') && l.contains("--")) {
            let row = |l: &str| -> Vec<String> { l.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect() };
            let mut rows = vec![row(raw)];
            i += 1;
            while i < lines.len() && lines[i].trim_start().starts_with('|') {
                rows.push(row(lines[i]));
                i += 1;
            }
            let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1);
            let cells = rows.iter().map(|r| (0..cols).map(|c| vec![inline_md(r.get(c).map_or("", |s| s.as_str()), ParaStyle::Body)]).collect()).collect();
            paras.push(block(AttKind::Table(Table { cells, widths: vec![None; cols], rtl: false }), nt_core::model::UTI_TABLE, &mut atts));
            continue;
        }
        let t = raw.trim_end().trim_end_matches("  ");
        if matches!(t.trim(), "---" | "***" | "___") {
            paras.push(block(AttKind::Divider, nt_core::model::UTI_DIVIDER, &mut atts));
            continue;
        }
        let (mut quote, mut t) = (false, t);
        while let Some(r) = t.strip_prefix("> ").or(t.strip_prefix('>').filter(|r| r.is_empty())) {
            quote = true;
            t = r;
        }
        // indents: four spaces or a tab a level (two spaces too)
        let ws: String = t.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let w: usize = ws.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum();
        let step = if ws.contains('\t') || w % 4 == 0 { 4 } else { 2 };
        let indent = (w / step).min(8) as u8;
        let body = &t[ws.len()..];
        let numbered = body.find(|c: char| !c.is_ascii_digit()).filter(|&n| n > 0 && (body[n..].starts_with(". ") || body[n..].starts_with(") ")));
        let (style, rest, checked, start) = if let Some(r) = body.strip_prefix("# ") {
            (ParaStyle::Title, r, false, None)
        } else if let Some(r) = body.strip_prefix("## ") {
            (ParaStyle::Heading, r, false, None)
        } else if let Some(r) = body.strip_prefix("### ").or(body.strip_prefix("#### ")).or(body.strip_prefix("##### ")).or(body.strip_prefix("###### ")) {
            (ParaStyle::Subheading, r, false, None)
        } else if let Some(r) = ["- [ ] ", "* [ ] ", "+ [ ] "].iter().find_map(|p| body.strip_prefix(p)) {
            (ParaStyle::Checklist, r, false, None)
        } else if let Some(r) = ["- [x] ", "- [X] ", "* [x] ", "+ [x] "].iter().find_map(|p| body.strip_prefix(p)) {
            (ParaStyle::Checklist, r, true, None)
        } else if let Some(r) = body.strip_prefix("* ").or(body.strip_prefix("+ ")).or(body.strip_prefix("• ")) {
            (ParaStyle::Bulleted, r, false, None)
        } else if let Some(r) = body.strip_prefix("- ") {
            (ParaStyle::Dashed, r, false, None)
        } else if let Some(n) = numbered {
            (ParaStyle::Numbered, &body[n + 2..], false, body[..n].parse::<u32>().ok())
        } else {
            (ParaStyle::Body, body, false, None)
        };
        let mut p = inline_md(rest, style);
        p.indent = if p.is_list() { indent } else { 0 };
        p.quote = quote;
        p.checked = checked;
        // (a numbered list keeps where it starts)
        if style == ParaStyle::Numbered && paras.last().is_none_or(|q| q.style != ParaStyle::Numbered) && start != Some(1) {
            p.start = start;
        }
        paras.push(p);
    }
    // (a trailing empty line is the clipboard's end, not a paragraph)
    while paras.len() > 1 && paras.last().is_some_and(|p| p.text.is_empty() && p.style == ParaStyle::Body) {
        paras.pop();
    }
    Frag { paras, atts, open_end: false }
}

/// One line's inline Markdown: **bold**, *italic*, ~~strike~~, ==highlight==, ++underline++,
/// `code` (as its text), [text](url) and <url> links, \-escapes.
fn inline_md(s: &str, style: ParaStyle) -> Para {
    use super::doc::Fmt;
    let mut p = Para { style, ..Default::default() };
    let mut f = Fmt::default();
    let cs: Vec<char> = s.chars().collect();
    let mut buf = String::new();
    let flush = |p: &mut Para, buf: &mut String, f: &Fmt| {
        if !buf.is_empty() {
            let at = p.text.len();
            p.insert(at, buf, f);
            buf.clear();
        }
    };
    let mut i = 0;
    let at = |i: usize, pat: &str| cs[i..].iter().take(pat.chars().count()).copied().eq(pat.chars());
    // (a marker only where its closing one follows)
    let closes = |i: usize, pat: &str| {
        let n = pat.chars().count();
        (i + n..cs.len()).any(|j| cs[j..].iter().take(n).copied().eq(pat.chars()) && j > i + n - 1)
    };
    while i < cs.len() {
        let c = cs[i];
        if c == '\\' && i + 1 < cs.len() && "\\`*_{}[]()#+-.!~=|<>".contains(cs[i + 1]) {
            buf.push(cs[i + 1]);
            i += 2;
            continue;
        }
        if c == '[' {
            // [text](url)
            if let Some(close) = (i + 1..cs.len()).find(|&j| cs[j] == ']') {
                if cs.get(close + 1) == Some(&'(') {
                    if let Some(end) = (close + 2..cs.len()).find(|&j| cs[j] == ')') {
                        flush(&mut p, &mut buf, &f);
                        let text: String = cs[i + 1..close].iter().collect();
                        let url: String = cs[close + 2..end].iter().collect();
                        let inner = inline_md(&text, style);
                        let lf = Fmt { link: Some(url.trim().to_string()), ..f.clone() };
                        let at = p.text.len();
                        p.insert(at, &inner.text, &lf);
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        if c == '<' {
            if let Some(end) = (i + 1..cs.len()).find(|&j| cs[j] == '>') {
                let url: String = cs[i + 1..end].iter().collect();
                if url.contains("://") || url.starts_with("mailto:") {
                    flush(&mut p, &mut buf, &f);
                    let at = p.text.len();
                    p.insert(at, &url, &Fmt { link: Some(url.clone()), ..f.clone() });
                    i = end + 1;
                    continue;
                }
            }
        }
        if c == '`' {
            if let Some(end) = (i + 1..cs.len()).find(|&j| cs[j] == '`') {
                buf.extend(&cs[i + 1..end]);
                i = end + 1;
                continue;
            }
        }
        let mut toggled = false;
        for (pat, which) in [("***", 0), ("___", 0), ("**", 1), ("__", 1), ("~~", 2), ("==", 3), ("++", 4), ("*", 5), ("_", 5)] {
            if !at(i, pat) {
                continue;
            }
            let on = match which {
                0 => f.bold && f.italic,
                1 => f.bold,
                2 => f.strike,
                3 => f.highlight.is_some(),
                4 => f.underline,
                _ => f.italic,
            };
            // (an opening marker needs a closing one; "_" inside a word is a character)
            if !on && !closes(i, pat) || pat == "_" && i > 0 && cs[i - 1].is_alphanumeric() {
                break;
            }
            flush(&mut p, &mut buf, &f);
            match which {
                0 => {
                    f.bold = !on;
                    f.italic = !on;
                }
                1 => f.bold = !on,
                2 => f.strike = !on,
                3 => f.highlight = if on { None } else { Some(1) },
                4 => f.underline = !on,
                _ => f.italic = !on,
            }
            i += pat.chars().count();
            toggled = true;
            break;
        }
        if toggled {
            continue;
        }
        buf.push(c);
        i += 1;
    }
    flush(&mut p, &mut buf, &f);
    p
}
