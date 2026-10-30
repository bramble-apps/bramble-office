//! File > Export To: PDF, Word (.docx), EPUB, Plain Text, Rich Text (RTF), HTML and Images,
//! following what Pages' own exports keep (research exports/EXPORT-NOTES.txt): Word gets
//! native styles, lists, footnotes, comments, tracked changes, links, page breaks, columns,
//! headers / footers, pictures and tables; plain text is the body with changes accepted and
//! footnotes appended.

use std::io::Write;

use kn_format::ObjId;
use kn_model::{Align, CharStyle, DrawableKind, LineSpacing};

use crate::body::{self, BuildCtx, Break, SpanKind};
use crate::flow::{self, Layout};
use crate::Pages;

/// One paragraph as exporters see it.
pub struct Para {
    pub style_name: String,
    pub level: u32,
    /// Bullet / number label and whether it is numbered.
    pub list: Option<(String, bool)>,
    pub align: Align,
    pub space_before: f32,
    pub space_after: f32,
    pub line: f32,
    /// A fixed line height instead: (exact, points); false: at least.
    pub line_fixed: Option<(bool, f32)>,
    /// Keep with the next paragraph / keep its lines together.
    pub keep_next: bool,
    pub keep_lines: bool,
    /// Its tab stops.
    pub tabs: Vec<kn_model::TabStop>,
    /// Its list style (each one a Word list of its own on save).
    pub list_style: Option<ObjId>,
    pub left: f32,
    pub right: f32,
    pub first: f32,
    pub brk: Option<Break>,
    pub runs: Vec<RunOut>,
    /// Pictures and tables shown with this paragraph (inline ones, and floating ones of a page it starts).
    pub objects: Vec<ObjId>,
    pub toc: bool,
    /// Paragraph border: colour, width, edges (Pages' 1 top, 2 bottom, 15 box), distance.
    pub border: Option<(kn_model::Color, f32, u64, f32)>,
    /// An empty paragraph's character style (Word's paragraph mark: the empty line's height).
    pub mark: Option<CharStyle>,
}

pub struct RunOut {
    pub text: String,
    pub style: CharStyle,
    pub link: Option<String>,
    /// Tracked change: (deletion, author).
    pub change: Option<(bool, String)>,
    /// Comment starting here: (id, text, author).
    pub comment: Option<(ObjId, String, String)>,
    /// A footnote reference: its text.
    pub footnote: Option<String>,
    /// A page number (false) or page count (true) field.
    pub field: Option<bool>,
}

fn author_name(p: &Pages, id: Option<ObjId>) -> String {
    id.and_then(|a| p.file().obj(a)).and_then(|a| a.str("name")).unwrap_or("Author").to_string()
}

/// The document's paragraphs for export.
pub fn paragraphs(p: &Pages, layout: Option<&Layout>) -> Vec<Para> {
    match p.body_storage() {
        Some(s) => paragraphs_of(p, s, layout),
        None => Vec::new(),
    }
}

/// The paragraphs of any text storage (the body, a header field, a text box).
pub fn paragraphs_of(p: &Pages, storage: ObjId, layout: Option<&Layout>) -> Vec<Para> {
    let k = p.file();
    let Some(b) = body::build(k, storage, None, BuildCtx { show_markup: false, word_labels: p.word_layout, ..Default::default() }) else { return Vec::new() };
    let is_body = p.body_storage() == Some(storage);
    let layout = layout.filter(|_| is_body);
    let mut out = Vec::new();
    let mut page_first: Vec<(usize, ObjId)> = Vec::new();
    if let Some(l) = layout {
        for (pi, page) in l.pages.iter().enumerate() {
            for o in page.objects.iter().filter(|o| !o.inline) {
                if matches!(k.type_name(o.id), "TSD.ImageArchive" | "TST.TableInfoArchive") {
                    page_first.push((pi, o.id));
                }
            }
        }
    }
    for (i, para) in b.tb.paragraphs.iter().enumerate() {
        let ex = &b.extra[i];
        let style_name = crate::edit::named_style(&p.doc, ex.style).1;
        let mut runs = Vec::new();
        for r in &para.runs {
            if r.end <= r.start {
                continue;
            }
            let mut cuts = vec![r.start, r.end];
            for s in &b.spans {
                for c in [s.start, s.end] {
                    if c > r.start && c < r.end {
                        cuts.push(c);
                    }
                }
            }
            cuts.sort_unstable();
            cuts.dedup();
            for w in cuts.windows(2) {
                let (a, e) = (w[0], w[1]);
                let mut ro = RunOut { text: b.tb.text[a..e].to_string(), style: r.style.clone(), link: None, change: None, comment: None, footnote: None, field: None };
                for s in b.spans.iter().filter(|s| s.start <= a && e <= s.end) {
                    match &s.kind {
                        SpanKind::Link(u) => ro.link = Some(u.clone()),
                        SpanKind::Change { change, deletion, .. } => {
                            let session = k.obj(*change).and_then(|c| c.ref_id("session")).and_then(|s| k.obj(s)).and_then(|s| s.ref_id("author"));
                            ro.change = Some((*deletion, author_name(p, session)));
                        }
                        SpanKind::Comment { highlight, storage, .. } => {
                            let c = storage.and_then(|c| k.obj(c));
                            if s.start == a {
                                ro.comment = Some((*highlight, c.and_then(|c| c.str("text")).unwrap_or("").to_string(), author_name(p, c.and_then(|c| c.ref_id("author")))));
                            }
                        }
                        SpanKind::Footnote { storage, .. } => {
                            ro.footnote = Some(crate::text_plain(&p.doc.storage_text(*storage)).trim().to_string());
                            ro.text.clear();
                        }
                        // an inline object's stand-in (the object goes out with the paragraph)
                        SpanKind::Object(_) => ro.text.clear(),
                        SpanKind::PageField { count } => ro.field = Some(*count),
                        _ => {}
                    }
                }
                runs.push(ro);
            }
        }
        let line = match para.line_spacing {
            LineSpacing::Relative(r) => r,
            _ => 1.0,
        };
        let line_fixed = match para.line_spacing {
            LineSpacing::Exactly(v) => Some((true, v)),
            LineSpacing::AtLeast(v) => Some((false, v)),
            _ => None,
        };
        let mut objects: Vec<ObjId> = ex.drawables.iter().copied().filter(|d| matches!(k.type_name(*d), "TSD.ImageArchive" | "TST.TableInfoArchive" | "TSCH.ChartDrawableArchive" | "TSWP.ShapeInfoArchive" | "TSD.ShapeArchive" | "TSD.MovieArchive" | "TSD.GroupArchive")).collect();
        if let Some(l) = layout {
            if let Some(pi) = l.pages.iter().position(|pg| pg.lines.first().is_some_and(|ln| ln.para == i)) {
                objects.extend(page_first.iter().filter(|(p2, _)| *p2 == pi).map(|(_, id)| *id));
            }
        }
        let list = para.label.as_ref().map(|lb| match lb {
            kn_model::ListLabel::Number(s) => (s.clone(), true),
            kn_model::ListLabel::Bullet(s) => (s.clone(), false),
            kn_model::ListLabel::Image(_) => ("•".into(), false),
        });
        // a list: the text where Pages puts it (label position + the label's gap in ems), the
        // first line hanging back to the label (Word's indents for the same picture)
        let (left, first) = if list.is_some() {
            let em = para.runs.first().map(|r| r.style.size).unwrap_or(12.0);
            let hang = para.list_indent + para.label_indent * em;
            (para.left_indent + hang, -hang)
        } else {
            // (Pages measures the first line from the edge, Word from the left indent)
            (para.left_indent, para.first_line_indent - para.left_indent)
        };
        out.push(Para {
            style_name,
            level: para.level,
            list,
            align: para.align,
            space_before: para.space_before,
            space_after: para.space_after,
            line,
            line_fixed,
            keep_next: ex.keep_with_next,
            keep_lines: ex.keep_together,
            tabs: para.tabs.clone(),
            list_style: ex.list.filter(|_| para.label.is_some()),
            left,
            right: para.right_indent,
            first,
            brk: ex.brk,
            runs,
            objects,
            toc: ex.toc.is_some(),
            border: ex.border.filter(|_| ex.border_edges != 0).map(|(c, w)| (c, w, ex.border_edges, ex.border_offset.unwrap_or(1.0))),
            mark: (para.end <= para.start).then(|| para.runs.first().map(|r| r.style.clone())).flatten(),
        });
    }
    out
}

fn layout_of(p: &Pages) -> Layout {
    let mut fonts = kn_render::fonts::Fonts::new();
    let mut cache = flow::Cache::default();
    flow::layout(p, &mut fonts, &mut cache, &flow::Options::default())
}

/// Plain text: the body with tracked changes accepted, footnotes appended.
pub fn to_text(p: &Pages) -> String {
    let k = p.file();
    let Some(b) = p.body_storage().and_then(|s| body::build(k, s, None, BuildCtx { show_deletions: false, show_markup: false, ..Default::default() })) else { return String::new() };
    let mut out = String::new();
    let mut notes = Vec::new();
    let mut last = 0;
    for s in b.spans.iter().filter(|s| matches!(s.kind, SpanKind::Footnote { .. } | SpanKind::Object(_))) {
        out.push_str(&b.tb.text[last..s.start]);
        if let SpanKind::Footnote { storage, number } = s.kind {
            notes.push(format!("[{number}] {}", crate::text_plain(&p.doc.storage_text(storage)).trim()));
            out.push_str(&format!("[{number}]"));
        }
        last = s.end;
    }
    out.push_str(&b.tb.text[last..]);
    let mut out = out.replace('\u{2028}', "\n");
    if !notes.is_empty() {
        out.push_str("\n\n");
        out.push_str(&notes.join("\n"));
    }
    out
}

fn esc_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn hex(c: kn_model::Color) -> String {
    format!("{:02X}{:02X}{:02X}", (c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8)
}

fn family_of(ps: &str) -> String {
    let (fam, _, _) = kn_render::fonts::parse_ps_name(ps);
    if fam.is_empty() { "Helvetica Neue".into() } else { fam }
}

/// A picture's bytes (PNG / JPEG), its extension and frame size.
fn image_bytes(p: &Pages, id: ObjId) -> Option<(Vec<u8>, &'static str, f32, f32)> {
    let ctx = kn_model::text::TextContext { slide_number: 1 };
    let d = kn_model::scene::drawable(p.file(), id, ctx)?;
    let DrawableKind::Image(img) = &d.kind else { return None };
    let bytes = p.doc.data(img.data_id?)?.to_vec();
    let ext = if bytes.starts_with(&[0x89, b'P']) { "png" } else { "jpeg" };
    Some((bytes, ext, d.geom.w, d.geom.h))
}

fn table_cells(p: &Pages, id: ObjId) -> Option<kn_model::TableData> {
    let m = p.file().obj(id)?;
    kn_model::table::table(p.file(), m)
}

pub(crate) fn zip_files(path: &std::path::Path, files: Vec<(String, Vec<u8>, bool)>) -> Result<(), String> {
    let f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipWriter::new(f);
    for (name, bytes, store) in files {
        let opts = zip::write::SimpleFileOptions::default().compression_method(if store { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated });
        z.start_file(name, opts).map_err(|e| e.to_string())?;
        z.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn word_style_id(name: &str) -> String {
    let id: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
    if id.is_empty() { "Body".into() } else { id }
}

/// Microsoft Word (.docx).
pub fn to_docx(p: &Pages, path: &std::path::Path) -> Result<(), String> {
    zip_files(path, docx_files(p)?)
}

/// The parts of a Word file for the document: (name, bytes, stored uncompressed).
pub fn docx_files(p: &Pages) -> Result<Vec<(String, Vec<u8>, bool)>, String> {
    crate::docx_out::docx_files(p)
}

/// The first Word writer (kept for comparison; docx_out replaces it).
#[allow(dead_code)]
fn docx_files_v1(p: &Pages) -> Result<Vec<(String, Vec<u8>, bool)>, String> {
    let layout = layout_of(p);
    let paras = paragraphs(p, Some(&layout));
    let setup = p.setup();
    let tw = |pt: f32| (pt * 20.0).round() as i64;
    let mut rels: Vec<(String, String, String, Option<String>)> = Vec::new();
    let mut media: Vec<(String, Vec<u8>)> = Vec::new();
    let mut footnotes: Vec<String> = Vec::new();
    let mut comments: Vec<(usize, String, String)> = Vec::new();
    let mut styles_used: Vec<String> = Vec::new();
    let mut doc = String::new();
    let mut rev = 1;
    let (cols, gap) = p.columns();
    for para in &paras {
        if para.toc {
            doc.push_str("<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> TOC \\o \"1-3\" \\h </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>Table of Contents</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>");
            continue;
        }
        let sid = word_style_id(&para.style_name);
        if !styles_used.contains(&para.style_name) {
            styles_used.push(para.style_name.clone());
        }
        let mut ppr = format!("<w:pStyle w:val=\"{sid}\"/>");
        if let Some((_, numbered)) = &para.list {
            ppr.push_str(&format!("<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{}\"/></w:numPr>", para.level, if *numbered { 2 } else { 1 }));
        }
        ppr.push_str(&format!("<w:spacing w:before=\"{}\" w:after=\"{}\" w:line=\"{}\" w:lineRule=\"auto\"/>", tw(para.space_before), tw(para.space_after), (para.line * 240.0).round() as i64));
        if para.left != 0.0 || para.right != 0.0 || para.first != 0.0 {
            ppr.push_str(&format!("<w:ind w:left=\"{}\" w:right=\"{}\" w:firstLine=\"{}\"/>", tw(para.left), tw(para.right), tw(para.first)));
        }
        let jc = match para.align {
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "both",
            _ => "left",
        };
        ppr.push_str(&format!("<w:jc w:val=\"{jc}\"/>"));
        doc.push_str(&format!("<w:p><w:pPr>{ppr}</w:pPr>"));
        for r in &para.runs {
            let s = &r.style;
            let mut rpr = String::new();
            rpr.push_str(&format!("<w:rFonts w:ascii=\"{0}\" w:hAnsi=\"{0}\" w:cs=\"{0}\"/>", esc_xml(&family_of(&s.font_name))));
            if s.bold {
                rpr.push_str("<w:b/>");
            }
            if s.italic {
                rpr.push_str("<w:i/>");
            }
            if s.strike {
                rpr.push_str("<w:strike/>");
            }
            if s.underline {
                rpr.push_str("<w:u w:val=\"single\"/>");
            }
            if s.caps == kn_model::Caps::All {
                rpr.push_str("<w:caps/>");
            } else if s.caps == kn_model::Caps::Small {
                rpr.push_str("<w:smallCaps/>");
            }
            rpr.push_str(&format!("<w:color w:val=\"{}\"/><w:sz w:val=\"{}\"/>", hex(s.color), (s.size * 2.0).round() as i64));
            if let Some(bg) = s.background {
                rpr.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(bg)));
            }
            if s.script != 0 {
                rpr.push_str(if s.script > 0 { "<w:vertAlign w:val=\"superscript\"/>" } else { "<w:vertAlign w:val=\"subscript\"/>" });
            }
            if let Some((_, text, author)) = &r.comment {
                let n = comments.len();
                comments.push((n, text.clone(), author.clone()));
                doc.push_str(&format!("<w:commentRangeStart w:id=\"{n}\"/>"));
            }
            let mut run_xml = String::new();
            if let Some(note) = &r.footnote {
                footnotes.push(note.clone());
                run_xml.push_str(&format!("<w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:footnoteReference w:id=\"{}\"/></w:r>", footnotes.len()));
            } else {
                let mut body = String::new();
                for (i, part) in r.text.split('\u{2028}').enumerate() {
                    if i > 0 {
                        body.push_str("<w:br/>");
                    }
                    for (j, seg) in part.split('\t').enumerate() {
                        if j > 0 {
                            body.push_str("<w:tab/>");
                        }
                        if !seg.is_empty() {
                            let tag = if matches!(r.change, Some((true, _))) { "w:delText" } else { "w:t" };
                            body.push_str(&format!("<{tag} xml:space=\"preserve\">{}</{tag}>", esc_xml(seg)));
                        }
                    }
                }
                run_xml.push_str(&format!("<w:r><w:rPr>{rpr}</w:rPr>{body}</w:r>"));
            }
            if let Some(url) = &r.link {
                let id = format!("rIdL{}", rels.len() + 1);
                rels.push((id.clone(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink".into(), esc_xml(url), Some("External".into())));
                run_xml = format!("<w:hyperlink r:id=\"{id}\">{run_xml}</w:hyperlink>");
            }
            if let Some((deletion, author)) = &r.change {
                let tag = if *deletion { "w:del" } else { "w:ins" };
                run_xml = format!("<{tag} w:id=\"{rev}\" w:author=\"{}\">{run_xml}</{tag}>", esc_xml(author));
                rev += 1;
            }
            doc.push_str(&run_xml);
            if r.comment.is_some() {
                let n = comments.len() - 1;
                doc.push_str(&format!("<w:commentRangeEnd w:id=\"{n}\"/><w:r><w:commentReference w:id=\"{n}\"/></w:r>"));
            }
        }
        for o in &para.objects {
            if let Some((bytes, ext, w, h)) = image_bytes(p, *o) {
                let n = media.len() + 1;
                let name = format!("image{n}.{ext}");
                media.push((name.clone(), bytes));
                let id = format!("rIdI{n}");
                rels.push((id.clone(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image".into(), format!("media/{name}"), None));
                let (cx, cy) = ((w * 12700.0) as i64, (h * 12700.0) as i64);
                doc.push_str(&format!(
                    "<w:r><w:drawing><wp:inline><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{n}\" name=\"Picture {n}\"/><a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"
                ));
            }
        }
        match para.brk {
            Some(Break::Page) => doc.push_str("<w:r><w:br w:type=\"page\"/></w:r>"),
            Some(Break::Column) => doc.push_str("<w:r><w:br w:type=\"column\"/></w:r>"),
            _ => {}
        }
        doc.push_str("</w:p>");
        for o in &para.objects {
            if let Some(t) = table_cells(p, *o) {
                doc.push_str("<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\"/><w:left w:val=\"single\" w:sz=\"4\"/><w:bottom w:val=\"single\" w:sz=\"4\"/><w:right w:val=\"single\" w:sz=\"4\"/><w:insideH w:val=\"single\" w:sz=\"4\"/><w:insideV w:val=\"single\" w:sz=\"4\"/></w:tblBorders></w:tblPr><w:tblGrid>");
                for w in &t.col_widths {
                    doc.push_str(&format!("<w:gridCol w:w=\"{}\"/>", tw(*w)));
                }
                doc.push_str("</w:tblGrid>");
                for r in 0..t.rows {
                    doc.push_str("<w:tr>");
                    for c in 0..t.cols {
                        let v = t.cells.get((r * t.cols + c) as usize).cloned().unwrap_or_default();
                        let bold = r < t.header_rows || c < t.header_cols;
                        doc.push_str(&format!("<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/></w:tcPr><w:p><w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r></w:p></w:tc>", tw(t.col_widths.get(c as usize).copied().unwrap_or(72.0)), if bold { "<w:rPr><w:b/></w:rPr>" } else { "" }, esc_xml(&v)));
                    }
                    doc.push_str("</w:tr>");
                }
                doc.push_str("</w:tbl><w:p/>");
            }
        }
    }
    let hf_text = |footer: bool| -> String {
        let secs = p.sections();
        let Some(m) = secs.first().and_then(|s| s.master(1, 1)) else { return String::new() };
        p.header_footer_storages(m, footer).iter().map(|s| crate::text_plain(&p.doc.storage_text(*s))).collect::<Vec<_>>().join("\t")
    };
    let field_xml = |text: &str| -> String {
        format!("<w:p><w:pPr><w:pStyle w:val=\"HeaderFooter\"/><w:tabs><w:tab w:val=\"center\" w:pos=\"{}\"/><w:tab w:val=\"right\" w:pos=\"{}\"/></w:tabs></w:pPr><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>", tw(setup.text_width() / 2.0), tw(setup.text_width()), esc_xml(text).replace('\t', "</w:t></w:r><w:r><w:tab/></w:r><w:r><w:t xml:space=\"preserve\">"))
    };
    let (htext, ftext) = (hf_text(false), hf_text(true));
    let ns = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\"";
    let mut files: Vec<(String, Vec<u8>, bool)> = Vec::new();
    let mut sect = String::new();
    if setup.headers && !htext.trim().is_empty() {
        rels.push(("rIdH1".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header".into(), "header1.xml".into(), None));
        files.push(("word/header1.xml".into(), format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:hdr {ns}>{}</w:hdr>", field_xml(&htext)).into_bytes(), false));
        sect.push_str("<w:headerReference w:type=\"default\" r:id=\"rIdH1\"/>");
    }
    if setup.footers && !ftext.trim().is_empty() {
        rels.push(("rIdF1".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer".into(), "footer1.xml".into(), None));
        files.push(("word/footer1.xml".into(), format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:ftr {ns}>{}</w:ftr>", field_xml(&ftext)).into_bytes(), false));
        sect.push_str("<w:footerReference w:type=\"default\" r:id=\"rIdF1\"/>");
    }
    sect.push_str(&format!(
        "<w:pgSz w:w=\"{}\" w:h=\"{}\" w:orient=\"{}\"/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"{}\" w:footer=\"{}\"/>",
        tw(setup.width),
        tw(setup.height),
        if setup.landscape { "landscape" } else { "portrait" },
        tw(setup.top),
        tw(setup.right),
        tw(setup.bottom),
        tw(setup.left),
        tw(setup.header_margin),
        tw(setup.footer_margin)
    ));
    if cols > 1 {
        sect.push_str(&format!("<w:cols w:num=\"{cols}\" w:space=\"{}\"/>", tw(gap * setup.text_width())));
    }
    let document = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document {ns}><w:body>{doc}<w:sectPr>{sect}</w:sectPr></w:body></w:document>");
    let mut styles = String::from("<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Helvetica Neue\" w:hAnsi=\"Helvetica Neue\"/><w:sz w:val=\"22\"/></w:rPr></w:rPrDefault></w:docDefaults>");
    for name in styles_used.iter().chain(std::iter::once(&"Header & Footer".to_string())) {
        let id = word_style_id(name);
        let sid = crate::edit::para_style_named(&p.doc, name);
        let cs = kn_model::style::char_style(p.file(), sid, None);
        let outline = match name.as_str() {
            "Title" | "Heading" => Some(0),
            "Heading 2" => Some(1),
            "Heading 3" => Some(2),
            _ => None,
        };
        styles.push_str(&format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"{id}\"><w:name w:val=\"{}\"/>{}<w:rPr><w:rFonts w:ascii=\"{2}\" w:hAnsi=\"{2}\"/>{3}<w:sz w:val=\"{4}\"/><w:color w:val=\"{5}\"/></w:rPr></w:style>",
            esc_xml(if name.is_empty() { "Body" } else { name }),
            outline.map(|o| format!("<w:pPr><w:outlineLvl w:val=\"{o}\"/></w:pPr>")).unwrap_or_default(),
            esc_xml(&family_of(&cs.font_name)),
            if cs.bold { "<w:b/>" } else { "" },
            (cs.size * 2.0).round() as i64,
            hex(cs.color)
        ));
    }
    let styles_xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:styles {ns}>{styles}</w:styles>");
    let lvls = |numbered: bool| -> String {
        (0..9)
            .map(|l| {
                let (fmt, text) = if numbered { ("decimal", format!("%{}.", l + 1)) } else { ("bullet", "•".to_string()) };
                format!("<w:lvl w:ilvl=\"{l}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"{text}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>", 720 * (l + 1))
            })
            .collect()
    };
    let numbering = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:numbering {ns}><w:abstractNum w:abstractNumId=\"0\">{}</w:abstractNum><w:abstractNum w:abstractNumId=\"1\">{}</w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num><w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num></w:numbering>", lvls(false), lvls(true));
    rels.push(("rIdS".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles".into(), "styles.xml".into(), None));
    rels.push(("rIdN".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering".into(), "numbering.xml".into(), None));
    if !footnotes.is_empty() {
        rels.push(("rIdFN".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes".into(), "footnotes.xml".into(), None));
        let notes: String = footnotes
            .iter()
            .enumerate()
            .map(|(i, t)| format!("<w:footnote w:id=\"{}\"><w:p><w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> {}</w:t></w:r></w:p></w:footnote>", i + 1, esc_xml(t)))
            .collect();
        files.push(("word/footnotes.xml".into(), format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:footnotes {ns}><w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>{notes}</w:footnotes>").into_bytes(), false));
    }
    if !comments.is_empty() {
        rels.push(("rIdC".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments".into(), "comments.xml".into(), None));
        let cs: String = comments.iter().map(|(n, t, a)| format!("<w:comment w:id=\"{n}\" w:author=\"{}\"><w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p></w:comment>", esc_xml(a), esc_xml(t))).collect();
        files.push(("word/comments.xml".into(), format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:comments {ns}>{cs}</w:comments>").into_bytes(), false));
    }
    let rels_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{}</Relationships>",
        rels.iter().map(|(id, ty, target, mode)| format!("<Relationship Id=\"{id}\" Type=\"{ty}\" Target=\"{target}\"{}/>", mode.as_ref().map(|m| format!(" TargetMode=\"{m}\"")).unwrap_or_default())).collect::<String>()
    );
    let has = |n: &str| files.iter().any(|f| f.0 == n);
    let content_types = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>{}{}{}{}</Types>",
        if has("word/header1.xml") { "<Override PartName=\"/word/header1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>" } else { "" },
        if has("word/footer1.xml") { "<Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/>" } else { "" },
        if has("word/footnotes.xml") { "<Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/>" } else { "" },
        if has("word/comments.xml") { "<Override PartName=\"/word/comments.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml\"/>" } else { "" },
    );
    files.insert(0, ("[Content_Types].xml".into(), content_types.into_bytes(), false));
    files.insert(1, ("_rels/.rels".into(), b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>".to_vec(), false));
    files.push(("word/document.xml".into(), document.into_bytes(), false));
    files.push(("word/styles.xml".into(), styles_xml.into_bytes(), false));
    files.push(("word/numbering.xml".into(), numbering.into_bytes(), false));
    files.push(("word/_rels/document.xml.rels".into(), rels_xml.into_bytes(), false));
    for (name, bytes) in media {
        files.push((format!("word/media/{name}"), bytes, true));
    }
    Ok(files)
}

fn rtf_esc(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '{' => o.push_str("\\{"),
            '}' => o.push_str("\\}"),
            '\t' => o.push_str("\\tab "),
            '\u{2028}' => o.push_str("\\line "),
            c if (c as u32) < 128 => o.push(c),
            c => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    o.push_str(&format!("\\u{}?", *u as i16));
                }
            }
        }
    }
    o
}

/// Rich Text Format.
pub fn to_rtf(p: &Pages) -> String {
    let paras = paragraphs(p, None);
    let setup = p.setup();
    let mut fonts: Vec<String> = Vec::new();
    let mut colors: Vec<String> = Vec::new();
    let mut body = String::new();
    let tw = |pt: f32| (pt * 20.0).round() as i64;
    for para in &paras {
        body.push_str("\\pard");
        body.push_str(match para.align {
            Align::Center => "\\qc",
            Align::Right => "\\qr",
            Align::Justify => "\\qj",
            _ => "\\ql",
        });
        body.push_str(&format!("\\sb{}\\sa{}\\li{}\\ri{}\\fi{} ", tw(para.space_before), tw(para.space_after), tw(para.left), tw(para.right), tw(para.first)));
        if let Some((label, _)) = &para.list {
            body.push_str(&format!("{}\\tab ", rtf_esc(label)));
        }
        for r in &para.runs {
            let s = &r.style;
            let fam = family_of(&s.font_name);
            let fi = fonts.iter().position(|f| *f == fam).unwrap_or_else(|| {
                fonts.push(fam.clone());
                fonts.len() - 1
            });
            let col = format!("\\red{}\\green{}\\blue{};", (s.color.r * 255.0) as u8, (s.color.g * 255.0) as u8, (s.color.b * 255.0) as u8);
            let ci = colors.iter().position(|c| *c == col).unwrap_or_else(|| {
                colors.push(col.clone());
                colors.len() - 1
            }) + 1;
            body.push_str(&format!("{{\\f{fi}\\fs{}\\cf{ci}", (s.size * 2.0).round() as i64));
            if s.bold {
                body.push_str("\\b");
            }
            if s.italic {
                body.push_str("\\i");
            }
            if s.underline {
                body.push_str("\\ul");
            }
            if s.strike {
                body.push_str("\\strike");
            }
            if s.script > 0 {
                body.push_str("\\super");
            } else if s.script < 0 {
                body.push_str("\\sub");
            }
            body.push(' ');
            if let Some(note) = &r.footnote {
                body.push_str(&format!("\\chftn{{\\footnote\\pard\\plain\\chftn {}}}", rtf_esc(note)));
            } else if let Some(url) = &r.link {
                body.push_str(&format!("{{\\field{{\\*\\fldinst HYPERLINK \"{}\"}}{{\\fldrslt {}}}}}", rtf_esc(url), rtf_esc(&r.text)));
            } else if !matches!(r.change, Some((true, _))) {
                body.push_str(&rtf_esc(&r.text));
            }
            body.push('}');
        }
        body.push_str(match para.brk {
            Some(Break::Page) | Some(Break::Section) => "\\par\\page\n",
            Some(Break::Column) => "\\par\\column\n",
            None => "\\par\n",
        });
    }
    let ftbl: String = fonts.iter().enumerate().map(|(i, f)| format!("{{\\f{i}\\fnil {};}}", rtf_esc(f))).collect();
    let ctbl: String = colors.concat();
    format!(
        "{{\\rtf1\\ansi\\ansicpg1252\\deff0{{\\fonttbl{ftbl}}}{{\\colortbl;{ctbl}}}\\paperw{}\\paperh{}\\margl{}\\margr{}\\margt{}\\margb{}\n{body}}}",
        tw(setup.width),
        tw(setup.height),
        tw(setup.left),
        tw(setup.right),
        tw(setup.top),
        tw(setup.bottom)
    )
}

pub fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::with_capacity(b.len() * 4 / 3 + 4);
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        o.push(T[(n >> 18) as usize & 63] as char);
        o.push(T[(n >> 12) as usize & 63] as char);
        o.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        o.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    o
}

/// The body as HTML (`images`: Some(list) collects pictures as files instead of data URLs).
pub fn to_html_body(p: &Pages, mut images: Option<&mut Vec<(String, Vec<u8>)>>) -> String {
    let layout = layout_of(p);
    let paras = paragraphs(p, Some(&layout));
    let mut out = String::new();
    let mut open_list: Option<bool> = None;
    let mut notes = Vec::new();
    for para in &paras {
        let tag = match para.style_name.as_str() {
            "Title" => "h1",
            "Heading" => "h2",
            "Heading 2" => "h3",
            "Heading 3" => "h4",
            _ => "p",
        };
        match (&para.list, open_list) {
            (Some((_, n)), Some(o)) if *n == o => {}
            (Some((_, n)), _) => {
                if let Some(o) = open_list {
                    out.push_str(if o { "</ol>\n" } else { "</ul>\n" });
                }
                out.push_str(if *n { "<ol>\n" } else { "<ul>\n" });
                open_list = Some(*n);
            }
            (None, Some(o)) => {
                out.push_str(if o { "</ol>\n" } else { "</ul>\n" });
                open_list = None;
            }
            _ => {}
        }
        let align = match para.align {
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
            _ => "left",
        };
        let mut inner = String::new();
        for r in &para.runs {
            if matches!(r.change, Some((true, _))) {
                continue;
            }
            let s = &r.style;
            if let Some(n) = &r.footnote {
                notes.push(n.clone());
                inner.push_str(&format!("<sup><a href=\"#fn{0}\" id=\"ref{0}\">{0}</a></sup>", notes.len()));
                continue;
            }
            let mut t = esc_xml(&r.text).replace('\u{2028}', "<br/>").replace('\t', "&#9;");
            let mut css = format!("font-family:'{}';font-size:{}pt;color:#{}", family_of(&s.font_name), s.size, hex(s.color));
            if let Some(bg) = s.background {
                css.push_str(&format!(";background:#{}", hex(bg)));
            }
            if s.bold {
                t = format!("<b>{t}</b>");
            }
            if s.italic {
                t = format!("<i>{t}</i>");
            }
            if s.underline {
                t = format!("<u>{t}</u>");
            }
            if s.strike {
                t = format!("<s>{t}</s>");
            }
            if s.script > 0 {
                t = format!("<sup>{t}</sup>");
            } else if s.script < 0 {
                t = format!("<sub>{t}</sub>");
            }
            t = format!("<span style=\"{css}\">{t}</span>");
            if let Some(u) = &r.link {
                t = format!("<a href=\"{}\">{t}</a>", esc_xml(u));
            }
            inner.push_str(&t);
        }
        for o in &para.objects {
            if let Some((bytes, ext, w, h)) = image_bytes(p, *o) {
                let src = match images.as_deref_mut() {
                    Some(list) => {
                        let name = format!("images/image{}.{ext}", list.len() + 1);
                        list.push((name.clone(), bytes));
                        name
                    }
                    None => format!("data:image/{ext};base64,{}", base64(&bytes)),
                };
                inner.push_str(&format!("<img src=\"{src}\" width=\"{}\" height=\"{}\" alt=\"\"/>", w.round(), h.round()));
            }
            if let Some(t) = table_cells(p, *o) {
                let mut tb = String::from("<table border=\"1\" style=\"border-collapse:collapse\">");
                for r in 0..t.rows {
                    tb.push_str("<tr>");
                    for c in 0..t.cols {
                        let v = t.cells.get((r * t.cols + c) as usize).cloned().unwrap_or_default();
                        let cell = if r < t.header_rows { "th" } else { "td" };
                        tb.push_str(&format!("<{cell}>{}</{cell}>", esc_xml(&v)));
                    }
                    tb.push_str("</tr>");
                }
                tb.push_str("</table>");
                inner.push_str(&tb);
            }
        }
        if para.toc {
            let heads: Vec<String> = paras.iter().filter(|q| q.style_name.starts_with("Heading")).map(|q| q.runs.iter().map(|r| r.text.as_str()).collect::<String>()).collect();
            inner = format!("<nav><ol>{}</ol></nav>", heads.iter().map(|h| format!("<li>{}</li>", esc_xml(h))).collect::<String>());
        }
        if inner.is_empty() {
            inner.push_str("<br/>");
        }
        let style = format!("text-align:{align};margin:{}pt {}pt {}pt {}pt;text-indent:{}pt", para.space_before, para.right, para.space_after, para.left, para.first);
        if para.list.is_some() {
            out.push_str(&format!("<li style=\"{style}\">{inner}</li>\n"));
        } else {
            out.push_str(&format!("<{tag} style=\"{style}\">{inner}</{tag}>\n"));
        }
        if matches!(para.brk, Some(Break::Page) | Some(Break::Section)) {
            out.push_str("<div style=\"page-break-after:always\"></div>\n");
        }
    }
    if let Some(o) = open_list {
        out.push_str(if o { "</ol>\n" } else { "</ul>\n" });
    }
    if !notes.is_empty() {
        out.push_str("<hr/><ol class=\"footnotes\">\n");
        for (i, n) in notes.iter().enumerate() {
            out.push_str(&format!("<li id=\"fn{0}\">{1} <a href=\"#ref{0}\">↩</a></li>\n", i + 1, esc_xml(n)));
        }
        out.push_str("</ol>\n");
    }
    out
}

pub fn to_html(p: &Pages, title: &str) -> String {
    format!("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"/><title>{}</title><style>body{{max-width:7in;margin:1in auto;}}</style></head><body>\n{}</body></html>\n", esc_xml(title), to_html_body(p, None))
}

/// EPUB 3 (reflowable).
pub fn to_epub(p: &Pages, path: &std::path::Path, title: &str) -> Result<(), String> {
    let mut images = Vec::new();
    let body = to_html_body(p, Some(&mut images));
    let xhtml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>{}</title><link rel=\"stylesheet\" href=\"css/book.css\"/></head><body>\n{}</body></html>\n", esc_xml(title), body);
    let nav = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>{0}</title></head><body><nav epub:type=\"toc\"><ol><li><a href=\"chapter-1.xhtml\">{0}</a></li></ol></nav></body></html>", esc_xml(title));
    let id = format!("urn:uuid:letters-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    let manifest_imgs: String = images.iter().enumerate().map(|(i, (n, _))| format!("<item id=\"img{i}\" href=\"{n}\" media-type=\"image/{}\"/>", if n.ends_with(".png") { "png" } else { "jpeg" })).collect();
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"bookid\">{id}</dc:identifier><dc:title>{}</dc:title><dc:language>en</dc:language><meta property=\"dcterms:modified\">2026-01-01T00:00:00Z</meta></metadata><manifest><item id=\"nav\" href=\"toc.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/><item id=\"ch1\" href=\"chapter-1.xhtml\" media-type=\"application/xhtml+xml\"/><item id=\"css\" href=\"css/book.css\" media-type=\"text/css\"/>{manifest_imgs}</manifest><spine><itemref idref=\"ch1\"/></spine></package>",
        esc_xml(title)
    );
    let mut files: Vec<(String, Vec<u8>, bool)> = vec![
        ("mimetype".into(), b"application/epub+zip".to_vec(), true),
        ("META-INF/container.xml".into(), b"<?xml version=\"1.0\"?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OPS/epb.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>".to_vec(), false),
        ("OPS/epb.opf".into(), opf.into_bytes(), false),
        ("OPS/toc.xhtml".into(), nav.into_bytes(), false),
        ("OPS/chapter-1.xhtml".into(), xhtml.into_bytes(), false),
        ("OPS/css/book.css".into(), b"body{font-family:'Helvetica Neue',sans-serif;line-height:1.3;} img{max-width:100%;height:auto;}".to_vec(), false),
    ];
    for (n, b) in images {
        files.push((format!("OPS/{n}"), b, true));
    }
    zip_files(path, files)
}

/// Every page rendered at `scale` pixels per point.
pub fn render_pages(p: &Pages, scale: f32) -> Vec<tiny_skia::Pixmap> {
    let mut r = kn_render::Renderer::new();
    let mut cache = flow::Cache::default();
    let l = flow::layout(p, &mut r.fonts, &mut cache, &flow::Options::default());
    (0..l.pages.len()).filter_map(|i| crate::render::render_page(p, &l, i, &mut r, scale, Default::default())).collect()
}

/// The first page rendered at `scale` pixels per point (the start screen's thumbnails).
pub fn first_page(p: &Pages, scale: f32) -> Option<tiny_skia::Pixmap> {
    let mut r = kn_render::Renderer::new();
    let mut cache = flow::Cache::default();
    let l = flow::layout(p, &mut r.fonts, &mut cache, &flow::Options::default());
    crate::render::render_page(p, &l, 0, &mut r, scale, Default::default())
}

/// PDF: the pages as printed (pictures of the pages at 200 dpi, as JPEG).
pub fn to_pdf(p: &Pages, path: &std::path::Path) -> Result<usize, String> {
    let setup = p.setup();
    let pages = render_pages(p, 200.0 / 72.0);
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let n = pages.len();
    let page_obj = |i: usize| 3 + i * 3;
    let obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend(body);
        out.extend(b"\nendobj\n");
    };
    obj(&mut out, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", page_obj(i))).collect();
    obj(&mut out, &mut offsets, format!("<< /Type /Pages /Count {n} /Kids [{}] >>", kids.join(" ")).as_bytes());
    for (i, pm) in pages.iter().enumerate() {
        let (content_id, image_id) = (page_obj(i) + 1, page_obj(i) + 2);
        // (each page at its own size: a Word section may be landscape)
        let (pw, ph) = if p.section_pages.is_empty() { (setup.width, setup.height) } else { (pm.width() as f32 * 72.0 / 200.0, pm.height() as f32 * 72.0 / 200.0) };
        let rgb: Vec<u8> = pm
            .pixels()
            .iter()
            .flat_map(|c| {
                let c = c.demultiply();
                [c.red(), c.green(), c.blue()]
            })
            .collect();
        let mut jpg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 92).encode(&rgb, pm.width(), pm.height(), image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
        obj(&mut out, &mut offsets, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw} {ph}] /Resources << /XObject << /Im0 {image_id} 0 R >> >> /Contents {content_id} 0 R >>").as_bytes());
        let content = format!("q {pw} 0 0 {ph} 0 0 cm /Im0 Do Q");
        obj(&mut out, &mut offsets, format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()).as_bytes());
        let mut body = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", pm.width(), pm.height(), jpg.len()).into_bytes();
        body.extend(&jpg);
        body.extend(b"\nendstream");
        obj(&mut out, &mut offsets, &body);
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
    std::fs::write(path, out).map_err(|e| e.to_string())?;
    Ok(n)
}

/// Images: one PNG per page in `dir` ("<stem>-N.png").
pub fn to_images(p: &Pages, dir: &std::path::Path, stem: &str) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let pages = render_pages(p, 2.0);
    for (i, pm) in pages.iter().enumerate() {
        pm.save_png(dir.join(format!("{stem}-{}.png", i + 1))).map_err(|e| e.to_string())?;
    }
    Ok(pages.len())
}

/// Write `p` to `path` in the format its extension names.
pub fn export(p: &Pages, path: &std::path::Path) -> Result<(), String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let title = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
    match ext.as_str() {
        "pdf" => to_pdf(p, path).map(|_| ()),
        "docx" => to_docx(p, path),
        "rtf" => std::fs::write(path, to_rtf(p)).map_err(|e| e.to_string()),
        "txt" => std::fs::write(path, to_text(p)).map_err(|e| e.to_string()),
        "html" | "htm" => std::fs::write(path, to_html(p, &title)).map_err(|e| e.to_string()),
        "epub" => to_epub(p, path, &title),
        "png" => {
            let dir = path.parent().unwrap_or(std::path::Path::new("."));
            to_images(p, dir, &title).map(|_| ())
        }
        _ => Err(format!("can't export to .{ext}")),
    }
}
