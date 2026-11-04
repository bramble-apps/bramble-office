//! Exports: Markdown (exactly as Notes 4.13's File > Export To > Markdown writes it, format
//! section 8), plain text and HTML. Attachments are referenced as `Attachments/<attachment UUID>.<ext>`
//! next to the exported file, as Notes does.

use std::path::{Path, PathBuf};

use crate::model::*;

/// An exported note: its text and the attachment files it refers to (relative path -> source file).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Exported {
    pub text: String,
    pub files: Vec<(String, PathBuf)>,
}

/// `Attachments/<uuid>.<ext>` for an attachment (drawings export their fallback image).
fn attachment_path(a: &Attachment) -> String {
    let ext = a
        .display_file()
        .and_then(|p| p.extension())
        .map(|e| e.to_string_lossy().into_owned())
        .or_else(|| Path::new(&a.name).extension().map(|e| e.to_string_lossy().into_owned()))
        .unwrap_or_else(|| match a.kind {
            AttachmentKind::Image | AttachmentKind::Drawing | AttachmentKind::Scan => "png".into(),
            AttachmentKind::Pdf => "pdf".into(),
            _ => "data".into(),
        });
    format!("Attachments/{}.{ext}", a.id)
}

fn add_file(files: &mut Vec<(String, PathBuf)>, a: &Attachment) -> String {
    let rel = attachment_path(a);
    if let Some(src) = a.display_file() {
        if !files.iter().any(|(r, _)| *r == rel) {
            files.push((rel.clone(), src.clone()));
        }
    }
    rel
}

// ------------------------------------------------------------------ Markdown

/// A note as Notes' Markdown export writes it.
pub fn markdown(note: &Note) -> Exported {
    let mut out = String::new();
    let mut files = Vec::new();
    let paras = &note.body.paragraphs;
    // the empty paragraph after a final '\n' isn't written
    let n = if paras.len() > 1 && paras.last().is_some_and(|p| p.spans.is_empty()) { paras.len() - 1 } else { paras.len() };
    let mut counters: Vec<u32> = Vec::new();
    let mut in_code = false;
    for p in &paras[..n] {
        if p.style == ParaStyle::Monostyled {
            if !in_code {
                out.push_str("```\n");
                in_code = true;
            }
            out.push_str(&p.text().replace(OBJ, ""));
            out.push('\n');
            continue;
        }
        if in_code {
            out.push_str("```\n");
            in_code = false;
        }
        if p.style != ParaStyle::Numbered {
            counters.truncate(if p.style.is_list() { p.indent as usize } else { 0 });
        }
        let mut line = String::new();
        if p.block_quote {
            line.push_str("> ");
        }
        let pad = "    ".repeat(p.indent as usize);
        match p.style {
            ParaStyle::Title => line.push_str("# "),
            ParaStyle::Heading => line.push_str("## "),
            ParaStyle::Subheading => line.push_str("### "),
            ParaStyle::Bulleted => line.push_str(&format!("{pad}* ")),
            ParaStyle::Dashed => line.push_str(&format!("{pad}- ")),
            ParaStyle::Checklist => line.push_str(&format!("{pad}- [{}] ", if p.checked { "x" } else { " " })),
            ParaStyle::Numbered => {
                let lvl = p.indent as usize;
                counters.truncate(lvl + 1);
                while counters.len() <= lvl {
                    counters.push(0);
                }
                counters[lvl] = if counters[lvl] == 0 { p.start_number.unwrap_or(1).max(1) } else { counters[lvl] + 1 };
                line.push_str(&format!("{pad}{}. ", counters[lvl]));
            }
            ParaStyle::Body | ParaStyle::Monostyled => {}
        }
        // headings are bold already: Notes doesn't mark bold runs in them
        let heading = matches!(p.style, ParaStyle::Title | ParaStyle::Heading | ParaStyle::Subheading);
        for s in &p.spans {
            let s = if heading && s.bold { &Span { bold: false, ..s.clone() } } else { s };
            line.push_str(&md_span(s, note, &mut files));
        }
        out.push_str(&line);
        out.push_str("  \n");
    }
    if in_code {
        out.push_str("```\n");
    }
    Exported { text: out, files }
}

fn md_span(s: &Span, note: &Note, files: &mut Vec<(String, PathBuf)>) -> String {
    if let Some(t) = &s.token {
        return md_token(t, note, files);
    }
    if s.text.is_empty() {
        return String::new();
    }
    let mut open = String::new();
    // a bold system font on imported text counts as bold again (Notes writes "****x****")
    let font_bold = s.font.as_ref().and_then(|f| f.name.as_deref()).is_some_and(|n| n.contains("Bold"));
    for _ in 0..(u8::from(s.bold) + u8::from(font_bold && s.bold)) {
        open.push_str("**");
    }
    if s.italic {
        open.push('*');
    }
    if s.strike {
        open.push_str("~~");
    }
    if s.underline {
        open.push_str("++");
    }
    if s.highlight.is_some() {
        open.push_str("==");
    }
    let close: String = open.chars().rev().collect();
    let inner = format!("{open}{}{close}", s.text);
    match &s.link {
        Some(url) => format!("[{inner}]({url})"),
        None => inner,
    }
}

fn md_token(t: &Token, note: &Note, files: &mut Vec<(String, PathBuf)>) -> String {
    match t.kind() {
        TokenKind::Divider => "\n---\n".into(),
        TokenKind::Attachment => {
            let Some(a) = note.attachment(&t.id) else { return String::new() };
            match a.kind {
                AttachmentKind::Table => a.table.as_ref().map(|tb| format!("\n{}", md_table(tb))).unwrap_or_default(),
                AttachmentKind::Image | AttachmentKind::Scan => format!("![Image]({})", add_file(files, a)),
                AttachmentKind::Drawing => format!("![Sketch]({})", add_file(files, a)),
                AttachmentKind::Pdf => format!("[PDF]({})", add_file(files, a)),
                AttachmentKind::Url => {
                    let url = a.url.clone().unwrap_or_default();
                    format!("[{}]({url})", if a.name.is_empty() { &url } else { &a.name })
                }
                _ => format!("[Attachment]({})", add_file(files, a)),
            }
        }
        // tags, note links (the link itself is lost, as in Notes), math results, mentions
        _ => t.alt.clone(),
    }
}

/// A pipe table: cells padded to the column width (at least 2), "| -- |" separator after row 1.
fn md_table(t: &Table) -> String {
    let grid: Vec<Vec<String>> = t.rows.iter().map(|r| r.iter().map(|c| c.text().replace('\n', " ").replace(OBJ, "")).collect()).collect();
    let cols = t.cols();
    let width = |c: usize| grid.iter().map(|r| r.get(c).map(|s| s.chars().count()).unwrap_or(0)).max().unwrap_or(0).max(2);
    let widths: Vec<usize> = (0..cols).map(width).collect();
    let row = |cells: Vec<String>| {
        let mut s = String::from("|");
        for (c, w) in widths.iter().enumerate() {
            let v = cells.get(c).cloned().unwrap_or_default();
            s.push_str(&format!(" {v}{} |", " ".repeat(w - v.chars().count())));
        }
        s.push('\n');
        s
    };
    let mut out = String::new();
    for (i, r) in grid.iter().enumerate() {
        out.push_str(&row(r.clone()));
        if i == 0 {
            out.push_str(&row(widths.iter().map(|w| "-".repeat(*w)).collect()));
        }
    }
    out
}

// ------------------------------------------------------------------ plain text

/// A note as plain text: one line per paragraph, list markers, tokens as their text, attachments
/// as "[name]", tables as tab-separated rows.
pub fn plain_text(note: &Note) -> String {
    let mut out = Vec::new();
    let mut counters: Vec<u32> = Vec::new();
    for p in &note.body.paragraphs {
        if p.style != ParaStyle::Numbered {
            counters.truncate(if p.style.is_list() { p.indent as usize } else { 0 });
        }
        let pad = "\t".repeat(p.indent as usize);
        let marker = match p.style {
            ParaStyle::Bulleted => format!("{pad}• "),
            ParaStyle::Dashed => format!("{pad}- "),
            ParaStyle::Checklist => format!("{pad}{} ", if p.checked { "☑" } else { "☐" }),
            ParaStyle::Numbered => {
                let lvl = p.indent as usize;
                counters.truncate(lvl + 1);
                while counters.len() <= lvl {
                    counters.push(0);
                }
                counters[lvl] = if counters[lvl] == 0 { p.start_number.unwrap_or(1).max(1) } else { counters[lvl] + 1 };
                format!("{pad}{}. ", counters[lvl])
            }
            _ => String::new(),
        };
        let mut line = marker;
        for s in &p.spans {
            match &s.token {
                None => line.push_str(&s.text),
                Some(t) if t.is_inline() => line.push_str(if t.uti == UTI_DIVIDER { "---" } else { &t.alt }),
                Some(t) => match note.attachment(&t.id) {
                    Some(a) if a.kind == AttachmentKind::Table => {
                        if let Some(tb) = &a.table {
                            line.push_str(&tb.text_grid().iter().map(|r| r.join("\t")).collect::<Vec<_>>().join("\n"));
                        }
                    }
                    Some(a) if a.kind == AttachmentKind::Url => line.push_str(a.url.as_deref().unwrap_or("")),
                    Some(a) => line.push_str(&format!("[{}]", if a.name.is_empty() { kind_name(a.kind) } else { &a.name })),
                    None => {}
                },
            }
        }
        out.push(line);
    }
    let mut s = out.join("\n");
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

fn kind_name(k: AttachmentKind) -> &'static str {
    match k {
        AttachmentKind::Image => "Image",
        AttachmentKind::Pdf => "PDF",
        AttachmentKind::Drawing => "Sketch",
        AttachmentKind::Table => "Table",
        AttachmentKind::Url => "Link",
        AttachmentKind::Audio => "Audio",
        AttachmentKind::Scan => "Scan",
        AttachmentKind::File => "Attachment",
    }
}

// ------------------------------------------------------------------ HTML

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

const HTML_STYLE: &str = "body{font:15px/1.45 -apple-system,'Helvetica Neue',sans-serif;max-width:46em;margin:2em auto;padding:0 1em;color:#1d1d1f}\
h1{font-size:1.75em}h2{font-size:1.35em}h3{font-size:1.1em}pre{background:#f4f4f4;padding:.6em;border-radius:6px;white-space:pre-wrap}\
blockquote{border-left:3px solid #ccc;margin:0;padding-left:.8em}ul.checklist{list-style:none;padding-left:1.2em}\
li.checked{color:#888;text-decoration:line-through}table{border-collapse:collapse}td{border:1px solid #ccc;padding:.3em .6em;vertical-align:top}\
img{max-width:100%}.tag{color:#c78a00}mark.hl1{background:#e5d5f5}mark.hl2{background:#f9d3e3}mark.hl3{background:#fde3c3}\
mark.hl4{background:#cdefe2}mark.hl5{background:#d2e4f9}";

/// A note as a standalone HTML page.
pub fn html(note: &Note) -> Exported {
    let mut files = Vec::new();
    let body = html_body(&note.body, note, &mut files);
    let text = format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n<style>{HTML_STYLE}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        esc(&note.title)
    );
    Exported { text, files }
}

/// A body as HTML elements (no page around it).
pub fn html_body(b: &Body, note: &Note, files: &mut Vec<(String, PathBuf)>) -> String {
    let mut out = String::new();
    // open lists: (tag, indent)
    let mut lists: Vec<(&'static str, u8)> = Vec::new();
    let paras = &b.paragraphs;
    let n = if paras.len() > 1 && paras.last().is_some_and(|p| p.spans.is_empty()) { paras.len() - 1 } else { paras.len() };
    let mut i = 0;
    while i < n {
        let p = &paras[i];
        let list_tag = match p.style {
            ParaStyle::Bulleted | ParaStyle::Dashed => Some("ul"),
            ParaStyle::Numbered => Some("ol"),
            ParaStyle::Checklist => Some("ul class=\"checklist\""),
            _ => None,
        };
        // close lists that are deeper, or the same depth but another kind; non-list paragraphs close all
        while let Some(&(tag, ind)) = lists.last() {
            let keep = list_tag.is_some_and(|t| ind < p.indent || (ind == p.indent && t == tag));
            if keep {
                break;
            }
            out.push_str(&format!("</{}>\n", tag.split(' ').next().unwrap()));
            lists.pop();
        }
        if let Some(tag) = list_tag {
            if lists.last().is_none_or(|&(_, ind)| ind < p.indent) {
                let start = match (tag, p.start_number) {
                    ("ol", Some(s)) if s > 1 => format!(" start=\"{s}\""),
                    _ => String::new(),
                };
                out.push_str(&format!("<{tag}{start}>\n"));
                lists.push((tag, p.indent));
            }
        }
        let inner = html_spans(&p.spans, note, files);
        let align = match p.align {
            Align::Center => " style=\"text-align:center\"",
            Align::Right => " style=\"text-align:right\"",
            Align::Justified => " style=\"text-align:justify\"",
            _ => "",
        };
        let el = match p.style {
            ParaStyle::Title => format!("<h1{align}>{inner}</h1>\n"),
            ParaStyle::Heading => format!("<h2{align}>{inner}</h2>\n"),
            ParaStyle::Subheading => format!("<h3{align}>{inner}</h3>\n"),
            ParaStyle::Monostyled => {
                // consecutive monostyled paragraphs are one block
                let mut lines = vec![inner];
                while i + 1 < n && paras[i + 1].style == ParaStyle::Monostyled {
                    i += 1;
                    lines.push(html_spans(&paras[i].spans, note, files));
                }
                format!("<pre>{}</pre>\n", lines.join("\n"))
            }
            ParaStyle::Checklist => format!("<li{}>{inner}</li>\n", if p.checked { " class=\"checked\"" } else { "" }),
            ParaStyle::Bulleted | ParaStyle::Dashed | ParaStyle::Numbered => format!("<li>{inner}</li>\n"),
            ParaStyle::Body => {
                let indent = if p.indent > 0 { format!(" style=\"margin-left:{}em\"", p.indent as f32 * 1.5) } else { String::new() };
                if inner.is_empty() { "<p><br></p>\n".into() } else { format!("<p{}{}>{inner}</p>\n", if indent.is_empty() { align } else { "" }, indent) }
            }
        };
        if p.block_quote {
            out.push_str(&format!("<blockquote>{}</blockquote>\n", el.trim_end()));
        } else {
            out.push_str(&el);
        }
        i += 1;
    }
    while let Some((tag, _)) = lists.pop() {
        out.push_str(&format!("</{}>\n", tag.split(' ').next().unwrap()));
    }
    out
}

fn html_spans(spans: &[Span], note: &Note, files: &mut Vec<(String, PathBuf)>) -> String {
    let mut out = String::new();
    for s in spans {
        if let Some(t) = &s.token {
            out.push_str(&html_token(t, note, files));
            continue;
        }
        let mut open = String::new();
        let mut close = Vec::new();
        let mut wrap = |o: String, c: &str| {
            open.push_str(&o);
            close.push(c.to_string());
        };
        if let Some(l) = &s.link {
            wrap(format!("<a href=\"{}\">", esc(l)), "</a>");
        }
        if let Some(h) = s.highlight {
            wrap(format!("<mark class=\"hl{h}\">"), "</mark>");
        }
        if s.bold {
            wrap("<b>".into(), "</b>");
        }
        if s.italic {
            wrap("<i>".into(), "</i>");
        }
        if s.underline {
            wrap("<u>".into(), "</u>");
        }
        if s.strike {
            wrap("<s>".into(), "</s>");
        }
        if s.script > 0 {
            wrap("<sup>".into(), "</sup>");
        } else if s.script < 0 {
            wrap("<sub>".into(), "</sub>");
        }
        if let Some([r, g, b, _]) = s.color {
            let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            wrap(format!("<span style=\"color:#{:02x}{:02x}{:02x}\">", c(r), c(g), c(b)), "</span>");
        }
        out.push_str(&open);
        out.push_str(&esc(&s.text));
        for c in close.iter().rev() {
            out.push_str(c);
        }
    }
    out
}

fn html_token(t: &Token, note: &Note, files: &mut Vec<(String, PathBuf)>) -> String {
    match t.kind() {
        TokenKind::Divider => "<hr>".into(),
        TokenKind::Tag => format!("<span class=\"tag\">{}</span>", esc(&t.alt)),
        TokenKind::NoteLink { .. } => format!("<a href=\"{}\">{}</a>", esc(&t.content), esc(&t.alt)),
        TokenKind::Attachment => {
            let Some(a) = note.attachment(&t.id) else { return String::new() };
            match a.kind {
                AttachmentKind::Table => {
                    let Some(tb) = &a.table else { return String::new() };
                    let mut s = String::from(if tb.rtl { "<table dir=\"rtl\">" } else { "<table>" });
                    for r in &tb.rows {
                        s.push_str("<tr>");
                        for c in r {
                            let cell: Vec<String> = c.paragraphs.iter().map(|p| html_spans(&p.spans, note, files)).collect();
                            s.push_str(&format!("<td>{}</td>", cell.join("<br>")));
                        }
                        s.push_str("</tr>");
                    }
                    s.push_str("</table>");
                    s
                }
                AttachmentKind::Image | AttachmentKind::Drawing | AttachmentKind::Scan => {
                    format!("<img src=\"{}\" alt=\"{}\">", esc(&add_file(files, a)), esc(&a.name))
                }
                AttachmentKind::Url => {
                    let url = a.url.clone().unwrap_or_default();
                    format!("<a href=\"{}\">{}</a>", esc(&url), esc(if a.name.is_empty() { &url } else { &a.name }))
                }
                _ => format!("<a href=\"{}\">{}</a>", esc(&add_file(files, a)), esc(if a.name.is_empty() { kind_name(a.kind) } else { &a.name })),
            }
        }
        _ => esc(&t.alt),
    }
}

// ------------------------------------------------------------------ writing files

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Markdown,
    PlainText,
    Html,
}

/// A file name from a note title (no path separators; "Untitled" when empty).
pub fn file_stem(title: &str) -> String {
    let s: String = title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '\0') { '-' } else { c }).collect();
    let s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() { "Untitled".into() } else { s.chars().take(200).collect() }
}

/// Write notes into `dir` as `<title>.md|.txt|.html` (+ `Attachments/`), like Notes' multi-note
/// export. Returns the files written.
pub fn write_notes(notes: &[&Note], dir: &Path, format: Format) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let ext = match format {
        Format::Markdown => "md",
        Format::PlainText => "txt",
        Format::Html => "html",
    };
    let mut written = Vec::new();
    let mut used = std::collections::HashSet::new();
    for n in notes {
        let ex = match format {
            Format::Markdown => markdown(n),
            Format::PlainText => Exported { text: plain_text(n), files: Vec::new() },
            Format::Html => html(n),
        };
        let stem = file_stem(&n.title);
        let mut name = format!("{stem}.{ext}");
        let mut k = 2;
        while !used.insert(name.clone()) || (dir.join(&name).exists() && !written.contains(&dir.join(&name))) {
            name = format!("{stem} {k}.{ext}");
            k += 1;
        }
        let path = dir.join(&name);
        std::fs::write(&path, &ex.text)?;
        written.push(path);
        for (rel, src) in ex.files {
            let to = dir.join(&rel);
            if let Some(p) = to.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(&src, &to)?;
        }
    }
    Ok(written)
}

// ------------------------------------------------------------------ export options

/// The export sheet's "Collapsible Section": every section expanded, every one collapsed (HTML's
/// `<details>` closed), or as the note shows them (its collapsed headings).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sections {
    #[default]
    ExpandAll,
    CollapseAll,
    AsShown,
}

/// A heading's level (Title 1, Heading 2, Subheading 3): what a collapsed section runs to.
fn level(p: &Paragraph) -> Option<u8> {
    match p.style {
        ParaStyle::Title => Some(1),
        ParaStyle::Heading => Some(2),
        ParaStyle::Subheading => Some(3),
        _ => None,
    }
}

/// Where the section under the heading at `i` ends (the next heading of its level or above).
fn section_end(paras: &[Paragraph], i: usize) -> usize {
    let l = level(&paras[i]).unwrap_or(0);
    (i + 1..paras.len()).find(|&j| level(&paras[j]).is_some_and(|m| m <= l)).unwrap_or(paras.len())
}

/// The body as the note shows it: what's under a collapsed heading left out (the heading kept).
/// For PDF's "As shown"; `ExpandAll` (and `CollapseAll`, which a page can't fold) give it whole.
pub fn shown_body(b: &Body, sections: Sections) -> Body {
    if sections != Sections::AsShown {
        return b.clone();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.paragraphs.len() {
        let p = &b.paragraphs[i];
        out.push(p.clone());
        i = if p.collapsed && level(p).is_some() { section_end(&b.paragraphs, i) } else { i + 1 };
    }
    Body { paragraphs: out }
}

const DETAILS_STYLE: &str = "details>summary{list-style:none;cursor:pointer}details>summary::-webkit-details-marker{display:none}\
details>summary>h1,details>summary>h2,details>summary>h3{display:inline-block}\
details>summary::before{content:'\\25BE';display:inline-block;width:1em;margin-left:-1em;color:#c78a00;transition:transform .15s}\
details:not([open])>summary::before{transform:rotate(-90deg)}";

/// The paragraphs as HTML, each heading's section a `<details>` (open or not by `sections`).
fn html_sections(paras: &[Paragraph], note: &Note, files: &mut Vec<(String, PathBuf)>, sections: Sections) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < paras.len() {
        if level(&paras[i]).is_some() {
            let end = section_end(paras, i);
            let head = html_body(&Body { paragraphs: vec![paras[i].clone()] }, note, files);
            let inner = html_sections(&paras[i + 1..end], note, files, sections);
            if inner.trim().is_empty() {
                out.push_str(&head);
            } else {
                let open = match sections {
                    Sections::ExpandAll => true,
                    Sections::CollapseAll => false,
                    Sections::AsShown => !paras[i].collapsed,
                };
                out.push_str(&format!("<details{}>\n<summary>{}</summary>\n{inner}</details>\n", if open { " open" } else { "" }, head.trim_end()));
            }
            i = end;
        } else {
            let run_end = (i..paras.len()).find(|&j| level(&paras[j]).is_some()).unwrap_or(paras.len());
            out.push_str(&html_body(&Body { paragraphs: paras[i..run_end].to_vec() }, note, files));
            i = run_end;
        }
    }
    out
}

/// A note as a standalone HTML page whose headings fold (`<details>`), open or shut by `sections`.
pub fn html_with(note: &Note, sections: Sections) -> Exported {
    let mut files = Vec::new();
    let paras = &note.body.paragraphs;
    let n = if paras.len() > 1 && paras.last().is_some_and(|p| p.spans.is_empty()) { paras.len() - 1 } else { paras.len() };
    let body = html_sections(&paras[..n], note, &mut files, sections);
    let text = format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n<style>{HTML_STYLE}{DETAILS_STYLE}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        esc(&note.title)
    );
    Exported { text, files }
}

/// Unix seconds as RFC 3339 in UTC ("2024-03-05T14:02:00Z").
fn rfc3339(t: f64) -> String {
    let secs = t.floor() as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // (Howard Hinnant's days-to-civil)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

/// A YAML double-quoted string.
fn yaml(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " "))
}

/// Markdown with, at its top, YAML front matter: the note's title, when it was made and last
/// edited, its folder (`folder`: its path, "Notes/Recipes") and its tags. (The export sheet's
/// "Include additional data".)
pub fn markdown_with_data(note: &Note, folder: &str) -> Exported {
    let mut ex = markdown(note);
    let mut fm = String::from("---\n");
    fm.push_str(&format!("title: {}\n", yaml(&note.title)));
    if note.created > 0.0 {
        fm.push_str(&format!("created: {}\n", rfc3339(note.created)));
    }
    if note.modified > 0.0 {
        fm.push_str(&format!("updated: {}\n", rfc3339(note.modified)));
    }
    if !folder.is_empty() {
        fm.push_str(&format!("folder: {}\n", yaml(folder)));
    }
    let tags = note.tags();
    if !tags.is_empty() {
        fm.push_str(&format!("tags: [{}]\n", tags.iter().map(|t| yaml(t)).collect::<Vec<_>>().join(", ")));
    }
    if note.pinned {
        fm.push_str("pinned: true\n");
    }
    fm.push_str("---\n\n");
    ex.text = fm + &ex.text;
    ex
}

/// How the export sheet writes a note.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub sections: Sections,
    /// Markdown: the front matter (dates, folder, tags)
    pub data: bool,
}

/// One note in `format` with the sheet's options (`folder`: its folder's path, for Markdown's data).
pub fn export_with(note: &Note, format: Format, opts: &Options, folder: &str) -> Exported {
    match format {
        Format::Markdown if opts.data => markdown_with_data(note, folder),
        Format::Markdown => markdown(note),
        Format::PlainText => Exported { text: plain_text(&Note { body: shown_body(&note.body, opts.sections), ..note.clone() }), files: Vec::new() },
        Format::Html => html_with(note, opts.sections),
    }
}

/// [`write_notes`] with the sheet's options: each note into `dir` (its attachments in
/// `Attachments/`), `folder_of` naming each note's folder for Markdown's data.
pub fn write_notes_with(notes: &[&Note], dir: &Path, format: Format, opts: &Options, folder_of: &dyn Fn(&Note) -> String) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let ext = match format {
        Format::Markdown => "md",
        Format::PlainText => "txt",
        Format::Html => "html",
    };
    let mut written = Vec::new();
    let mut used = std::collections::HashSet::new();
    for n in notes {
        let ex = export_with(n, format, opts, &folder_of(n));
        let stem = file_stem(&n.title);
        let mut name = format!("{stem}.{ext}");
        let mut k = 2;
        while !used.insert(name.clone()) || (dir.join(&name).exists() && !written.contains(&dir.join(&name))) {
            name = format!("{stem} {k}.{ext}");
            k += 1;
        }
        let path = dir.join(&name);
        std::fs::write(&path, &ex.text)?;
        written.push(path);
        for (rel, src) in ex.files {
            let to = dir.join(&rel);
            if let Some(p) = to.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(&src, &to)?;
        }
    }
    Ok(written)
}

#[cfg(test)]
mod option_tests {
    use super::*;

    fn note() -> Note {
        let mut h = Paragraph::plain(ParaStyle::Heading, "Plan");
        h.collapsed = true;
        let body = Body {
            paragraphs: vec![
                Paragraph::plain(ParaStyle::Title, "Trip"),
                Paragraph::plain(ParaStyle::Body, "Intro"),
                h,
                Paragraph::plain(ParaStyle::Body, "Hidden step"),
                Paragraph::plain(ParaStyle::Subheading, "Sub"),
                Paragraph::plain(ParaStyle::Body, "Deeper"),
                Paragraph::plain(ParaStyle::Heading, "Open one"),
                Paragraph::plain(ParaStyle::Body, "Shown"),
            ],
        };
        Note { title: "Trip".into(), created: 1_709_647_320.0, modified: 1_709_733_720.0, body, ..Note::default() }
    }

    #[test]
    fn html_sections_fold() {
        let n = note();
        let all = html_with(&n, Sections::ExpandAll).text;
        assert_eq!(all.matches("<details open>").count(), 4);
        let shut = html_with(&n, Sections::CollapseAll).text;
        assert_eq!((shut.matches("<details>").count(), shut.matches("<details open>").count()), (4, 0));
        let shown = html_with(&n, Sections::AsShown).text;
        // (only Plan is collapsed in the note)
        assert_eq!(shown.matches("<details>").count(), 1);
        assert!(shown.contains("<summary><h2>Plan</h2></summary>"));
        // the section runs to the next heading of its level: Sub is inside Plan, Open one isn't
        let plan = shown.find("Plan").unwrap();
        let close = shown[plan..].find("</details>").unwrap() + plan;
        assert!(shown[plan..close].contains("Deeper") && !shown[plan..close].contains("Shown"));
    }

    #[test]
    fn shown_body_drops_collapsed() {
        let b = shown_body(&note().body, Sections::AsShown);
        let t: Vec<String> = b.paragraphs.iter().map(|p| p.text()).collect();
        assert_eq!(t, ["Trip", "Intro", "Plan", "Open one", "Shown"]);
        assert_eq!(shown_body(&note().body, Sections::ExpandAll).paragraphs.len(), 8);
    }

    #[test]
    fn markdown_front_matter() {
        let t = markdown_with_data(&note(), "Notes/Travel").text;
        assert!(t.starts_with("---\ntitle: \"Trip\"\ncreated: 2024-03-05T14:02:00Z\nupdated: 2024-03-06T14:02:00Z\nfolder: \"Notes/Travel\"\n---\n\n"), "{t}");
        assert!(t.contains("# Trip"));
    }
}
