//! Word (.docx) out, aiming at what Word, OnlyOffice and LibreOffice all show the same way:
//! page size, margins and columns; the page colour (an image background as a full-page picture
//! behind the text, from the header, so it repeats on every page); headers and footers with
//! their formatting and live page number / page count fields (Pages' left / centre / right
//! fields on centre and right tab stops), a different first page and each section's own;
//! section breaks; paragraph styles as Word's own (Body = Normal, Heading = Heading 1…) with
//! their spacing; lists; run formatting, links, footnotes, comments, tracked changes; inline
//! pictures; floating pictures, text boxes and shapes where they sit on the page with their
//! wrap (shapes keep their exact outline, fill and border); charts as pictures; tables with
//! every cell's fill, text, alignment, padding and borders, merged cells and repeating header
//! rows. Page layout documents become one Word page per page with its objects anchored on it.

use std::collections::HashMap;

use kn_format::ObjId;
use kn_model::{Align, CharStyle, Drawable, DrawableKind, Fill, LineSpacing, PathEl};

use crate::body::Break;
use crate::export::{Para, RunOut};
use crate::Pages;

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\" xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" xmlns:wp14=\"http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" mc:Ignorable=\"w14 wp14\"";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const XML_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A colour as Word writes it: opaque, so a translucent one as it looks on white paper.
fn hex(c: kn_model::Color) -> String {
    let a = c.a.clamp(0.0, 1.0);
    let ch = |v: f32| ((v * a + (1.0 - a)) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("{:02X}{:02X}{:02X}", ch(c.r), ch(c.g), ch(c.b))
}

fn tw(pt: f32) -> i64 {
    (pt * 20.0).round() as i64
}

fn emu(pt: f32) -> i64 {
    (pt as f64 * 12700.0).round() as i64
}

fn family_of(ps: &str) -> String {
    let (fam, _, _) = kn_render::fonts::parse_ps_name(ps);
    if fam.is_empty() { "Helvetica Neue".into() } else { fam }
}

/// A Pages paragraph style as Word's: (style id, Word name, outline level).
fn word_style(name: &str) -> (String, String, Option<u32>) {
    match name {
        "" | "Body" => ("Normal".into(), "Normal".into(), None),
        "Title" => ("Title".into(), "Title".into(), Some(0)),
        "Subtitle" => ("Subtitle".into(), "Subtitle".into(), None),
        "Heading" | "Heading 1" => ("Heading1".into(), "heading 1".into(), Some(0)),
        "Heading 2" => ("Heading2".into(), "heading 2".into(), Some(1)),
        "Heading 3" => ("Heading3".into(), "heading 3".into(), Some(2)),
        "Heading 4" => ("Heading4".into(), "heading 4".into(), Some(3)),
        "Caption" => ("Caption".into(), "caption".into(), None),
        "Quote" => ("Quote".into(), "Quote".into(), None),
        _ => {
            let id: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
            (if id.is_empty() { "Normal".into() } else { id }, name.into(), None)
        }
    }
}

struct Out<'a> {
    p: &'a Pages,
    rels: Vec<(String, String, String, bool)>,
    media: Vec<(String, Vec<u8>)>,
    /// Chart parts (word/charts/chartN.xml).
    charts: Vec<String>,
    footnotes: Vec<String>,
    comments: Vec<(String, String)>,
    rev: u32,
    pic_id: u32,
    renderer: kn_render::Renderer,
    styles_used: Vec<String>,
}

impl Out<'_> {
    /// A relationship of document.xml (headers / footers keep their own lists).
    fn rel(rels: &mut Vec<(String, String, String, bool)>, ty: &str, target: &str, external: bool) -> String {
        if let Some(r) = rels.iter().find(|r| r.1.ends_with(ty) && r.2 == target) {
            return r.0.clone();
        }
        let id = format!("rId{}", rels.len() + 10);
        rels.push((id.clone(), format!("{REL}/{ty}"), target.to_string(), external));
        id
    }

    fn add_media(&mut self, bytes: Vec<u8>) -> String {
        let ext = if bytes.starts_with(&[0x89, b'P']) { "png" } else if bytes.starts_with(b"GIF") { "gif" } else { "jpeg" };
        let name = format!("image{}.{ext}", self.media.len() + 1);
        self.media.push((name.clone(), bytes));
        format!("media/{name}")
    }

    fn next_pic(&mut self) -> u32 {
        self.pic_id += 1;
        self.pic_id
    }
}

fn rpr(s: &CharStyle) -> String {
    let mut x = format!("<w:rFonts w:ascii=\"{0}\" w:hAnsi=\"{0}\" w:cs=\"{0}\" w:eastAsia=\"{0}\"/>", esc(&family_of(&s.font_name)));
    if s.bold {
        x.push_str("<w:b/><w:bCs/>");
    }
    if s.italic {
        x.push_str("<w:i/><w:iCs/>");
    }
    if s.caps == kn_model::Caps::All {
        x.push_str("<w:caps/>");
    } else if s.caps == kn_model::Caps::Small {
        x.push_str("<w:smallCaps/>");
    }
    if s.strike {
        x.push_str("<w:strike/>");
    }
    x.push_str(&format!("<w:color w:val=\"{}\"/>", hex(s.color)));
    x.push_str(&format!("<w:sz w:val=\"{0}\"/><w:szCs w:val=\"{0}\"/>", (s.size * 2.0).round().max(2.0) as i64));
    if s.underline {
        x.push_str("<w:u w:val=\"single\"/>");
    }
    if let Some(bg) = s.background {
        x.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(bg)));
    }
    if s.script != 0 {
        x.push_str(if s.script > 0 { "<w:vertAlign w:val=\"superscript\"/>" } else { "<w:vertAlign w:val=\"subscript\"/>" });
    }
    x
}

/// Text as Word runs' content: line breaks, tabs, text.
fn text_xml(t: &str, deleted: bool) -> String {
    let mut body = String::new();
    for (i, part) in t.split('\u{2028}').enumerate() {
        if i > 0 {
            body.push_str("<w:br/>");
        }
        for (j, seg) in part.split('\t').enumerate() {
            if j > 0 {
                body.push_str("<w:tab/>");
            }
            if !seg.is_empty() {
                let tag = if deleted { "w:delText" } else { "w:t" };
                body.push_str(&format!("<{tag} xml:space=\"preserve\">{}</{tag}>", esc(seg)));
            }
        }
    }
    body
}

/// One run (with its link, change, comment, footnote or field).
fn run_xml(o: &mut Out, rels: &mut Vec<(String, String, String, bool)>, r: &RunOut, doc_part: bool) -> String {
    let props = rpr(&r.style);
    let mut out = String::new();
    if doc_part {
        if let Some((_, text, author)) = &r.comment {
            let n = o.comments.len();
            o.comments.push((text.clone(), author.clone()));
            out.push_str(&format!("<w:commentRangeStart w:id=\"{n}\"/>"));
        }
    }
    let mut x = if let Some(count) = r.field {
        let instr = if count { "NUMPAGES" } else { "PAGE" };
        format!("<w:fldSimple w:instr=\" {instr} \"><w:r><w:rPr>{props}</w:rPr><w:t>{}</w:t></w:r></w:fldSimple>", esc(r.text.trim()).replace(['\u{fffc}'], "1"))
    } else if let (true, Some(note)) = (doc_part, &r.footnote) {
        o.footnotes.push(note.clone());
        format!("<w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:footnoteReference w:id=\"{}\"/></w:r>", o.footnotes.len())
    } else if r.text.is_empty() {
        String::new()
    } else {
        format!("<w:r><w:rPr>{props}</w:rPr>{}</w:r>", text_xml(&r.text, matches!(r.change, Some((true, _)))))
    };
    if let (Some(url), false) = (&r.link, x.is_empty()) {
        let id = Out::rel(rels, "hyperlink", &esc(url), true);
        x = format!("<w:hyperlink r:id=\"{id}\">{x}</w:hyperlink>");
    }
    if let (true, Some((deletion, author))) = (doc_part, &r.change) {
        let tag = if *deletion { "w:del" } else { "w:ins" };
        x = format!("<{tag} w:id=\"{}\" w:author=\"{}\">{x}</{tag}>", o.rev, esc(author));
        o.rev += 1;
    }
    out.push_str(&x);
    if doc_part && r.comment.is_some() {
        let n = o.comments.len() - 1;
        out.push_str(&format!("<w:commentRangeEnd w:id=\"{n}\"/><w:r><w:commentReference w:id=\"{n}\"/></w:r>"));
    }
    out
}

thread_local! {
    /// While writing a document: each list style's Word list (numId).
    static LIST_IDS: std::cell::RefCell<Vec<ObjId>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The Word list (numId) of a list style, numbered from 3 (1 and 2 are the plain bullet and
/// numbered lists).
fn list_num_id(id: ObjId) -> u32 {
    LIST_IDS.with(|l| {
        let mut l = l.borrow_mut();
        let i = l.iter().position(|x| *x == id).unwrap_or_else(|| {
            l.push(id);
            l.len() - 1
        });
        3 + i as u32
    })
}

fn ppr(para: &Para, style_id: &str) -> String {
    let mut x = format!("<w:pStyle w:val=\"{style_id}\"/>");
    if para.keep_next {
        x.push_str("<w:keepNext/>");
    }
    if para.keep_lines {
        x.push_str("<w:keepLines/>");
    }
    if let Some((_, numbered)) = &para.list {
        let num = para.list_style.map(list_num_id).unwrap_or(if *numbered { 2 } else { 1 });
        x.push_str(&format!("<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{num}\"/></w:numPr>", para.level.min(8)));
    }
    // its tab stops (positions from the text's left edge, as Word measures them)
    if !para.tabs.is_empty() {
        x.push_str("<w:tabs>");
        for t in &para.tabs {
            let val = match t.align {
                kn_model::TabAlign::Center => "center",
                kn_model::TabAlign::Right => "right",
                kn_model::TabAlign::Decimal => "decimal",
                kn_model::TabAlign::Left => "left",
            };
            let leader = match t.leader {
                Some('.') => " w:leader=\"dot\"",
                Some('-') => " w:leader=\"hyphen\"",
                Some('_') => " w:leader=\"underscore\"",
                Some('·') => " w:leader=\"middleDot\"",
                _ => "",
            };
            x.push_str(&format!("<w:tab w:val=\"{val}\" w:pos=\"{}\"{leader}/>", tw(t.pos)));
        }
        x.push_str("</w:tabs>");
    }
    let line = match para.line_fixed {
        Some((true, v)) => format!("w:line=\"{}\" w:lineRule=\"exact\"", tw(v)),
        Some((false, v)) => format!("w:line=\"{}\" w:lineRule=\"atLeast\"", tw(v)),
        None => format!("w:line=\"{}\" w:lineRule=\"auto\"", (para.line * 240.0).round() as i64),
    };
    x.push_str(&format!("<w:spacing w:before=\"{}\" w:after=\"{}\" {line}/>", tw(para.space_before), tw(para.space_after)));
    // (a list's own indents too: they override its numbering level's)
    if para.list.is_some() || para.left != 0.0 || para.right != 0.0 || para.first != 0.0 {
        let first = if para.first < 0.0 { format!("w:hanging=\"{}\"", tw(-para.first)) } else { format!("w:firstLine=\"{}\"", tw(para.first)) };
        x.push_str(&format!("<w:ind w:left=\"{}\" w:right=\"{}\" {first}/>", tw(para.left), tw(para.right)));
    }
    // paragraph border: the edges Pages has (1 top, 2 bottom, 15 all round)
    if let Some((c, w, edges, space)) = para.border {
        let line = |tag: &str| format!("<w:{tag} w:val=\"single\" w:sz=\"{}\" w:space=\"{}\" w:color=\"{}\"/>", (w * 8.0).round().max(2.0) as i64, space.round() as i64, hex(c));
        let mut b = String::new();
        if edges & 1 != 0 || edges == 15 {
            b.push_str(&line("top"));
        }
        if edges == 15 {
            b.push_str(&line("left"));
        }
        if edges & 2 != 0 || edges == 15 {
            b.push_str(&line("bottom"));
        }
        if edges == 15 {
            b.push_str(&line("right"));
        }
        x.push_str(&format!("<w:pBdr>{b}</w:pBdr>"));
    }
    let jc = match para.align {
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "both",
        _ => "left",
    };
    x.push_str(&format!("<w:jc w:val=\"{jc}\"/>"));
    // an empty paragraph's mark: its size and font (the empty line's height)
    if let Some(m) = &para.mark {
        x.push_str(&format!("<w:rPr>{}</w:rPr>", rpr(m)));
    }
    x
}

// ---------------------------------------------------------------------- drawings

fn drawable(p: &Pages, id: ObjId) -> Option<Drawable> {
    kn_model::scene::drawable(p.file(), id, kn_model::text::TextContext { slide_number: 1 })
}

/// A picture of any drawable (charts, movies' posters, anything Word can't hold itself).
fn picture_of(o: &mut Out, d: &Drawable) -> Option<Vec<u8>> {
    let k = 2.0;
    let mut pm = kn_render::tiny_skia::Pixmap::new((d.geom.w * k).ceil().max(1.0) as u32, (d.geom.h * k).ceil().max(1.0) as u32)?;
    let mut d2 = d.clone();
    d2.geom.x = 0.0;
    d2.geom.y = 0.0;
    d2.geom.angle = 0.0;
    o.renderer.draw_drawable(&o.p.doc, &d2, &mut pm, kn_render::tiny_skia::Transform::from_scale(k, k));
    pm.encode_png().ok()
}

/// The `<pic:pic>` of picture bytes.
fn pic_graphic(o: &mut Out, rels: &mut Vec<(String, String, String, bool)>, bytes: Vec<u8>, cx: i64, cy: i64, n: u32) -> String {
    let target = o.add_media(bytes);
    let id = Out::rel(rels, "image", &target, false);
    format!("<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"Picture {n}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>")
}

fn fill_xml(f: &Fill) -> String {
    match f {
        Fill::Color(c) if c.a > 0.001 => format!("<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>", hex(*c)),
        Fill::Gradient { stops, .. } => match stops.first() {
            Some(s) => format!("<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>", hex(s.1)),
            None => "<a:noFill/>".into(),
        },
        _ => "<a:noFill/>".into(),
    }
}

/// A shape's outline as DrawingML custom geometry (its exact path).
fn cust_geom(path: &kn_model::Path, w: f32, h: f32) -> String {
    let pt = |q: &kn_model::Point| format!("<a:pt x=\"{}\" y=\"{}\"/>", emu(q.x), emu(q.y));
    let mut x = String::new();
    for e in &path.els {
        match e {
            PathEl::Move(q) => x.push_str(&format!("<a:moveTo>{}</a:moveTo>", pt(q))),
            PathEl::Line(q) => x.push_str(&format!("<a:lnTo>{}</a:lnTo>", pt(q))),
            PathEl::Quad(c, q) => x.push_str(&format!("<a:quadBezTo>{}{}</a:quadBezTo>", pt(c), pt(q))),
            PathEl::Cubic(c1, c2, q) => x.push_str(&format!("<a:cubicBezTo>{}{}{}</a:cubicBezTo>", pt(c1), pt(c2), pt(q))),
            PathEl::Close => x.push_str("<a:close/>"),
        }
    }
    format!("<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"0\" t=\"0\" r=\"r\" b=\"b\"/><a:pathLst><a:path w=\"{}\" h=\"{}\">{x}</a:path></a:pathLst></a:custGeom>", emu(w).max(1), emu(h).max(1))
}

/// A shape or text box as a Word shape (wps:wsp).
fn shape_graphic(o: &mut Out, rels: &mut Vec<(String, String, String, bool)>, d: &Drawable, n: u32, storage: Option<ObjId>) -> Option<String> {
    let DrawableKind::Shape(s) = &d.kind else { return None };
    let (cx, cy) = (emu(d.geom.w).max(1), emu(d.geom.h).max(1));
    let rot = if d.geom.angle.abs() > 0.01 { format!(" rot=\"{}\"", (d.geom.angle * 60000.0).round() as i64) } else { String::new() };
    let flip = format!("{}{}", if d.geom.hflip { " flipH=\"1\"" } else { "" }, if d.geom.vflip { " flipV=\"1\"" } else { "" });
    let geom = if s.is_text_box && s.path.els.len() <= 6 { "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>".to_string() } else { cust_geom(&s.path, d.geom.w, d.geom.h) };
    let fill = if s.is_line { "<a:noFill/>".to_string() } else { fill_xml(&s.fill) };
    let line = match &s.stroke {
        Some(st) => format!("<a:ln w=\"{}\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>{}{}</a:ln>", emu(st.width), hex(st.color), if s.head.is_some() { "<a:tailEnd type=\"triangle\"/>" } else { "" }, if s.tail.is_some() { "<a:headEnd type=\"triangle\"/>" } else { "" }),
        None => "<a:ln><a:noFill/></a:ln>".into(),
    };
    let text = storage.filter(|st| !o.p.doc.storage_text(*st).trim().is_empty()).map(|st| {
        let paras = crate::export::paragraphs_of(o.p, st, None);
        let mut x = String::new();
        for para in &paras {
            let (sid, _, _) = word_style(&para.style_name);
            x.push_str(&format!("<w:p><w:pPr>{}</w:pPr>", ppr(para, &sid)));
            for r in &para.runs {
                x.push_str(&run_xml(o, rels, r, false));
            }
            x.push_str("</w:p>");
        }
        format!("<wps:txbx><w:txbxContent>{x}</w:txbxContent></wps:txbx>")
    });
    let body = if text.is_some() { "<wps:bodyPr rot=\"0\" vert=\"horz\" wrap=\"square\" lIns=\"45720\" tIns=\"45720\" rIns=\"45720\" bIns=\"45720\" anchor=\"t\"><a:noAutofit/></wps:bodyPr>" } else { "<wps:bodyPr/>" };
    Some(format!(
        "<a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\"><wps:wsp><wps:cNvPr id=\"{n}\" name=\"Shape {n}\"/><wps:cNvSpPr{}/><wps:spPr><a:xfrm{rot}{flip}><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>{geom}{fill}{line}</wps:spPr>{}{body}</wps:wsp></a:graphicData></a:graphic>",
        if text.is_some() { " txBox=\"1\"" } else { "" },
        text.unwrap_or_default()
    ))
}

/// The graphic of any drawable: a picture, a shape / text box, or a picture of it.
fn graphic(o: &mut Out, rels: &mut Vec<(String, String, String, bool)>, id: ObjId, d: &Drawable, n: u32) -> Option<(String, bool)> {
    let (cx, cy) = (emu(d.geom.w).max(1), emu(d.geom.h).max(1));
    match &d.kind {
        DrawableKind::Image(img) if img.mask.is_none() && img.web_video.is_none() && img.gallery.len() <= 1 && img.data_id.and_then(|i| o.p.doc.data(i)).is_some() => {
            let bytes = o.p.doc.data(img.data_id?)?.to_vec();
            Some((pic_graphic(o, rels, bytes, cx, cy, n), true))
        }
        // an image fill (a picture background): drawn as Pages fills it
        DrawableKind::Shape(s) if matches!(s.fill, Fill::Image { .. }) => {
            let bytes = picture_of(o, d)?;
            Some((pic_graphic(o, rels, bytes, cx, cy, n), true))
        }
        DrawableKind::Shape(_) => {
            let storage = o.p.doc.storage_of(id);
            shape_graphic(o, rels, d, n, storage).map(|g| (g, false))
        }
        DrawableKind::Table(_) => None,
        // a chart: Word's own (its type, data, colours, title and legend), not a picture
        DrawableKind::Chart(c) if !c.series.is_empty() => {
            o.charts.push(chart_xml(c));
            let target = format!("charts/chart{}.xml", o.charts.len());
            let rid = Out::rel(rels, "chart", &target, false);
            Some((format!("<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" r:id=\"{rid}\"/></a:graphicData></a:graphic>"), true))
        }
        _ => {
            let bytes = picture_of(o, d)?;
            Some((pic_graphic(o, rels, bytes, cx, cy, n), true))
        }
    }
}

/// A chart as Word's chart part: its values written in it (literal data, which Word and
/// OnlyOffice draw), each series' colour, the title and legend.
fn chart_xml(c: &kn_model::ChartData) -> String {
    let t = c.chart_type.as_str();
    let stacked = t.starts_with("stacked");
    let is_bar = t.starts_with("bar") || t.starts_with("stackedBar") || t.starts_with("multiDataBar");
    let (kind, extra, axes) = if t.contains("olumn") || is_bar {
        let bar = is_bar;
        ("barChart", format!("<c:barDir val=\"{}\"/><c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>", if bar { "bar" } else { "col" }, if stacked { "stacked" } else { "clustered" }), true)
    } else if t.starts_with("line") {
        ("lineChart", "<c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>".to_string(), true)
    } else if t.contains("rea") {
        ("areaChart", format!("<c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>", if stacked { "stacked" } else { "standard" }), true)
    } else if t.starts_with("pie") {
        ("pieChart", "<c:varyColors val=\"1\"/>".to_string(), false)
    } else if t.starts_with("donut") {
        ("doughnutChart", "<c:varyColors val=\"1\"/>".to_string(), false)
    } else if t.starts_with("radar") {
        ("radarChart", "<c:radarStyle val=\"marker\"/><c:varyColors val=\"0\"/>".to_string(), true)
    } else {
        ("barChart", "<c:barDir val=\"col\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/>".to_string(), true)
    };
    let lit_str = |v: &[String]| format!("<c:strLit><c:ptCount val=\"{}\"/>{}</c:strLit>", v.len(), v.iter().enumerate().map(|(i, x)| format!("<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", esc(x))).collect::<String>());
    let lit_num = |v: &[f64]| format!("<c:numLit><c:formatCode>General</c:formatCode><c:ptCount val=\"{}\"/>{}</c:numLit>", v.len(), v.iter().enumerate().filter(|(_, x)| x.is_finite()).map(|(i, x)| format!("<c:pt idx=\"{i}\"><c:v>{x}</c:v></c:pt>")).collect::<String>());
    let round = kind == "pieChart" || kind == "doughnutChart";
    let mut ser = String::new();
    for (i, se) in c.series.iter().enumerate() {
        // (a pie's slices take their own colours; a series its one)
        let fill = if round { String::new() } else { format!("<c:spPr><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill></c:spPr>", hex(se.color)) };
        ser.push_str(&format!(
            "<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:tx><c:v>{}</c:v></c:tx>{fill}<c:cat>{}</c:cat><c:val>{}</c:val></c:ser>",
            esc(&se.name),
            lit_str(&c.categories),
            lit_num(&se.values)
        ));
        if round {
            break;
        }
    }
    let ax_ids = if axes { "<c:axId val=\"500000001\"/><c:axId val=\"500000002\"/>" } else { "" };
    let hole = if kind == "doughnutChart" { "<c:firstSliceAng val=\"0\"/><c:holeSize val=\"50\"/>" } else { "" };
    // (Pages' bar spacing, as its own Word export writes it)
    let gap = if kind == "barChart" { format!("<c:gapWidth val=\"40\"/><c:overlap val=\"{}\"/>", if stacked { 100 } else { -10 }) } else { String::new() };
    let axes_xml = if axes {
        let (cat_pos, val_pos) = if extra.contains("\"bar\"") { ("l", "b") } else { ("b", "l") };
        format!("<c:catAx><c:axId val=\"500000001\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{cat_pos}\"/><c:numFmt formatCode=\"General\" sourceLinked=\"0\"/><c:majorTickMark val=\"none\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/><c:crossAx val=\"500000002\"/><c:crosses val=\"autoZero\"/><c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/></c:catAx><c:valAx><c:axId val=\"500000002\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{val_pos}\"/><c:majorGridlines/><c:numFmt formatCode=\"General\" sourceLinked=\"0\"/><c:majorTickMark val=\"none\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/><c:crossAx val=\"500000001\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"between\"/></c:valAx>")
    } else {
        String::new()
    };
    let title = match &c.title {
        Some(tt) if !tt.is_empty() => format!("<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val=\"0\"/></c:title><c:autoTitleDeleted val=\"0\"/>", esc(tt)),
        _ => "<c:autoTitleDeleted val=\"1\"/>".into(),
    };
    let legend = if c.show_legend { "<c:legend><c:legendPos val=\"t\"/><c:overlay val=\"0\"/></c:legend>" } else { "" };
    format!(
        "{XML_HEAD}<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><c:roundedCorners val=\"0\"/><c:chart>{title}<c:plotArea><c:layout/><c:{kind}>{extra}{ser}{gap}{hole}{ax_ids}</c:{kind}>{axes_xml}</c:plotArea>{legend}<c:plotVisOnly val=\"1\"/></c:chart><c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr></c:chartSpace>"
    )
}

fn wrapped(g: &str, is_pic: bool) -> String {
    if is_pic {
        g.to_string()
    } else {
        // Word 2010 shapes, inside a compatibility choice (Word, OnlyOffice and LibreOffice read it)
        format!("<mc:AlternateContent><mc:Choice Requires=\"wps\">{g}</mc:Choice></mc:AlternateContent>")
    }
}

fn inline_drawing(o: &mut Out, rels: &mut Vec<(String, String, String, bool)>, id: ObjId) -> String {
    let Some(d) = drawable(o.p, id) else { return String::new() };
    let n = o.next_pic();
    let Some((g, pic)) = graphic(o, rels, id, &d, n) else { return String::new() };
    let (cx, cy) = (emu(d.geom.w).max(1), emu(d.geom.h).max(1));
    let x = format!("<wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:docPr id=\"{n}\" name=\"Object {n}\"/><wp:cNvGraphicFramePr/>{g}</wp:inline>");
    if pic {
        format!("<w:r><w:drawing>{x}</w:drawing></w:r>")
    } else {
        format!("<w:r><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing>{x}</w:drawing></mc:Choice></mc:AlternateContent></w:r>")
    }
}

/// A drawable anchored on the page at its page position, with Pages' wrap.
fn anchored(o: &mut Out, rels: &mut Vec<(String, String, String, bool)>, id: ObjId, d: &Drawable, wrap: u64, behind: bool) -> String {
    let n = o.next_pic();
    let Some((g, pic)) = graphic(o, rels, id, d, n) else { return String::new() };
    let (cx, cy) = (emu(d.geom.w).max(1), emu(d.geom.h).max(1));
    let wrap_xml = match (behind, wrap) {
        (true, _) | (_, 2) => "<wp:wrapNone/>",
        (_, 3) => "<wp:wrapTopAndBottom/>",
        _ => "<wp:wrapSquare wrapText=\"bothSides\"/>",
    };
    let x = format!(
        "<wp:anchor distT=\"0\" distB=\"0\" distL=\"114300\" distR=\"114300\" simplePos=\"0\" relativeHeight=\"{}\" behindDoc=\"{}\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\"><wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset></wp:positionH><wp:positionV relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset></wp:positionV><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>{wrap_xml}<wp:docPr id=\"{n}\" name=\"Object {n}\"/><wp:cNvGraphicFramePr/>{g}</wp:anchor>",
        251658240 + n,
        if behind { 1 } else { 0 },
        emu(d.geom.x),
        emu(d.geom.y)
    );
    let _ = wrapped;
    if pic {
        format!("<w:r><w:drawing>{x}</w:drawing></w:r>")
    } else {
        format!("<w:r><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing>{x}</w:drawing></mc:Choice></mc:AlternateContent></w:r>")
    }
}

// ---------------------------------------------------------------------- tables

fn border(s: &Option<kn_model::Stroke>, side: &str) -> String {
    match s {
        Some(st) if st.width > 0.0 && st.color.a > 0.01 => {
            let val = if st.dash.is_empty() { "single" } else if st.dash.first().is_some_and(|d| *d <= 1.5) { "dotted" } else { "dashed" };
            format!("<w:{side} w:val=\"{val}\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>", ((st.width * 8.0).round() as i64).clamp(2, 96), hex(st.color))
        }
        _ => format!("<w:{side} w:val=\"nil\"/>"),
    }
}

/// A table as Word's. `align` / `indent`: where it sits across the text (its paragraph's
/// alignment and left indent); `pics`: floating pictures over its cells, put in them.
fn table_xml(o: &mut Out, rels: &mut Vec<(String, String, String, bool)>, id: ObjId, align: Align, indent: f32, pics: &HashMap<(u32, u32), Vec<ObjId>>) -> String {
    let Some(t) = o.p.file().obj(id).and_then(|m| kn_model::table::table(o.p.file(), m)) else { return String::new() };
    let rows = (0..t.rows).map(|r| *t.row_heights.get(r as usize).unwrap_or(&20.0)).collect::<Vec<_>>();
    let place = match align {
        Align::Center => "<w:jc w:val=\"center\"/>".to_string(),
        Align::Right => "<w:jc w:val=\"right\"/>".to_string(),
        _ if indent.abs() > 0.05 => format!("<w:tblInd w:w=\"{}\" w:type=\"dxa\"/>", tw(indent)),
        _ => String::new(),
    };
    let mut x = format!("<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/>{place}<w:tblLayout w:type=\"fixed\"/><w:tblCellMar><w:left w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"0\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr><w:tblGrid>");
    for c in 0..t.cols {
        x.push_str(&format!("<w:gridCol w:w=\"{}\"/>", tw(*t.col_widths.get(c as usize).unwrap_or(&72.0))));
    }
    x.push_str("</w:tblGrid>");
    let cols = t.cols as usize;
    for r in 0..t.rows {
        let header = r < t.header_rows;
        // the row's least height without its cells' margins and borders (OnlyOffice adds those
        // to it), so a fixed row keeps its height
        let line_w = |b: u32, col: u32| t.h_strokes.get((b * t.cols + col) as usize).cloned().flatten().map(|s| s.width).unwrap_or(0.0);
        let extra = (0..t.cols)
            .map(|col| {
                let st = t.cell_styles.get((r * t.cols + col) as usize);
                let pad = st.map(|s| s.padding).unwrap_or_default();
                pad[1].max(line_w(r, col)) + pad[3].max(if r + 1 == t.rows { line_w(r + 1, col) } else { 0.0 })
            })
            .fold(0.0f32, f32::max);
        let least = (rows[r as usize] - extra).max(0.0);
        // a Word document's exact row (its whole height) and a table kept on one page (its
        // rows unsplit, its paragraphs kept with the next)
        let exact = o.p.exact_rows.get(&id).and_then(|v| v.get(r as usize)).copied().unwrap_or(false);
        let kept = o.p.keep_tables.contains(&id);
        let height = if exact { format!("<w:trHeight w:val=\"{}\" w:hRule=\"exact\"/>", tw(rows[r as usize])) } else { format!("<w:trHeight w:val=\"{}\" w:hRule=\"atLeast\"/>", tw(least)) };
        x.push_str(&format!("<w:tr><w:trPr>{height}{}{}</w:trPr>", if kept { "<w:cantSplit/>" } else { "" }, if header { "<w:tblHeader/>" } else { "" }));
        let keep_p = if kept { "<w:keepNext/>" } else { "" };
        let mut c = 0u32;
        while c < t.cols {
            let m = t.merge_at(r, c).copied();
            // a merged range: its first row spans the columns and starts the vertical merge;
            // the rows below continue it
            let (span, vmerge, src) = match m {
                Some(m) if m.row == r && m.col == c => (m.cols, if m.rows > 1 { "<w:vMerge w:val=\"restart\"/>" } else { "" }, (r, c)),
                Some(m) if m.col == c => (m.cols, "<w:vMerge/>", (m.row, m.col)),
                Some(_) => {
                    c += 1;
                    continue;
                }
                None => (1, "", (r, c)),
            };
            let i = src.0 as usize * cols + src.1 as usize;
            let st = t.cell_styles.get(i).cloned().unwrap_or_default();
            let width: f32 = (c..(c + span).min(t.cols)).map(|k| *t.col_widths.get(k as usize).unwrap_or(&72.0)).sum();
            let h = |b: u32, col: u32| t.h_strokes.get((b * t.cols + col) as usize).cloned().flatten();
            let v = |row: u32, b: u32| t.v_strokes.get((row * (t.cols + 1) + b) as usize).cloned().flatten();
            let last_row = m.map(|m| m.row + m.rows).unwrap_or(r + 1);
            // each grid line once: a cell's top and left, the bottom / right ones only at the
            // table's edge (written on both sides, Word and OnlyOffice count the line twice in
            // the row's height)
            let right_edge = (c + span).min(t.cols);
            let borders = format!(
                "<w:tcBorders>{}{}{}{}</w:tcBorders>",
                border(&h(r, c), "top"),
                border(&v(r, c), "left"),
                if last_row >= t.rows { border(&h(last_row, c), "bottom") } else { String::new() },
                if right_edge >= t.cols { border(&v(r, right_edge), "right") } else { String::new() }
            );
            let fill = match &st.fill {
                Fill::Color(col) if col.a > 0.01 => format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(*col)),
                _ => String::new(),
            };
            let valign = match st.valign {
                kn_model::VAlign::Middle => "center",
                kn_model::VAlign::Bottom => "bottom",
                _ => "top",
            };
            // (Word lays a row out with its borders' width as well: padding without them)
            let mut pad = st.padding;
            let hw = |b: u32| t.h_strokes.get((b * t.cols + c) as usize).cloned().flatten().map(|s| s.width).unwrap_or(0.0);
            pad[1] = (pad[1] - hw(r)).max(0.0);
            if r + 1 == t.rows {
                pad[3] = (pad[3] - hw(r + 1)).max(0.0);
            }
            x.push_str(&format!(
                "<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/>{}{vmerge}{borders}{fill}<w:tcMar><w:top w:w=\"{}\" w:type=\"dxa\"/><w:left w:w=\"{}\" w:type=\"dxa\"/><w:bottom w:w=\"{}\" w:type=\"dxa\"/><w:right w:w=\"{}\" w:type=\"dxa\"/></w:tcMar><w:vAlign w:val=\"{valign}\"/></w:tcPr>",
                tw(width),
                if span > 1 { format!("<w:gridSpan w:val=\"{span}\"/>") } else { String::new() },
                tw(pad[1]),
                tw(pad[0]),
                tw(pad[3]),
                tw(pad[2])
            ));
            // (a cell's text ends with its paragraph's end: no empty paragraph after it in Word)
            let text = if vmerge == "<w:vMerge/>" { String::new() } else { t.cells.get(i).map(|s| s.trim_end_matches(['\n', '\u{2029}']).to_string()).unwrap_or_default() };
            let jc = match st.align {
                Align::Center => "center",
                Align::Right => "right",
                Align::Justify => "both",
                _ => "left",
            };
            let props = rpr(&st.text);
            // (the cell's padding is its margins, w:tcMar; its paragraphs keep their own spacing)
            // a rich text cell: its own paragraphs and runs (fonts, sizes, bold … per run)
            if let Some(tb) = t.rich.get(i).and_then(|r| r.as_ref()).filter(|_| vmerge != "<w:vMerge/>") {
                let n = tb.paragraphs.len();

                for (pi, para) in tb.paragraphs.iter().enumerate() {
                    let before = tw(para.space_before);
                    let after = tw(para.space_after);
                    let _ = n;
                    let line = match para.line_spacing {
                        kn_model::LineSpacing::Relative(k) => format!("w:line=\"{}\" w:lineRule=\"auto\"", (240.0 * k).round() as i64),
                        kn_model::LineSpacing::Exactly(v) => format!("w:line=\"{}\" w:lineRule=\"exact\"", tw(v)),
                        kn_model::LineSpacing::AtLeast(v) => format!("w:line=\"{}\" w:lineRule=\"atLeast\"", tw(v)),
                        _ => "w:line=\"240\" w:lineRule=\"auto\"".to_string(),
                    };
                    let pjc = match para.align {
                        Align::Center => "center",
                        Align::Right => "right",
                        Align::Justify => "both",
                        _ => "left",
                    };
                    let mark = para.runs.first().map(|r| rpr(&r.style)).unwrap_or_else(|| props.clone());
                    // indents (Pages measures the first line from the edge, Word from the left one)
                    let first = para.first_line_indent - para.left_indent;
                    let ind = if para.left_indent != 0.0 || para.right_indent != 0.0 || first != 0.0 {
                        format!("<w:ind w:left=\"{}\" w:right=\"{}\" {}/>", tw(para.left_indent), tw(para.right_indent), if first < 0.0 { format!("w:hanging=\"{}\"", tw(-first)) } else { format!("w:firstLine=\"{}\"", tw(first)) })
                    } else {
                        String::new()
                    };
                    x.push_str(&format!("<w:p><w:pPr>{keep_p}<w:spacing w:before=\"{before}\" w:after=\"{after}\" {line}/>{ind}<w:jc w:val=\"{pjc}\"/><w:rPr>{mark}</w:rPr></w:pPr>"));
                    if pi == 0 {
                        for pid in pics.get(&(r, c)).into_iter().flatten() {
                            x.push_str(&inline_drawing(o, rels, *pid));
                        }
                    }
                    for run in &para.runs {
                        let s = tb.text.get(run.start..run.end).unwrap_or("").trim_end_matches(['\n', '\u{2029}']);
                        if !s.is_empty() {
                            x.push_str(&format!("<w:r><w:rPr>{}</w:rPr>{}</w:r>", rpr(&run.style), text_xml(s, false)));
                        }
                    }
                    x.push_str("</w:p>");
                }
                x.push_str("</w:tc>");
                c += span;
                continue;
            }
            let lines: Vec<&str> = text.split('\n').collect();
            for (li, line) in lines.iter().enumerate() {
                // (a Word document's cell keeps its paragraph spacing and line spacing; a Pages
                // cell's spacing is its padding)
                let (before, after, pitch) = match (o.p.word_layout, st.para.line_spacing) {
                    (true, kn_model::LineSpacing::Relative(k)) => (tw(st.para.space_before), tw(st.para.space_after), (240.0 * k).round() as i64),
                    (true, _) => (tw(st.para.space_before), tw(st.para.space_after), 240),
                    _ => (0, 0, 240),
                };
                // the paragraph mark at the cell's text size: an empty cell is as tall as in Pages,
                // not Word's 11 pt default
                x.push_str(&format!("<w:p><w:pPr>{keep_p}<w:spacing w:before=\"{before}\" w:after=\"{after}\" w:line=\"{pitch}\" w:lineRule=\"auto\"/><w:jc w:val=\"{jc}\"/><w:rPr>{props}</w:rPr></w:pPr>"));
                if li == 0 {
                    for pid in pics.get(&(r, c)).into_iter().flatten() {
                        x.push_str(&inline_drawing(o, rels, *pid));
                    }
                }
                if !line.is_empty() {
                    x.push_str(&format!("<w:r><w:rPr>{props}</w:rPr>{}</w:r>", text_xml(line, false)));
                }
                x.push_str("</w:p>");
            }
            x.push_str("</w:tc>");
            c += span;
        }
        x.push_str("</w:tr>");
    }
    x.push_str("</w:tbl>");
    x
}

// ---------------------------------------------------------------------- headers / footers

/// The header or footer of a master: Pages' three fields on one Word paragraph per line (the
/// centre field after a centre tab, the right one after a right tab), plus an image background.
fn header_part(o: &mut Out, master: ObjId, footer: bool, text_width: f32, background: Option<(ObjId, &Drawable)>, layout_objs: &[ObjId]) -> Option<(String, Vec<(String, String, String, bool)>)> {
    let storages = o.p.header_footer_storages(master, footer);
    let fields: Vec<Vec<Para>> = storages.iter().take(3).map(|s| if o.p.doc.storage_text(*s).trim().is_empty() { Vec::new() } else { crate::export::paragraphs_of(o.p, *s, None) }).collect();
    let lines = fields.iter().map(|f| f.len()).max().unwrap_or(0);
    if lines == 0 && background.is_none() && layout_objs.is_empty() {
        return None;
    }
    let mut rels = Vec::new();
    let style = if footer { "Footer" } else { "Header" };
    let mut x = String::new();
    // where each field's text sits across the column (its indents: a Word footer table's cells
    // came in as them): the left one's indent, a centre tab mid-way in the centre field's box,
    // a right tab at the right field's edge; the fields' own line spacing
    let first = |k: usize| fields[k].first();
    let left_at = first(0).map(|q| q.left).unwrap_or(0.0);
    let centre_at = first(1).map(|q| (q.left + text_width - q.right) / 2.0).unwrap_or(text_width / 2.0);
    let right_at = first(2).map(|q| text_width - q.right).unwrap_or(text_width);
    for i in 0..lines.max(1) {
        let line = fields.iter().find_map(|f| f.get(i)).map(|q| q.line).filter(|l| *l > 0.0).unwrap_or(1.0);
        let after = fields.iter().find_map(|f| f.get(i)).map(|q| q.space_after).unwrap_or(0.0);
        x.push_str(&format!(
            "<w:p><w:pPr><w:pStyle w:val=\"{style}\"/><w:tabs><w:tab w:val=\"center\" w:pos=\"{}\"/><w:tab w:val=\"right\" w:pos=\"{}\"/></w:tabs><w:spacing w:before=\"0\" w:after=\"{}\" w:line=\"{}\" w:lineRule=\"auto\"/>{}</w:pPr>",
            tw(centre_at),
            tw(right_at),
            tw(after),
            (line * 240.0).round() as i64,
            if left_at > 0.05 { format!("<w:ind w:left=\"{}\"/>", tw(left_at)) } else { String::new() }
        ));
        if i == 0 {
            if let Some((bid, d)) = background {
                x.push_str(&anchored(o, &mut rels, bid, d, 2, true));
            }
            // the section template's objects (shown on every page): anchored in the header,
            // which Word repeats on every page too
            // (template objects sit behind the text without pushing it, as in Pages; in a Word
            // header a wrapping object would push the body off every page)
            for id in layout_objs {
                if let Some(d) = drawable(o.p, *id) {
                    x.push_str(&anchored(o, &mut rels, *id, &d, 2, true));
                }
            }
        }
        for (k, f) in fields.iter().enumerate() {
            if k > 0 && fields[k..].iter().any(|g| g.get(i).is_some()) {
                x.push_str("<w:r><w:tab/></w:r>");
            }
            if let Some(para) = f.get(i) {
                for r in &para.runs {
                    x.push_str(&run_xml(o, &mut rels, r, false));
                }
            }
        }
        x.push_str("</w:p>");
    }
    let tag = if footer { "w:ftr" } else { "w:hdr" };
    Some((format!("{XML_HEAD}<{tag} {NS}>{x}</{tag}>"), rels))
}

// ---------------------------------------------------------------------- the document

pub fn docx_files(p: &Pages) -> Result<Vec<(String, Vec<u8>, bool)>, String> {
    LIST_IDS.with(|l| l.borrow_mut().clear());
    let mut fonts = kn_render::Renderer::new();
    let mut cache = crate::flow::Cache::default();
    let layout = crate::flow::layout(p, &mut fonts.fonts, &mut cache, &Default::default());
    let setup = layout.setup.clone();
    let mut o = Out { p, rels: Vec::new(), media: Vec::new(), charts: Vec::new(), footnotes: Vec::new(), comments: Vec::new(), rev: 1, pic_id: 0, renderer: fonts, styles_used: vec!["Body".into()] };
    let k = p.file();
    let floating_kind = |id: ObjId| matches!(k.type_name(id), "TSD.ImageArchive" | "TSWP.ShapeInfoArchive" | "TSD.ShapeArchive" | "TSCH.ChartDrawableArchive" | "TSD.MovieArchive" | "TSD.GroupArchive" | "TST.TableInfoArchive");
    // the floating objects of each page, anchored to the first paragraph shown on it
    let mut page_objs: Vec<(usize, Vec<ObjId>)> = Vec::new();
    for (pi, page) in layout.pages.iter().enumerate() {
        let ids: Vec<ObjId> = page.objects.iter().filter(|ob| !ob.inline && !ob.layout && floating_kind(ob.id) && p.inline_attachment(ob.id).is_none()).map(|ob| ob.id).collect();
        if !ids.is_empty() {
            page_objs.push((pi, ids));
        }
    }
    let mut paras = crate::export::paragraphs(p, None);
    // which paragraph each page's objects anchor to
    let mut anchors: Vec<Vec<ObjId>> = vec![Vec::new(); paras.len().max(1)];
    let mut layout_doc = false;
    if paras.is_empty() || !setup.body {
        // page layout: an empty paragraph per page, a page break after each but the last
        layout_doc = true;
        paras = (0..layout.pages.len().max(1))
            .map(|i| Para { style_name: "Body".into(), level: 0, list: None, align: Align::Left, space_before: 0.0, space_after: 0.0, line: 1.0, line_fixed: None, keep_next: false, keep_lines: false, tabs: Vec::new(), list_style: None, left: 0.0, right: 0.0, first: 0.0, brk: if i + 1 < layout.pages.len() { Some(Break::Page) } else { None }, runs: Vec::new(), objects: Vec::new(), toc: false, border: None, mark: None })
            .collect();
        anchors = vec![Vec::new(); paras.len()];
        for (pi, ids) in &page_objs {
            if let Some(a) = anchors.get_mut(*pi) {
                a.extend(ids.iter().copied());
            }
        }
    } else {
        for (pi, ids) in &page_objs {
            let at = layout.pages[*pi].lines.first().map(|ln| ln.para).unwrap_or(0).min(paras.len() - 1);
            anchors[at].extend(ids.iter().copied());
        }
    }
    // floating pictures lying over a table's cell (a Word cell's picture came in as one): back
    // into that cell
    let mut cell_pics: HashMap<ObjId, HashMap<(u32, u32), Vec<ObjId>>> = HashMap::new();
    for page in &layout.pages {
        for tob in page.objects.iter().filter(|ob| k.type_name(ob.id) == "TST.TableInfoArchive") {
            let Some(t) = k.obj(tob.id).and_then(|m| kn_model::table::table(k, m)) else { continue };
            let rows: Vec<f32> = page.fitted_rows.get(&tob.id).map(|r| r.to_vec()).unwrap_or_else(|| t.row_heights.clone());
            for ob in page.objects.iter().filter(|ob| !ob.inline && !ob.layout && matches!(k.type_name(ob.id), "TSD.ImageArchive" | "TSWP.ShapeInfoArchive")) {
                let inside = ob.x >= tob.x - 1.0 && ob.y >= tob.y - 1.0 && ob.x + ob.w <= tob.x + tob.w + 1.0 && ob.y + ob.h <= tob.y + tob.h + 1.0;
                if !inside {
                    continue;
                }
                let at = |sizes: &[f32], v: f32| -> u32 {
                    let mut acc = 0.0;
                    for (i, s) in sizes.iter().enumerate() {
                        acc += s;
                        if v < acc - 0.5 {
                            return i as u32;
                        }
                    }
                    sizes.len().saturating_sub(1) as u32
                };
                let cell = (at(&rows, ob.y - tob.y + 0.5), at(&t.col_widths, ob.x - tob.x + 0.5));
                cell_pics.entry(tob.id).or_default().entry(cell).or_default().push(ob.id);
                for a in anchors.iter_mut() {
                    a.retain(|id| *id != ob.id);
                }
            }
        }
    }
    // where each later section starts (paragraph index), from the stored body text
    let sections = p.sections();
    let mut section_starts: Vec<usize> = Vec::new();
    if !layout_doc {
        if let Some(b) = p.body_storage() {
            let text = p.doc.storage_text(b);
            for s in sections.iter().skip(1) {
                let mut u = 0usize;
                let mut idx = 0usize;
                for ch in text.chars() {
                    if u >= s.start16 {
                        break;
                    }
                    if crate::body::is_para_end(ch) {
                        idx += 1;
                    }
                    u += ch.len_utf16();
                }
                section_starts.push(idx);
            }
        }
    }
    let mut files: Vec<(String, Vec<u8>, bool)> = Vec::new();
    let mut header_n = 0usize;
    // each section's headers / footers: the sectPr references
    let mut sect_refs = |o: &mut Out, si: usize, files: &mut Vec<(String, Vec<u8>, bool)>| -> String {
        let Some(s) = sections.get(si) else { return String::new() };
        let bg = s.background.clone();
        let mut x = String::new();
        // an image background: a full-page picture behind the text, from the header
        let bg_drawable = match &bg {
            Some(Fill::Image { .. }) => {
                let kind = DrawableKind::Shape(kn_model::ShapeData { path: kn_model::Path::rect(setup.width, setup.height), fill: bg.clone().unwrap(), stroke: None, head: None, tail: None, text: None, is_line: false, is_text_box: false });
                Some(Drawable { id: 0, geom: kn_model::Geometry { x: 0.0, y: 0.0, w: setup.width, h: setup.height, ..Default::default() }, opacity: 1.0, shadow: None, reflection: None, locked: false, hyperlink: None, alt_text: None, placeholder: None, from_template: false, kind })
            }
            _ => None,
        };
        if s.inherit_previous && si > 0 && bg_drawable.is_none() {
            return x;
        }
        let first_differs = s.first_page_different || s.first_page_hides_header_footer;
        // the template objects of the section's pages: its first page, and a later one (the
        // other pages' template when the first page differs)
        let pages_of: Vec<usize> = layout.pages.iter().enumerate().filter(|(_, pg)| pg.section == si).map(|(i, _)| i).collect();
        let objs_on = |pi: Option<&usize>| -> Vec<ObjId> { pi.and_then(|i| layout.pages.get(*i)).map(|pg| pg.objects.iter().filter(|ob| ob.layout).map(|ob| ob.id).collect()).unwrap_or_default() };
        let first_objs = objs_on(pages_of.first());
        let main_objs = if first_differs { objs_on(pages_of.get(1).or(pages_of.first())) } else { first_objs.clone() };
        // the template objects ride in the footer's first line when only the footer has text: an
        // empty header paragraph of their own would be taller than a small top margin and push
        // the body down
        let main_m = [s.odd, s.first, s.even].into_iter().flatten().next();
        let has_text = |footer: bool| main_m.is_some_and(|m| o.p.header_footer_storages(m, footer).iter().take(3).any(|st| !o.p.doc.storage_text(*st).trim().is_empty()));
        let in_footer = setup.footers && has_text(true) && !(setup.headers && has_text(false));
        for footer in [false, true] {
            let on = if footer { setup.footers } else { setup.headers };
            let main = [s.odd, s.first, s.even].into_iter().flatten().next();
            let mut write = |o: &mut Out, master: Option<ObjId>, kind: &str, files: &mut Vec<(String, Vec<u8>, bool)>, hide: bool| {
                let back = if footer { None } else { bg_drawable.as_ref().map(|d| (0u64, d)) };
                let objs: &[ObjId] = if footer != in_footer { &[] } else if kind == "first" { &first_objs } else { &main_objs };
                let part = match (master, on && !hide) {
                    (Some(m), true) => header_part(o, m, footer, setup.text_width(), back, objs),
                    (_, _) if back.is_some() || !objs.is_empty() => {
                        // no header text: the background and template objects alone
                        let mut rels = Vec::new();
                        let mut a = back.map(|b| anchored(o, &mut rels, b.0, b.1, 2, true)).unwrap_or_default();
                        for id in objs {
                            if let Some(d) = drawable(o.p, *id) {
                                a.push_str(&anchored(o, &mut rels, *id, &d, 2, true));
                            }
                        }
                        // (a one-point line: the header takes no room from the page)
                        Some((format!("{XML_HEAD}<w:hdr {NS}><w:p><w:pPr><w:pStyle w:val=\"Header\"/><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"20\" w:lineRule=\"exact\"/><w:rPr><w:sz w:val=\"2\"/></w:rPr></w:pPr>{a}</w:p></w:hdr>"), rels))
                    }
                    _ => None,
                };
                if let Some((xml, rels)) = part {
                    header_n += 1;
                    let name = format!("{}{header_n}.xml", if footer { "footer" } else { "header" });
                    let id = Out::rel(&mut o.rels, if footer { "footer" } else { "header" }, &name, false);
                    files.push((format!("word/{name}"), xml.into_bytes(), false));
                    if !rels.is_empty() {
                        files.push((format!("word/_rels/{name}.rels"), rels_xml(&rels).into_bytes(), false));
                    }
                    x.push_str(&format!("<w:{}Reference w:type=\"{kind}\" r:id=\"{id}\"/>", if footer { "footer" } else { "header" }));
                }
            };
            write(o, main, "default", files, false);
            // left and right pages apart: the even pages' own (settings: evenAndOddHeaders)
            if s.even_odd_different {
                write(o, s.even.or(main), "even", files, false);
            }
            if first_differs {
                write(o, if s.first_page_hides_header_footer { None } else { s.first.or(main) }, "first", files, s.first_page_hides_header_footer);
            }
        }
        if first_differs {
            x.push_str("<w:titlePg/>");
        }
        x
    };
    let page_xml = |s: &crate::PageSetup| -> String {
        format!(
            "<w:pgSz w:w=\"{}\" w:h=\"{}\"{}/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"{}\" w:footer=\"{}\" w:gutter=\"0\"/>",
            tw(s.width),
            tw(s.height),
            if s.landscape || s.width > s.height { " w:orient=\"landscape\"" } else { "" },
            tw(s.top),
            tw(s.right),
            tw(s.bottom),
            tw(s.left),
            tw(s.header_margin),
            tw(s.footer_margin)
        )
    };
    let (cols, gap) = p.columns();
    let cols_xml = if cols > 1 { format!("<w:cols w:num=\"{cols}\" w:space=\"{}\"/>", tw(gap * setup.text_width())) } else { String::new() };
    let mut doc = String::new();
    let mut doc_rels = std::mem::take(&mut o.rels);
    for (i, para) in paras.iter().enumerate() {
        if para.toc {
            doc.push_str("<w:p><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> TOC \\o \"1-3\" \\h </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>Table of Contents</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>");
            continue;
        }
        if !o.styles_used.contains(&para.style_name) {
            o.styles_used.push(para.style_name.clone());
        }
        let (sid, _, _) = word_style(&para.style_name);
        let mut pp = ppr(para, &sid);
        // this paragraph ends a section: that section's settings go here
        if let Some(si) = section_starts.iter().position(|s| *s == i + 1) {
            o.rels = std::mem::take(&mut doc_rels);
            let refs = sect_refs(&mut o, si, &mut files);
            doc_rels = std::mem::take(&mut o.rels);
            // (the section's own page, a Word section's with its size / orientation / margins)
            let sp = sections.get(si).and_then(|s| p.section_pages.get(&s.id)).unwrap_or(&setup);
            pp.push_str(&format!("<w:sectPr>{refs}<w:type w:val=\"nextPage\"/>{}{cols_xml}</w:sectPr>", page_xml(sp)));
        }
        // a paragraph holding only a table: just the table (Word keeps no paragraph around it)
        let table_only = !para.objects.is_empty()
            && para.objects.iter().all(|id| k.type_name(*id) == "TST.TableInfoArchive")
            && para.runs.iter().all(|r| r.text.is_empty())
            && anchors.get(i).is_none_or(|a| a.iter().all(|id| k.type_name(*id) == "TST.TableInfoArchive"))
            && para.brk.is_none()
            && !section_starts.contains(&(i + 1));
        if table_only {
            let n = para.objects.len();
            for (ti, t) in para.objects.clone().into_iter().enumerate() {
                let pics = cell_pics.get(&t).cloned().unwrap_or_default();
                doc.push_str(&table_xml(&mut o, &mut doc_rels, t, para.align, para.left, &pics));
                // (tables one after another stay so, as Word files have them; a document can't
                // end with a table)
                if ti + 1 == n && i + 1 == paras.len() {
                    doc.push_str("<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/></w:pPr></w:p>");
                }
            }
            continue;
        }
        doc.push_str(&format!("<w:p><w:pPr>{pp}</w:pPr>"));
        // anchored objects first (they belong to the paragraph, not to a place in its text)
        for id in anchors.get(i).cloned().unwrap_or_default() {
            if k.type_name(id) == "TST.TableInfoArchive" {
                continue;
            }
            let Some(d) = drawable(p, id) else { continue };
            let (wrap, _) = p.wrap_of(id);
            doc.push_str(&anchored(&mut o, &mut doc_rels, id, &d, wrap, false));
        }
        for r in &para.runs {
            doc.push_str(&run_xml(&mut o, &mut doc_rels, r, true));
        }
        for id in &para.objects {
            if k.type_name(*id) == "TST.TableInfoArchive" {
                continue;
            }
            doc.push_str(&inline_drawing(&mut o, &mut doc_rels, *id));
        }
        match para.brk {
            Some(Break::Page) => doc.push_str("<w:r><w:br w:type=\"page\"/></w:r>"),
            // a column break in one column goes on to the next page (Pages): Word's page break
            Some(Break::Column) if cols <= 1 => doc.push_str("<w:r><w:br w:type=\"page\"/></w:r>"),
            Some(Break::Column) => doc.push_str("<w:r><w:br w:type=\"column\"/></w:r>"),
            _ => {}
        }
        doc.push_str("</w:p>");
        // tables: inline ones in the text, floating ones after their paragraph
        let tables: Vec<ObjId> = para.objects.iter().chain(anchors.get(i).into_iter().flatten()).copied().filter(|id| k.type_name(*id) == "TST.TableInfoArchive").collect();
        for t in tables {
            let pics = cell_pics.get(&t).cloned().unwrap_or_default();
            doc.push_str(&table_xml(&mut o, &mut doc_rels, t, Align::Left, 0.0, &pics));
            doc.push_str("<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/></w:pPr></w:p>");
        }
    }
    o.rels = std::mem::take(&mut doc_rels);
    let last = sections.len().saturating_sub(1).max(section_starts.len());
    let refs = sect_refs(&mut o, last.min(sections.len().saturating_sub(1)), &mut files);
    let background = match sections.first().and_then(|s| s.background.clone()) {
        Some(Fill::Color(c)) => format!("<w:background w:color=\"{}\"/>", hex(c)),
        _ => String::new(),
    };
    let last = sections.last().and_then(|s| p.section_pages.get(&s.id)).unwrap_or(&setup);
    let document = format!("{XML_HEAD}<w:document {NS}>{background}<w:body>{doc}<w:sectPr>{refs}{}{cols_xml}</w:sectPr></w:body></w:document>", page_xml(last));
    // styles: the paragraph styles used, as Word's, with their look
    let mut styles = String::from("<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Helvetica Neue\" w:hAnsi=\"Helvetica Neue\" w:cs=\"Helvetica Neue\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/><w:lang w:val=\"en-US\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>");
    for name in o.styles_used.clone().iter().chain(["Header & Footer".to_string()].iter()) {
        let sid = crate::edit::para_style_named(&p.doc, name);
        let cs = kn_model::style::char_style(p.file(), sid, None);
        let pa = sid.and_then(|s| kn_model::style::chain(p.file(), Some(s)).into_iter().find_map(|m| m.msg("para_properties").cloned()));
        let (before, after) = (pa.as_ref().and_then(|m| m.f32("space_before")).unwrap_or(0.0), pa.as_ref().and_then(|m| m.f32("space_after")).unwrap_or(0.0));
        let ids: Vec<(String, String, Option<u32>)> = if name == "Header & Footer" { vec![("Header".into(), "header".into(), None), ("Footer".into(), "footer".into(), None)] } else { vec![word_style(name)] };
        for (id, wname, outline) in ids {
            styles.push_str(&format!(
                "<w:style w:type=\"paragraph\"{} w:styleId=\"{id}\"><w:name w:val=\"{}\"/>{}<w:qFormat/><w:pPr><w:spacing w:before=\"{}\" w:after=\"{}\"/>{}</w:pPr><w:rPr>{}</w:rPr></w:style>",
                if id == "Normal" { " w:default=\"1\"" } else { "" },
                esc(&wname),
                if id != "Normal" { "<w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/>" } else { "" },
                tw(before),
                tw(after),
                outline.map(|o| format!("<w:keepNext/><w:outlineLvl w:val=\"{o}\"/>")).unwrap_or_default(),
                rpr(&cs)
            ));
        }
    }
    styles.push_str("<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style><w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/><w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>");
    let styles_xml = format!("{XML_HEAD}<w:styles {NS}>{styles}</w:styles>");
    // a Word document's Symbol-font bullet (U+F0B7, as Word writes it) goes back out so
    let symbol_bullet = paras.iter().any(|p| p.list.as_ref().is_some_and(|l| !l.1 && l.0.starts_with('\u{f0b7}')));
    let lvls = |numbered: bool| -> String {
        (0..9)
            .map(|l| {
                let (fmt, text) = if numbered { ("decimal", format!("%{}.", l + 1)) } else if symbol_bullet && l == 0 { ("bullet", "\u{f0b7}".to_string()) } else { ("bullet", ["•", "◦", "▪"][l % 3].to_string()) };
                let rpr = if text == "\u{f0b7}" { "<w:rPr><w:rFonts w:ascii=\"Symbol\" w:hAnsi=\"Symbol\" w:hint=\"default\"/></w:rPr>" } else { "" };
                format!("<w:lvl w:ilvl=\"{l}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"{text}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr>{rpr}</w:lvl>", 720 * (l + 1))
            })
            .collect()
    };
    // each list style the text uses: a Word list of its own, with its levels' number formats
    // (decimal / roman / letters, "1." "(1)" "1)", tiered "1.1."), bullets and indents
    let (mut abs, mut nums) = (String::new(), String::new());
    for (i, id) in LIST_IDS.with(|l| l.borrow().clone()).into_iter().enumerate() {
        let n = 3 + i;
        let levels: String = (0..9)
            .map(|l| {
                let lv = crate::edit::list_level(&p.doc, id, l);
                let (fmt, text, rpr) = match lv.kind {
                    3 => {
                        let fmt = ["decimal", "upperRoman", "lowerRoman", "upperLetter", "lowerLetter"][(lv.number / 3).min(4) as usize];
                        let body = if lv.tiered { (1..=l + 1).map(|k| format!("%{k}")).collect::<Vec<_>>().join(".") } else { format!("%{}", l + 1) };
                        let text = match lv.number % 3 {
                            1 => format!("({body})"),
                            2 => format!("{body})"),
                            _ => format!("{body}."),
                        };
                        (fmt, text, String::new())
                    }
                    _ => {
                        let symbol = lv.string.starts_with('\u{f0b7}');
                        let rpr = if symbol { "<w:rPr><w:rFonts w:ascii=\"Symbol\" w:hAnsi=\"Symbol\" w:hint=\"default\"/></w:rPr>".to_string() } else { String::new() };
                        ("bullet", if lv.string.is_empty() { "•".to_string() } else { lv.string.clone() }, rpr)
                    }
                };
                format!("<w:lvl w:ilvl=\"{l}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"{}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr>{rpr}</w:lvl>", esc(&text), 720 * (l + 1))
            })
            .collect();
        abs.push_str(&format!("<w:abstractNum w:abstractNumId=\"{n}\"><w:multiLevelType w:val=\"multilevel\"/>{levels}</w:abstractNum>"));
        nums.push_str(&format!("<w:num w:numId=\"{n}\"><w:abstractNumId w:val=\"{n}\"/></w:num>"));
    }
    let numbering = format!("{XML_HEAD}<w:numbering {NS}><w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"hybridMultilevel\"/>{}</w:abstractNum><w:abstractNum w:abstractNumId=\"1\"><w:multiLevelType w:val=\"hybridMultilevel\"/>{}</w:abstractNum>{abs}<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num><w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num>{nums}</w:numbering>", lvls(false), lvls(true));
    // the default tab interval (Body's, as Format > Tabs sets it)
    let default_tab = tw(crate::edit::para_style_named(&p.doc, "Body").and_then(|id| p.file().obj(id)).and_then(|m| m.msg("para_properties")).and_then(|pp| pp.f32("default_tab_stops")).filter(|v| *v > 0.0).unwrap_or(36.0));
    let settings = format!(
        "{XML_HEAD}<w:settings {NS}>{}{}<w:defaultTabStop w:val=\"{default_tab}\"/><w:characterSpacingControl w:val=\"doNotCompress\"/><w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat></w:settings>",
        if background.is_empty() { "" } else { "<w:displayBackgroundShape/>" },
        if sections.iter().any(|s| s.even_odd_different) { "<w:evenAndOddHeaders/>" } else { "" }
    );
    let mut rels = std::mem::take(&mut o.rels);
    Out::rel(&mut rels, "styles", "styles.xml", false);
    Out::rel(&mut rels, "numbering", "numbering.xml", false);
    Out::rel(&mut rels, "settings", "settings.xml", false);
    if !o.footnotes.is_empty() {
        Out::rel(&mut rels, "footnotes", "footnotes.xml", false);
        let notes: String = o.footnotes.iter().enumerate().map(|(i, t)| format!("<w:footnote w:id=\"{}\"><w:p><w:pPr><w:pStyle w:val=\"Normal\"/></w:pPr><w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> {}</w:t></w:r></w:p></w:footnote>", i + 1, esc(t))).collect();
        files.push(("word/footnotes.xml".into(), format!("{XML_HEAD}<w:footnotes {NS}><w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>{notes}</w:footnotes>").into_bytes(), false));
    }
    if !o.comments.is_empty() {
        Out::rel(&mut rels, "comments", "comments.xml", false);
        let cs: String = o.comments.iter().enumerate().map(|(n, (t, a))| format!("<w:comment w:id=\"{n}\" w:author=\"{}\"><w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p></w:comment>", esc(a), esc(t))).collect();
        files.push(("word/comments.xml".into(), format!("{XML_HEAD}<w:comments {NS}>{cs}</w:comments>").into_bytes(), false));
    }
    let mut overrides = String::new();
    for i in 0..o.charts.len() {
        overrides.push_str(&format!("<Override PartName=\"/word/charts/chart{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawingml.chart+xml\"/>", i + 1));
    }
    for (name, _, _) in &files {
        let ty = if name.contains("/header") && name.ends_with(".xml") {
            "header"
        } else if name.contains("/footer") && name.ends_with(".xml") && !name.contains("footnotes") {
            "footer"
        } else if name.ends_with("footnotes.xml") {
            "footnotes"
        } else if name.ends_with("comments.xml") {
            "comments"
        } else {
            continue;
        };
        overrides.push_str(&format!("<Override PartName=\"/{name}\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.{ty}+xml\"/>"));
    }
    let content_types = format!(
        "{XML_HEAD}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/><Default Extension=\"gif\" ContentType=\"image/gif\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/><Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>{overrides}<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>"
    );
    let mut out: Vec<(String, Vec<u8>, bool)> = vec![
        ("[Content_Types].xml".into(), content_types.into_bytes(), false),
        ("_rels/.rels".into(), format!("{XML_HEAD}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL}/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>").into_bytes(), false),
        ("docProps/core.xml".into(), format!("{XML_HEAD}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:creator>Letters</dc:creator></cp:coreProperties>").into_bytes(), false),
        ("word/document.xml".into(), document.into_bytes(), false),
        ("word/styles.xml".into(), styles_xml.into_bytes(), false),
        ("word/numbering.xml".into(), numbering.into_bytes(), false),
        ("word/settings.xml".into(), settings.into_bytes(), false),
        ("word/_rels/document.xml.rels".into(), rels_xml(&rels).into_bytes(), false),
    ];
    out.extend(files);
    for (name, bytes) in std::mem::take(&mut o.media) {
        out.push((format!("word/media/{name}"), bytes, true));
    }
    for (i, x) in std::mem::take(&mut o.charts).into_iter().enumerate() {
        out.push((format!("word/charts/chart{}.xml", i + 1), x.into_bytes(), false));
    }
    let _ = LineSpacing::Relative(1.0);
    Ok(out)
}

fn rels_xml(rels: &[(String, String, String, bool)]) -> String {
    format!(
        "{XML_HEAD}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{}</Relationships>",
        rels.iter().map(|(id, ty, target, ext)| format!("<Relationship Id=\"{id}\" Type=\"{ty}\" Target=\"{target}\"{}/>", if *ext { " TargetMode=\"External\"" } else { "" })).collect::<String>()
    )
}
