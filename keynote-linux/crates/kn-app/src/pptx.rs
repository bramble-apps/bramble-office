//! PowerPoint (.pptx), both ways, with real objects (not slide pictures): shapes with their
//! exact outlines, fills and borders; text boxes and shape text with fonts, sizes, colours,
//! alignment, spacing, bullets and insets; pictures; tables (cell text, fills, borders, merges);
//! slide backgrounds; presenter notes; skipped slides; the slide size. Charts, movies and groups
//! go out as pictures of themselves. PowerPoint + Keydeck (as Values' Excel + Values, Letters'
//! Word + Letters) also keeps the whole Keynote document inside the .pptx with a fingerprint
//! of the slides it was saved with: opened again unchanged, Keydeck uses that copy; changed by
//! PowerPoint / OnlyOffice / LibreOffice, it reads the slides.

use std::collections::HashMap;
use std::io::Write;

use kn_format::ObjId;
use kn_model::edit::History;
use kn_model::{Color, Document, Drawable, DrawableKind, Fill, PathEl};
use kn_render::{tiny_skia as sk, Renderer};
use roxmltree::Node;

const KEY_PART: &str = "keydeck/presentation.key";
const PRINT_PART: &str = "keydeck/fingerprint.txt";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn emu(pt: f32) -> i64 {
    (pt as f64 * 12700.0).round() as i64
}

fn hex(c: Color) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("{:02X}{:02X}{:02X}", b(c.r), b(c.g), b(c.b))
}

fn color_xml(c: Color, opacity: f32) -> String {
    let a = (c.a * opacity).clamp(0.0, 1.0);
    if a < 0.999 {
        format!(r#"<a:srgbClr val="{}"><a:alpha val="{}"/></a:srgbClr>"#, hex(c), (a * 100000.0).round() as i64)
    } else {
        format!(r#"<a:srgbClr val="{}"/>"#, hex(c))
    }
}

/// The fingerprint of a package's slide XML (slide parts in name order).
fn slides_print<'a>(parts: impl Iterator<Item = (&'a str, &'a [u8])>) -> String {
    let mut v: Vec<(&str, &[u8])> = parts.filter(|(n, _)| n.starts_with("ppt/slides/slide") && n.ends_with(".xml")).collect();
    v.sort_by(|a, b| a.0.cmp(b.0));
    fingerprint(v.into_iter().map(|p| p.1))
}

/// FNV-1a over bytes (stable across builds).
fn fingerprint<'a>(parts: impl Iterator<Item = &'a [u8]>) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in parts {
        for x in b {
            h ^= *x as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

// ---------------------------------------------------------------------- writing

struct Slide {
    xml: String,
    rels: Vec<(String, String, String)>,
}

struct Out<'a> {
    doc: &'a Document,
    r: &'a mut Renderer,
    media: Vec<(String, Vec<u8>)>,
    id: u32,
}

impl Out<'_> {
    fn media(&mut self, slide: &mut Slide, bytes: Vec<u8>) -> String {
        let ext = if bytes.starts_with(&[0x89, b'P']) { "png" } else if bytes.starts_with(b"GIF") { "gif" } else { "jpeg" };
        let name = format!("image{}.{ext}", self.media.len() + 1);
        self.media.push((name.clone(), bytes));
        let rid = format!("rId{}", slide.rels.len() + 10);
        slide.rels.push((rid.clone(), format!("{REL}/image"), format!("../media/{name}")));
        rid
    }

    fn next(&mut self) -> u32 {
        self.id += 1;
        self.id
    }

    /// A picture of any drawable (charts, movies, groups, masked pictures), with what it draws
    /// outside its frame (chart labels, shadows): the PNG and the frame it covers.
    fn picture(&mut self, d: &Drawable) -> Option<(Vec<u8>, kn_model::Geometry)> {
        let k = 2.0;
        let pad = 120.0 + d.geom.w.max(d.geom.h) * 0.5;
        let (w, h) = (d.geom.w + 2.0 * pad, d.geom.h + 2.0 * pad);
        let mut pm = sk::Pixmap::new((w * k).ceil().max(1.0) as u32, (h * k).ceil().max(1.0) as u32)?;
        let mut d2 = d.clone();
        d2.geom.x = pad;
        d2.geom.y = pad;
        self.r.draw_drawable(self.doc, &d2, &mut pm, sk::Transform::from_scale(k, k));
        // trim to what was drawn
        let (pw, ph) = (pm.width() as usize, pm.height() as usize);
        let px = pm.data();
        let (mut x0, mut y0, mut x1, mut y1) = (pw, ph, 0, 0);
        for y in 0..ph {
            for x in 0..pw {
                if px[(y * pw + x) * 4 + 3] > 0 {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                }
            }
        }
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let crop = pm.clone_rect(sk::IntRect::from_xywh(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32)?)?;
        let mut g = d.geom.clone();
        g.x = d.geom.x - pad + x0 as f32 / k;
        g.y = d.geom.y - pad + y0 as f32 / k;
        g.w = (x1 - x0) as f32 / k;
        g.h = (y1 - y0) as f32 / k;
        // drawn already turned and flipped
        (g.angle, g.hflip, g.vflip) = (0.0, false, false);
        crop.encode_png().ok().map(|b| (b, g))
    }
}

/// Picture bytes PowerPoint shows as they are (others, such as PDF equations, are drawn).
fn web_picture(b: &[u8]) -> bool {
    b.starts_with(&[0x89, b'P', b'N', b'G']) || b.starts_with(&[0xff, 0xd8]) || b.starts_with(b"GIF8")
}

/// Picture bytes Keydeck can keep (PNG / JPEG); anything else is converted to PNG.
fn keepable(b: &[u8]) -> Option<(Vec<u8>, &'static str)> {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some((b.to_vec(), "png"));
    }
    if b.starts_with(&[0xff, 0xd8]) {
        return Some((b.to_vec(), "jpg"));
    }
    let img = image::load_from_memory(b).ok()?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some((out.into_inner(), "png"))
}

fn xfrm(d: &Drawable) -> String {
    let g = &d.geom;
    let rot = if g.angle.abs() > 0.01 { format!(r#" rot="{}""#, ((-g.angle).rem_euclid(360.0) * 60000.0).round() as i64) } else { String::new() };
    let flip = format!("{}{}", if g.hflip { r#" flipH="1""# } else { "" }, if g.vflip { r#" flipV="1""# } else { "" });
    format!(r#"<a:xfrm{rot}{flip}><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm>"#, emu(g.x), emu(g.y), emu(g.w).max(1), emu(g.h).max(1))
}

/// Drop shadow and reflection as PowerPoint effects.
fn effects(d: &Drawable) -> String {
    let mut e = String::new();
    if let Some(sh) = d.shadow.as_ref().filter(|s| s.kind == 0 && s.opacity > 0.0) {
        // Keynote's angle counts counter-clockwise (315 = down-right); PowerPoint's clockwise
        e.push_str(&format!(
            r#"<a:outerShdw blurRad="{}" dist="{}" dir="{}" algn="ctr" rotWithShape="0">{}</a:outerShdw>"#,
            emu(sh.blur),
            emu(sh.offset),
            ((360.0 - sh.angle).rem_euclid(360.0) * 60000.0).round() as i64,
            color_xml(sh.color, sh.opacity)
        ));
    }
    if let Some(r) = d.reflection.filter(|r| *r > 0.0) {
        e.push_str(&format!(r#"<a:reflection blurRad="0" stA="{}" endPos="50000" dist="0" dir="5400000" sy="-100000" algn="bl" rotWithShape="0"/>"#, (r.clamp(0.0, 1.0) * 100000.0).round() as i64));
    }
    if e.is_empty() { e } else { format!("<a:effectLst>{e}</a:effectLst>") }
}

fn fill_xml(o: &mut Out, slide: &mut Slide, f: &Fill, opacity: f32) -> String {
    match f {
        Fill::Color(c) => format!("<a:solidFill>{}</a:solidFill>", color_xml(*c, opacity)),
        Fill::Gradient { kind, stops, opacity: go, .. } => {
            let opacity = opacity * go;
            let angle = match kind {
                kn_model::GradientKind::Linear { angle } => *angle,
                kn_model::GradientKind::Radial => 0.0,
            };
            let gs: String = stops.iter().map(|(pos, c)| format!(r#"<a:gs pos="{}">{}</a:gs>"#, (pos.clamp(0.0, 1.0) * 100000.0).round() as i64, color_xml(*c, opacity))).collect();
            format!(r#"<a:gradFill rotWithShape="1"><a:gsLst>{gs}</a:gsLst><a:lin ang="{}" scaled="0"/></a:gradFill>"#, ((-angle).rem_euclid(360.0) * 60000.0).round() as i64)
        }
        Fill::Image { data_id, .. } => match o.doc.data(*data_id) {
            Some(b) => {
                let rid = o.media(slide, b.to_vec());
                format!(r#"<a:blipFill dpi="0" rotWithShape="1"><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></a:blipFill>"#)
            }
            None => "<a:noFill/>".into(),
        },
        _ => "<a:noFill/>".into(),
    }
}

fn cust_geom(path: &kn_model::Path, w: f32, h: f32) -> String {
    let pt = |q: &kn_model::Point| format!(r#"<a:pt x="{}" y="{}"/>"#, emu(q.x), emu(q.y));
    let mut x = String::new();
    for e in &path.els {
        match e {
            PathEl::Move(q) => x.push_str(&format!("<a:moveTo>{}</a:moveTo>", pt(q))),
            PathEl::Line(q) => x.push_str(&format!("<a:lnTo>{}</a:lnTo>", pt(q))),
            PathEl::Quad(c, q) => x.push_str(&format!("<a:quadBezTo>{}{}</a:quadBezTo>", pt(c), pt(q))),
            PathEl::Cubic(a, b, q) => x.push_str(&format!("<a:cubicBezTo>{}{}{}</a:cubicBezTo>", pt(a), pt(b), pt(q))),
            PathEl::Close => x.push_str("<a:close/>"),
        }
    }
    format!(r#"<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l="0" t="0" r="r" b="b"/><a:pathLst><a:path w="{}" h="{}">{x}</a:path></a:pathLst></a:custGeom>"#, emu(w).max(1), emu(h).max(1))
}

fn txbody(tb: &kn_model::TextBox, auto_width: bool, scale: f32) -> String {
    let [pl, pt, pr, pb] = tb.padding;
    let anchor = match tb.valign {
        kn_model::VAlign::Middle => "ctr",
        kn_model::VAlign::Bottom => "b",
        _ => "t",
    };
    let fit = if tb.shrink_to_fit && scale < 0.999 {
        format!(r#"<a:normAutofit fontScale="{}"/>"#, (scale * 100000.0).round() as i64)
    } else if tb.shrink_to_fit {
        "<a:normAutofit/>".to_string()
    } else {
        "<a:noAutofit/>".to_string()
    };
    format!(
        r#"<p:txBody><a:bodyPr wrap="{}" lIns="{}" tIns="{}" rIns="{}" bIns="{}" anchor="{anchor}" rtlCol="0">{fit}</a:bodyPr><a:lstStyle/>{}</p:txBody>"#,
        if auto_width { "none" } else { "square" },
        emu(pl),
        emu(pt),
        emu(pr),
        emu(pb),
        crate::pptx_text::paragraphs_xml(tb)
    )
}

fn shape_xml(o: &mut Out, slide: &mut Slide, d: &Drawable) -> String {
    let DrawableKind::Shape(s) = &d.kind else { return String::new() };
    // text boxes that size themselves: the frame as drawn
    let auto_w = d.geom.auto_width && (s.is_text_box || d.placeholder.is_some() || d.geom.w <= 0.0);
    // shrunk to fit: by how much, so PowerPoint starts from the same size
    let scale = s.text.as_ref().filter(|t| t.shrink_to_fit).and_then(|_| o.r.text_layout(d)).map(|(tl, _)| tl.scale).unwrap_or(1.0);
    let mut sized;
    let d = if s.text.is_some() && (s.is_text_box || d.geom.w <= 0.0 || d.geom.h <= 0.0) {
        match o.r.text_layout(d) {
            Some((_, g)) => {
                sized = d.clone();
                (sized.geom.x, sized.geom.y, sized.geom.w, sized.geom.h) = (g.x, g.y, g.w, g.h);
                &sized
            }
            None => d,
        }
    } else {
        d
    };
    let n = o.next();
    let geom = if s.is_text_box && !s.is_line { r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#.to_string() } else { cust_geom(&s.path, d.geom.w, d.geom.h) };
    let fill = if s.is_line { "<a:noFill/>".to_string() } else { fill_xml(o, slide, &s.fill, d.opacity) };
    let line = match &s.stroke {
        Some(st) => {
            let dash = if st.dash.is_empty() { "" } else if st.dash.first().is_some_and(|v| *v <= 1.5) { r#"<a:prstDash val="sysDot"/>"# } else { r#"<a:prstDash val="dash"/>"# };
            format!(
                r#"<a:ln w="{}"><a:solidFill>{}</a:solidFill>{dash}{}{}</a:ln>"#,
                emu(st.width),
                color_xml(st.color, d.opacity),
                if s.tail.is_some() { r#"<a:headEnd type="triangle"/>"# } else { "" },
                if s.head.is_some() { r#"<a:tailEnd type="triangle"/>"# } else { "" }
            )
        }
        None => "<a:ln><a:noFill/></a:ln>".into(),
    };
    let text = s.text.as_ref().filter(|t| !t.text.is_empty()).map(|t| txbody(t, auto_w, scale)).unwrap_or_default();
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{n}" name="{} {n}"/><p:cNvSpPr{}/><p:nvPr/></p:nvSpPr><p:spPr>{}{geom}{fill}{line}{}</p:spPr>{text}</p:sp>"#,
        if s.is_text_box { "TextBox" } else { "Shape" },
        if s.is_text_box { r#" txBox="1""# } else { "" },
        xfrm(d),
        effects(d)
    )
}

fn pic_xml(o: &mut Out, slide: &mut Slide, d: &Drawable, bytes: Vec<u8>) -> String {
    let n = o.next();
    let rid = o.media(slide, bytes);
    format!(
        r#"<p:pic><p:nvPicPr><p:cNvPr id="{n}" name="Picture {n}"{}/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>{}</p:spPr></p:pic>"#,
        d.alt_text.as_ref().map(|t| format!(r#" descr="{}""#, esc(t))).unwrap_or_default(),
        xfrm(d),
        effects(d)
    )
}

fn table_xml(o: &mut Out, d: &Drawable, t: &kn_model::TableData) -> String {
    let n = o.next();
    let border = |s: Option<kn_model::Stroke>, tag: &str| match s {
        Some(st) if st.width > 0.0 => format!(r#"<a:{tag} w="{}"><a:solidFill>{}</a:solidFill></a:{tag}>"#, emu(st.width), color_xml(st.color, 1.0)),
        _ => format!("<a:{tag} w=\"0\"><a:noFill/></a:{tag}>"),
    };
    let mut grid = String::new();
    for c in 0..t.cols {
        grid.push_str(&format!(r#"<a:gridCol w="{}"/>"#, emu(*t.col_widths.get(c as usize).unwrap_or(&72.0))));
    }
    let mut rows = String::new();
    for r in 0..t.rows {
        rows.push_str(&format!(r#"<a:tr h="{}">"#, emu(*t.row_heights.get(r as usize).unwrap_or(&20.0))));
        for c in 0..t.cols {
            let i = (r * t.cols + c) as usize;
            let m = t.merge_at(r, c).copied();
            let span = match m {
                Some(m) if m.row == r && m.col == c => format!("{}{}", if m.cols > 1 { format!(r#" gridSpan="{}""#, m.cols) } else { String::new() }, if m.rows > 1 { format!(r#" rowSpan="{}""#, m.rows) } else { String::new() }),
                Some(m) => format!("{}{}", if m.col != c { r#" hMerge="1""# } else { "" }, if m.row != r { r#" vMerge="1""# } else { "" }),
                None => String::new(),
            };
            let st = t.cell_styles.get(i).cloned().unwrap_or_default();
            let text = t.cells.get(i).cloned().unwrap_or_default();
            let algn = match st.align {
                kn_model::Align::Center => "ctr",
                kn_model::Align::Right => "r",
                kn_model::Align::Justify => "just",
                _ => "l",
            };
            let (family, weight, italic) = kn_render::fonts::parse_ps_name(&st.text.font_name);
            let rpr = format!(
                r#"<a:rPr lang="en-US" sz="{}" b="{}" i="{}"><a:solidFill>{}</a:solidFill><a:latin typeface="{}"/></a:rPr>"#,
                (st.text.size * 100.0).round() as i64,
                u8::from(st.text.bold || weight >= 600),
                u8::from(st.text.italic || italic),
                color_xml(st.text.color, 1.0),
                esc(&family)
            );
            let paras: String = text.trim_end_matches('\n').split('\n').map(|l| format!(r#"<a:p><a:pPr algn="{algn}"/>{}<a:endParaRPr lang="en-US" sz="{}"/></a:p>"#, if l.is_empty() { String::new() } else { format!("<a:r>{rpr}<a:t>{}</a:t></a:r>", esc(l)) }, (st.text.size * 100.0).round() as i64)).collect();
            let anchor = match st.valign {
                kn_model::VAlign::Middle => "ctr",
                kn_model::VAlign::Bottom => "b",
                _ => "t",
            };
            let h = |b: u32, col: u32| t.h_strokes.get((b * t.cols + col) as usize).cloned().flatten();
            let v = |row: u32, b: u32| t.v_strokes.get((row * (t.cols + 1) + b) as usize).cloned().flatten();
            let fill = match &st.fill {
                Fill::Color(col) if col.a > 0.01 => format!("<a:solidFill>{}</a:solidFill>", color_xml(*col, 1.0)),
                _ => "<a:noFill/>".into(),
            };
            let [pl, ptp, pr, pb] = st.padding;
            rows.push_str(&format!(
                r#"<a:tc{span}><a:txBody><a:bodyPr/><a:lstStyle/>{paras}</a:txBody><a:tcPr marL="{}" marR="{}" marT="{}" marB="{}" anchor="{anchor}">{}{}{}{}{fill}</a:tcPr></a:tc>"#,
                emu(pl),
                emu(pr),
                emu(ptp),
                emu(pb),
                border(v(r, c), "lnL"),
                border(v(r, c + 1), "lnR"),
                border(h(r, c), "lnT"),
                border(h(r + 1, c), "lnB")
            ));
        }
        rows.push_str("</a:tr>");
    }
    format!(
        r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{n}" name="Table {n}"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr firstRow="{}"/><a:tblGrid>{grid}</a:tblGrid>{rows}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#,
        emu(d.geom.x),
        emu(d.geom.y),
        emu(t.width().max(d.geom.w)),
        emu(t.height().max(d.geom.h)),
        u8::from(t.header_rows > 0)
    )
}

fn drawable_xml(o: &mut Out, slide: &mut Slide, d: &Drawable) -> String {
    match &d.kind {
        DrawableKind::Shape(_) => shape_xml(o, slide, d),
        DrawableKind::Image(img) if img.mask.is_none() && img.data_id.and_then(|i| o.doc.data(i)).is_some_and(|b| web_picture(&b)) && d.opacity > 0.99 => {
            let bytes = o.doc.data(img.data_id.unwrap()).unwrap().to_vec();
            pic_xml(o, slide, d, bytes)
        }
        DrawableKind::Table(t) => table_xml(o, d, t),
        _ => match o.picture(d) {
            Some((b, g)) => {
                let mut d2 = d.clone();
                d2.geom = g;
                (d2.shadow, d2.reflection) = (None, None); // drawn into the picture
                pic_xml(o, slide, &d2, b)
            }
            None => String::new(),
        },
    }
}

/// The parts of a .pptx for the document.
pub fn files(doc: &Document, r: &mut Renderer) -> Result<Vec<(String, Vec<u8>)>, String> {
    let slides = doc.slides();
    let size = doc.slide_size();
    let (cx, cy) = (emu(size.w), emu(size.h));
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut o = Out { doc, r, media: Vec::new(), id: 1 };
    let tree = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;
    let mut slide_parts = Vec::new();
    for (i, info) in slides.iter().enumerate() {
        let Some(sc) = doc.scene(&slides, i) else { continue };
        let mut s = Slide { xml: String::new(), rels: Vec::new() };
        o.id = 1;
        let bg = match &sc.background {
            Fill::None => String::new(),
            f => {
                let fx = fill_xml(&mut o, &mut s, f, 1.0);
                format!("<p:bg><p:bgPr>{fx}<a:effectLst/></p:bgPr></p:bg>")
            }
        };
        let mut body = String::new();
        for d in &sc.drawables {
            body.push_str(&drawable_xml(&mut o, &mut s, d));
        }
        let show = if info.skipped { r#" show="0""# } else { "" };
        s.xml = format!(r#"{HEAD}<p:sld {NS}{show}><p:cSld>{bg}<p:spTree>{tree}{body}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#);
        slide_parts.push((s, doc.notes(info.slide)));
    }
    let n = slide_parts.len();
    let mut ct = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="jpeg" ContentType="image/jpeg"/><Default Extension="png" ContentType="image/png"/><Default Extension="gif" ContentType="image/gif"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/ppt/notesMasters/notesMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml"/><Override PartName="/ppt/theme/theme2.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>"#);
    for i in 1..=n {
        ct.push_str(&format!(r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/><Override PartName="/ppt/notesSlides/notesSlide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"/>"#));
    }
    ct.push_str("</Types>");
    out.push(("[Content_Types].xml".into(), ct.into_bytes()));
    out.push(("_rels/.rels".into(), format!(r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/officeDocument" Target="ppt/presentation.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#).into_bytes()));
    out.push(("docProps/core.xml".into(), format!(r#"{HEAD}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>Keydeck</dc:creator></cp:coreProperties>"#).into_bytes()));
    let mut sld_ids = String::new();
    let mut pres_rels = format!(r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/slideMaster" Target="slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="{REL}/theme" Target="theme/theme1.xml"/><Relationship Id="rId3" Type="{REL}/notesMaster" Target="notesMasters/notesMaster1.xml"/>"#);
    for i in 1..=n {
        sld_ids.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 255 + i, 10 + i));
        pres_rels.push_str(&format!(r#"<Relationship Id="rId{}" Type="{REL}/slide" Target="slides/slide{i}.xml"/>"#, 10 + i));
    }
    pres_rels.push_str("</Relationships>");
    out.push(("ppt/presentation.xml".into(), format!(r#"{HEAD}<p:presentation {NS}><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:notesMasterIdLst><p:notesMasterId r:id="rId3"/></p:notesMasterIdLst><p:sldIdLst>{sld_ids}</p:sldIdLst><p:sldSz cx="{cx}" cy="{cy}"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#).into_bytes()));
    out.push(("ppt/_rels/presentation.xml.rels".into(), pres_rels.into_bytes()));
    let theme = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Keydeck"><a:themeElements><a:clrScheme name="Keydeck"><a:dk1><a:srgbClr val="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1F1F1F"/></a:dk2><a:lt2><a:srgbClr val="EEEEEE"/></a:lt2><a:accent1><a:srgbClr val="0A84FF"/></a:accent1><a:accent2><a:srgbClr val="30D158"/></a:accent2><a:accent3><a:srgbClr val="FFD60A"/></a:accent3><a:accent4><a:srgbClr val="FF9F0A"/></a:accent4><a:accent5><a:srgbClr val="FF453A"/></a:accent5><a:accent6><a:srgbClr val="BF5AF2"/></a:accent6><a:hlink><a:srgbClr val="0A84FF"/></a:hlink><a:folHlink><a:srgbClr val="BF5AF2"/></a:folHlink></a:clrScheme><a:fontScheme name="Keydeck"><a:majorFont><a:latin typeface="Helvetica Neue"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Helvetica Neue"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Keydeck"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#;
    out.push(("ppt/theme/theme1.xml".into(), theme.as_bytes().to_vec()));
    out.push(("ppt/theme/theme2.xml".into(), theme.as_bytes().to_vec()));
    let clr = r#"<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>"#;
    out.push(("ppt/slideMasters/slideMaster1.xml".into(), format!(r#"{HEAD}<p:sldMaster {NS}><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree>{tree}</p:spTree></p:cSld>{clr}<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#).into_bytes()));
    out.push(("ppt/slideMasters/_rels/slideMaster1.xml.rels".into(), format!(r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="{REL}/theme" Target="../theme/theme1.xml"/></Relationships>"#).into_bytes()));
    out.push(("ppt/slideLayouts/slideLayout1.xml".into(), format!(r#"{HEAD}<p:sldLayout {NS} type="blank"><p:cSld name="Blank"><p:spTree>{tree}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#).into_bytes()));
    out.push(("ppt/slideLayouts/_rels/slideLayout1.xml.rels".into(), format!(r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#).into_bytes()));
    out.push(("ppt/notesMasters/notesMaster1.xml".into(), format!(r#"{HEAD}<p:notesMaster {NS}><p:cSld><p:spTree>{tree}</p:spTree></p:cSld>{clr}</p:notesMaster>"#).into_bytes()));
    out.push(("ppt/notesMasters/_rels/notesMaster1.xml.rels".into(), format!(r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/theme" Target="../theme/theme2.xml"/></Relationships>"#).into_bytes()));
    for (i, (s, notes)) in slide_parts.into_iter().enumerate() {
        let k = i + 1;
        out.push((format!("ppt/slides/slide{k}.xml"), s.xml.into_bytes()));
        let mut rels = format!(r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="{REL}/notesSlide" Target="../notesSlides/notesSlide{k}.xml"/>"#);
        for (id, ty, target) in &s.rels {
            rels.push_str(&format!(r#"<Relationship Id="{id}" Type="{ty}" Target="{target}"/>"#));
        }
        rels.push_str("</Relationships>");
        out.push((format!("ppt/slides/_rels/slide{k}.xml.rels"), rels.into_bytes()));
        let paras: String = notes.split('\n').filter(|l| !l.is_empty()).map(|l| format!(r#"<a:p><a:r><a:rPr lang="en-US"/><a:t>{}</a:t></a:r></a:p>"#, esc(l))).collect();
        let paras = if paras.is_empty() { "<a:p/>".to_string() } else { paras };
        out.push((format!("ppt/notesSlides/notesSlide{k}.xml"), format!(r#"{HEAD}<p:notes {NS}><p:cSld><p:spTree>{tree}<p:sp><p:nvSpPr><p:cNvPr id="2" name="Notes"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="685800" y="4343400"/><a:ext cx="5486400" cy="4114800"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/>{paras}</p:txBody></p:sp></p:spTree></p:cSld></p:notes>"#).into_bytes()));
        out.push((format!("ppt/notesSlides/_rels/notesSlide{k}.xml.rels"), format!(r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/notesMaster" Target="../notesMasters/notesMaster1.xml"/><Relationship Id="rId2" Type="{REL}/slide" Target="../slides/slide{k}.xml"/></Relationships>"#).into_bytes()));
    }
    for (name, b) in o.media {
        out.push((format!("ppt/media/{name}"), b));
    }
    Ok(out)
}

/// Save as PowerPoint: plain, or with the Keynote document inside (`key` = its bytes).
pub fn write(doc: &Document, r: &mut Renderer, path: &std::path::Path, key: Option<Vec<u8>>) -> Result<(), String> {
    let mut parts = files(doc, r)?;
    if let Some(bytes) = key {
        let print = slides_print(parts.iter().map(|p| (p.0.as_str(), p.1.as_slice())));
        for p in parts.iter_mut() {
            if p.0 == "[Content_Types].xml" {
                p.1 = String::from_utf8_lossy(&p.1).replace(r#"<Default Extension="rels""#, r#"<Default Extension="key" ContentType="application/vnd.apple.keynote"/><Default Extension="txt" ContentType="text/plain"/><Default Extension="rels""#).into_bytes();
            }
            if p.0 == "_rels/.rels" {
                p.1 = String::from_utf8_lossy(&p.1).replace("</Relationships>", &format!(r#"<Relationship Id="rIdKeydeck" Type="https://keydeck.app/relationships/keynote-document" Target="{KEY_PART}"/></Relationships>"#)).into_bytes();
            }
        }
        parts.push((KEY_PART.into(), bytes));
        parts.push((PRINT_PART.into(), print.into_bytes()));
    }
    let tmp = path.with_extension("pptx.tmp");
    {
        let f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut z = zip::ZipWriter::new(f);
        for (name, b) in parts {
            let store = name.starts_with("ppt/media/") || name == KEY_PART;
            let o = zip::write::SimpleFileOptions::default().compression_method(if store { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated });
            z.start_file(name, o).map_err(|e| e.to_string())?;
            z.write_all(&b).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------- reading

fn attr<'a>(n: &Node<'a, '_>, local: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == local).map(|a| a.value())
}

fn child<'a, 'i>(n: &Node<'a, 'i>, local: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.tag_name().name() == local)
}

fn num(n: &Node, a: &str) -> Option<f32> {
    attr(n, a).and_then(|v| v.parse::<f64>().ok()).map(|v| v as f32)
}

fn pt(v: Option<f32>) -> f32 {
    v.unwrap_or(0.0) / 12700.0
}

struct Package {
    files: HashMap<String, Vec<u8>>,
    /// The text size runs without their own get (presentation's default text style), points.
    def_sz: f32,
}

impl Package {
    fn text(&self, name: &str) -> Option<String> {
        self.files.get(name).map(|b| String::from_utf8_lossy(b).into_owned())
    }

    /// part -> relationship id -> target part
    fn rels(&self, part: &str) -> HashMap<String, String> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let mut out = HashMap::new();
        if let Some(x) = self.text(&format!("{dir}/_rels/{file}.rels")) {
            if let Ok(d) = roxmltree::Document::parse(&x) {
                for r in d.descendants().filter(|n| n.tag_name().name() == "Relationship") {
                    if let (Some(id), Some(t)) = (r.attribute("Id"), r.attribute("Target")) {
                        let target = if r.attribute("TargetMode") == Some("External") {
                            t.to_string()
                        } else if let Some(abs) = t.strip_prefix('/') {
                            abs.to_string()
                        } else {
                            // resolve ../ against the part's folder
                            let mut segs: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
                            for s in t.split('/') {
                                match s {
                                    ".." => {
                                        segs.pop();
                                    }
                                    "." | "" => {}
                                    s => segs.push(s),
                                }
                            }
                            segs.join("/")
                        };
                        out.insert(id.to_string(), target);
                    }
                }
            }
        }
        out
    }
}

/// The theme's colours (scheme name -> colour), for schemeClr.
fn theme_colors(pkg: &Package, master: &str) -> HashMap<String, Color> {
    let mut out = HashMap::new();
    let Some(theme) = pkg.rels(master).into_values().find(|t| t.contains("theme/")) else { return out };
    let Some(x) = pkg.text(&theme) else { return out };
    let Ok(d) = roxmltree::Document::parse(&x) else { return out };
    if let Some(cs) = d.descendants().find(|n| n.tag_name().name() == "clrScheme") {
        for c in cs.children().filter(|c| c.is_element()) {
            let v = c.descendants().find_map(|n| match n.tag_name().name() {
                "srgbClr" => attr(&n, "val").map(str::to_string),
                "sysClr" => attr(&n, "lastClr").map(str::to_string),
                _ => None,
            });
            if let Some(col) = v.and_then(|v| hex_color(&v)) {
                out.insert(c.tag_name().name().to_string(), col);
            }
        }
    }
    // the usual mapping of the text / background names
    for (k, v) in [("tx1", "dk1"), ("bg1", "lt1"), ("tx2", "dk2"), ("bg2", "lt2")] {
        if let Some(c) = out.get(v).copied() {
            out.insert(k.into(), c);
        }
    }
    out
}

fn hex_color(v: &str) -> Option<Color> {
    if v.len() != 6 {
        return None;
    }
    let h = |i: usize| u8::from_str_radix(&v[i..i + 2], 16).ok().map(|b| b as f32 / 255.0);
    Some(Color { r: h(0)?, g: h(2)?, b: h(4)?, a: 1.0 })
}

/// A colour element (srgbClr / schemeClr / sysClr …) with lumMod / lumOff / alpha.
fn color_of(n: Node, theme: &HashMap<String, Color>) -> Option<Color> {
    let c = n.children().find(|c| matches!(c.tag_name().name(), "srgbClr" | "schemeClr" | "sysClr" | "prstClr"))?;
    let mut col = match c.tag_name().name() {
        "srgbClr" => hex_color(attr(&c, "val")?)?,
        "sysClr" => hex_color(attr(&c, "lastClr").unwrap_or("000000"))?,
        "schemeClr" => *theme.get(attr(&c, "val")?)?,
        _ => match attr(&c, "val")? {
            "white" => Color::WHITE,
            _ => Color::BLACK,
        },
    };
    for m in c.children().filter(|m| m.is_element()) {
        let v = num(&m, "val").unwrap_or(100000.0) / 100000.0;
        match m.tag_name().name() {
            "lumMod" => {
                col.r *= v;
                col.g *= v;
                col.b *= v;
            }
            "lumOff" => {
                col.r = (col.r + v).min(1.0);
                col.g = (col.g + v).min(1.0);
                col.b = (col.b + v).min(1.0);
            }
            "alpha" => col.a = v,
            _ => {}
        }
    }
    Some(col)
}

/// An object read off a slide, in slide points.
struct Obj<'a> {
    node: Node<'a, 'a>,
    g: kn_model::Geometry,
}

/// Group transforms: child coordinates mapped into slide coordinates.
fn collect<'a>(tree: Node<'a, 'a>, map: &dyn Fn(f32, f32, f32, f32) -> (f32, f32, f32, f32), out: &mut Vec<Obj<'a>>) {
    for n in tree.children().filter(|c| c.is_element()) {
        let name = n.tag_name().name();
        let xf = n.descendants().find(|c| matches!(c.tag_name().name(), "xfrm") && c.parent().is_some_and(|p| matches!(p.tag_name().name(), "spPr" | "grpSpPr" | "graphicFrame")));
        let frame = |x: Node| -> (f32, f32, f32, f32, f32, bool, bool) {
            let off = child(&x, "off");
            let ext = child(&x, "ext");
            (
                pt(off.and_then(|o| num(&o, "x"))),
                pt(off.and_then(|o| num(&o, "y"))),
                pt(ext.and_then(|e| num(&e, "cx"))),
                pt(ext.and_then(|e| num(&e, "cy"))),
                num(&x, "rot").unwrap_or(0.0) / 60000.0,
                attr(&x, "flipH") == Some("1"),
                attr(&x, "flipV") == Some("1"),
            )
        };
        match name {
            "grpSp" => {
                let Some(x) = child(&n, "grpSpPr").and_then(|p| child(&p, "xfrm")) else { continue };
                let (ox, oy, w, h, _, _, _) = frame(x);
                let cho = child(&x, "chOff");
                let che = child(&x, "chExt");
                let (cx, cy) = (pt(cho.and_then(|o| num(&o, "x"))), pt(cho.and_then(|o| num(&o, "y"))));
                let (cw, ch) = (pt(che.and_then(|e| num(&e, "cx"))).max(0.001), pt(che.and_then(|e| num(&e, "cy"))).max(0.001));
                let (sx, sy) = (w / cw, h / ch);
                let inner = move |x: f32, y: f32, w2: f32, h2: f32| map(ox + (x - cx) * sx, oy + (y - cy) * sy, w2 * sx, h2 * sy);
                collect(n, &inner, out);
            }
            "sp" | "pic" | "graphicFrame" | "cxnSp" => {
                let x = if name == "graphicFrame" { child(&n, "xfrm") } else { xf };
                let (x0, y0, w, h, rot, fh, fv) = x.map(frame).unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0, false, false));
                let (x1, y1, w1, h1) = map(x0, y0, w, h);
                out.push(Obj { node: n, g: kn_model::Geometry { x: x1, y: y1, w: w1, h: h1, angle: (-rot).rem_euclid(360.0), hflip: fh, vflip: fv, ..Default::default() } });
            }
            _ => {}
        }
    }
}

/// A placeholder's frame from the layout / master (slides often leave it out).
fn placeholder_frame(pkg: &Package, part: &str, n: Node) -> Option<kn_model::Geometry> {
    let ph = n.descendants().find(|c| c.tag_name().name() == "ph")?;
    let (ty, idx) = (attr(&ph, "type").unwrap_or("body"), attr(&ph, "idx"));
    let layout = pkg.rels(part).into_values().find(|t| t.contains("slideLayouts/"))?;
    let master = pkg.rels(&layout).into_values().find(|t| t.contains("slideMasters/"));
    for p in [Some(layout), master].into_iter().flatten() {
        let Some(x) = pkg.text(&p) else { continue };
        let Ok(d) = roxmltree::Document::parse(&x) else { continue };
        for sp in d.descendants().filter(|c| c.tag_name().name() == "sp") {
            let Some(q) = sp.descendants().find(|c| c.tag_name().name() == "ph") else { continue };
            let qt = attr(&q, "type").unwrap_or("body");
            let same = (idx.is_some() && attr(&q, "idx") == idx) || qt == ty || (ty == "ctrTitle" && qt == "title") || (ty == "subTitle" && qt == "body");
            if !same {
                continue;
            }
            if let Some(xf) = sp.descendants().find(|c| c.tag_name().name() == "xfrm") {
                let off = child(&xf, "off");
                let ext = child(&xf, "ext");
                return Some(kn_model::Geometry { x: pt(off.and_then(|o| num(&o, "x"))), y: pt(off.and_then(|o| num(&o, "y"))), w: pt(ext.and_then(|e| num(&e, "cx"))), h: pt(ext.and_then(|e| num(&e, "cy"))), ..Default::default() });
            }
        }
    }
    None
}

/// A placeholder's inherited text size: the layout's / master's own list style, else the
/// master's title or body style.
fn placeholder_size(pkg: &Package, part: &str, n: Node) -> Option<f32> {
    let ph = n.descendants().find(|c| c.tag_name().name() == "ph")?;
    let (ty, idx) = (attr(&ph, "type").unwrap_or("body"), attr(&ph, "idx"));
    let lvl1 = |x: Node| x.descendants().find(|c| c.tag_name().name() == "lvl1pPr").and_then(|l| l.descendants().find(|c| c.tag_name().name() == "defRPr")).and_then(|r| num(&r, "sz")).map(|v| v / 100.0);
    let layout = pkg.rels(part).into_values().find(|t| t.contains("slideLayouts/"))?;
    let master = pkg.rels(&layout).into_values().find(|t| t.contains("slideMasters/"))?;
    for p in [&layout, &master] {
        let Some(x) = pkg.text(p) else { continue };
        let Ok(d) = roxmltree::Document::parse(&x) else { continue };
        for sp in d.descendants().filter(|c| c.tag_name().name() == "sp") {
            let Some(q) = sp.descendants().find(|c| c.tag_name().name() == "ph") else { continue };
            let qt = attr(&q, "type").unwrap_or("body");
            let same = (idx.is_some() && attr(&q, "idx") == idx) || qt == ty || (ty == "ctrTitle" && qt == "title") || (ty == "subTitle" && qt == "body");
            if same {
                if let Some(v) = sp.descendants().find(|c| c.tag_name().name() == "lstStyle").and_then(lvl1) {
                    return Some(v);
                }
            }
        }
    }
    let x = pkg.text(&master)?;
    let d = roxmltree::Document::parse(&x).ok()?;
    let style = if matches!(ty, "title" | "ctrTitle") { "titleStyle" } else if matches!(ty, "body" | "subTitle" | "obj") { "bodyStyle" } else { "otherStyle" };
    d.descendants().find(|c| c.tag_name().name() == style).and_then(lvl1)
}

pub struct Opened {
    pub doc: Document,
    pub hybrid: bool,
    pub changed_elsewhere: bool,
}

fn read_zip(path: &std::path::Path) -> Result<Package, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut files = HashMap::new();
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| e.to_string())?;
        if e.is_dir() {
            continue;
        }
        let Ok(name) = e.name().map(|n| n.to_string()) else { continue };
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut e, &mut b).map_err(|e| e.to_string())?;
        files.insert(name, b);
    }
    let mut pkg = Package { files, def_sz: 18.0 };
    let def = pkg.text("ppt/presentation.xml").and_then(|x| {
        let d = roxmltree::Document::parse(&x).ok()?;
        let lvl = d.descendants().find(|n| n.tag_name().name() == "defaultTextStyle")?.descendants().find(|n| matches!(n.tag_name().name(), "lvl1pPr" | "defPPr"))?;
        lvl.descendants().find(|n| n.tag_name().name() == "defRPr").and_then(|r| num(&r, "sz")).map(|v| v / 100.0)
    });
    if let Some(v) = def {
        pkg.def_sz = v;
    }
    Ok(pkg)
}

/// Open a .pptx: its Keydeck copy when it has a current one, else the slides.
pub fn open(path: &std::path::Path, theme: kn_format::KeyFile) -> Result<Opened, String> {
    let pkg = read_zip(path)?;
    let copy = pkg.files.get(KEY_PART).cloned();
    if let Some(bytes) = &copy {
        let now = slides_print(pkg.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())));
        let saved = pkg.text(PRINT_PART).map(|s| s.trim().to_string());
        if saved.as_deref() == Some(now.as_str()) {
            if let Ok(k) = kn_format::KeyFile::from_zip(bytes) {
                return Ok(Opened { doc: Document::from_file(k), hybrid: true, changed_elsewhere: false });
            }
        }
    }
    let doc = build(&pkg, theme)?;
    Ok(Opened { doc, hybrid: copy.is_some(), changed_elsewhere: copy.is_some() })
}

/// The slides of the presentation built into a Keynote document (from `theme`).
fn build(pkg: &Package, theme: kn_format::KeyFile) -> Result<Document, String> {
    let pres = pkg.text("ppt/presentation.xml").ok_or("not a PowerPoint presentation")?;
    let pd = roxmltree::Document::parse(&pres).map_err(|e| e.to_string())?;
    let prels = pkg.rels("ppt/presentation.xml");
    let size = pd.descendants().find(|n| n.tag_name().name() == "sldSz").map(|s| (pt(num(&s, "cx")), pt(num(&s, "cy")))).unwrap_or((960.0, 540.0));
    let slide_parts: Vec<String> = pd.descendants().filter(|n| n.tag_name().name() == "sldId").filter_map(|s| s.attributes().find(|a| a.name() == "id" && a.namespace().is_some()).and_then(|a| prels.get(a.value())).cloned()).collect();
    let mut doc = Document::new_from_theme(theme).map_err(|e| e.to_string())?;
    let mut h = History::default();
    doc.begin(&mut h, "Import");
    doc.set_slide_size(&mut h, size.0, size.1);
    let blank = doc.layouts().into_iter().find(|(_, n)| n.eq_ignore_ascii_case("blank")).or_else(|| doc.layouts().into_iter().last()).map(|l| l.0);
    let existing = doc.slides().len();
    for (si, part) in slide_parts.iter().enumerate() {
        let Some(x) = pkg.text(part) else { continue };
        let Ok(d) = roxmltree::Document::parse(&x) else { continue };
        let at = existing + si;
        let index = match blank {
            Some(l) => doc.new_slide_from_layout(&mut h, l, at).map_err(|e| e.to_string())?,
            None => doc.new_slide_after(&mut h, at.saturating_sub(1)).map_err(|e| e.to_string())?,
        };
        let slides = doc.slides();
        let Some(info) = slides.get(index) else { continue };
        let (slide, node) = (info.slide, info.node);
        // the layout's placeholders that came with the slide go: the .pptx says what's there
        if let Some(sc) = doc.scene(&slides, index) {
            let own: Vec<ObjId> = sc.drawables.iter().filter(|d| !d.from_template).map(|d| d.id).collect();
            if !own.is_empty() {
                doc.delete_drawables(&mut h, slide, &own);
            }
        }
        let master = pkg.rels(part).into_values().find(|t| t.contains("slideLayouts/")).and_then(|l| pkg.rels(&l).into_values().find(|t| t.contains("slideMasters/"))).unwrap_or_default();
        let theme = theme_colors(pkg, &master);
        let rels = pkg.rels(part);
        let root = d.root_element();
        if attr(&root, "show") == Some("0") {
            doc.set_skipped(&mut h, node, true);
        }
        // background
        if let Some(bgp) = root.descendants().find(|n| n.tag_name().name() == "bgPr") {
            if let Some(f) = child(&bgp, "solidFill").and_then(|s| color_of(s, &theme)) {
                doc.set_slide_background(&mut h, slide, &kn_model::format::FillSpec::Color(f));
            } else if let Some(bytes) = child(&bgp, "blipFill").and_then(|b| b.descendants().find(|n| n.tag_name().name() == "blip")).and_then(|b| b.attributes().find(|a| a.name() == "embed")).and_then(|a| rels.get(a.value())).and_then(|t| pkg.files.get(t)).and_then(|b| keepable(b)) {
                if let Ok(id) = doc.file.add_image_data(bytes.0, &format!("background.{}", bytes.1)) {
                    doc.set_slide_background(&mut h, slide, &kn_model::format::FillSpec::Image { data_id: id, technique: kn_model::ImageFillTechnique::ScaleToFill, size: Some(size), reference: None, tint: None });
                }
            }
        }
        // objects, back to front
        let Some(tree) = root.descendants().find(|n| n.tag_name().name() == "spTree") else { continue };
        let mut objs = Vec::new();
        collect(tree, &|x, y, w, h| (x, y, w, h), &mut objs);
        for o in objs {
            let mut g = o.g;
            if g.w <= 0.0 && g.h <= 0.0 {
                match placeholder_frame(pkg, part, o.node) {
                    Some(p) => {
                        g.x = p.x;
                        g.y = p.y;
                        g.w = p.w;
                        g.h = p.h;
                    }
                    None => continue,
                }
            }
            let size = placeholder_size(pkg, part, o.node).unwrap_or(pkg.def_sz);
            add_object(&mut doc, &mut h, slide, o.node, g, &theme, &rels, pkg, size);
        }
        // notes
        if let Some(np) = rels.values().find(|t| t.contains("notesSlides/")) {
            if let Some(nx) = pkg.text(np) {
                if let Ok(nd) = roxmltree::Document::parse(&nx) {
                    let body = nd.descendants().filter(|n| n.tag_name().name() == "sp").find(|sp| sp.descendants().any(|p| p.tag_name().name() == "ph" && attr(&p, "type").is_none_or(|t| t == "body")));
                    if let Some(b) = body {
                        let text: Vec<String> = b.descendants().filter(|n| n.tag_name().name() == "p").map(|p| p.descendants().filter(|t| t.tag_name().name() == "t").filter_map(|t| t.text()).collect::<String>()).collect();
                        let t = text.join("\n").trim().to_string();
                        if !t.is_empty() {
                            doc.set_notes(&mut h, slide, &t);
                        }
                    }
                }
            }
        }
    }
    // the theme's own slides go
    if existing > 0 && doc.slides().len() > existing {
        let _ = doc.delete_slides(&mut h, &(0..existing).collect::<Vec<_>>());
    }
    doc.end(&mut h);
    doc.path = None;
    doc.dirty = false;
    Ok(doc)
}

/// One slide object made in the document.
#[allow(clippy::too_many_arguments)]
fn add_object(doc: &mut Document, h: &mut History, slide: ObjId, n: Node, g: kn_model::Geometry, theme: &HashMap<String, Color>, rels: &HashMap<String, String>, pkg: &Package, size: f32) {
    use kn_model::insert::{BasicShape, NewObject};
    let name = n.tag_name().name();
    let sppr = child(&n, "spPr");
    match name {
        "pic" => {
            let Some((bytes, ext)) = n.descendants().find(|c| c.tag_name().name() == "blip").and_then(|b| b.attributes().find(|a| a.name() == "embed")).and_then(|a| rels.get(a.value())).and_then(|t| pkg.files.get(t)).and_then(|b| keepable(b)) else { return };
            if let Ok(id) = doc.insert(h, slide, &NewObject::Image(bytes, format!("picture.{ext}"))) {
                let mut g2 = doc.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
                (g2.x, g2.y, g2.w, g2.h, g2.angle, g2.hflip, g2.vflip) = (g.x, g.y, g.w, g.h, g.angle, g.hflip, g.vflip);
                doc.set_frame(h, id, g2);
                read_effects(doc, h, id, sppr, theme);
            }
        }
        "graphicFrame" => {
            if let Some(tbl) = n.descendants().find(|c| c.tag_name().name() == "tbl") {
                add_table(doc, h, slide, tbl, g, theme, size);
            }
        }
        "sp" | "cxnSp" => {
            let prst = sppr.and_then(|p| child(&p, "prstGeom")).and_then(|p| attr(&p, "prst")).map(str::to_string);
            let cust = sppr.and_then(|p| child(&p, "custGeom"));
            let txbody = child(&n, "txBody");
            let has_text = txbody.is_some_and(|t| t.descendants().any(|e| e.tag_name().name() == "t" && e.text().is_some_and(|s| !s.is_empty())));
            let is_line = name == "cxnSp" || matches!(prst.as_deref(), Some("line" | "straightConnector1" | "bentConnector3" | "curvedConnector3"));
            let fill = sppr.and_then(|p| {
                if child(&p, "noFill").is_some() {
                    Some(None)
                } else {
                    child(&p, "solidFill").map(|s| color_of(s, theme))
                }
            });
            let ln = sppr.and_then(|p| child(&p, "ln"));
            let stroke = ln.and_then(|l| {
                if child(&l, "noFill").is_some() {
                    return Some(None);
                }
                let c = child(&l, "solidFill").and_then(|s| color_of(s, theme))?;
                Some(Some((c, pt(num(&l, "w")).max(0.25))))
            });
            // a style reference without own colours: the theme's (accent 1 fill, a darker line)
            let styled = child(&n, "style").is_some();
            let plain_text_box = has_text && !is_line && fill.is_none_or(|f| f.is_none()) && stroke.is_none_or(|s| s.is_none()) && !styled && matches!(prst.as_deref(), None | Some("rect"));
            let id = if is_line {
                let id = doc.insert(h, slide, &NewObject::Line { arrow: ln.is_some_and(|l| child(&l, "tailEnd").is_some_and(|t| attr(&t, "type").is_some_and(|v| v != "none"))) }).ok();
                if let Some(id) = id {
                    let (x0, x1) = if g.hflip { (g.x + g.w, g.x) } else { (g.x, g.x + g.w) };
                    let (y0, y1) = if g.vflip { (g.y + g.h, g.y) } else { (g.y, g.y + g.h) };
                    doc.set_line_points(h, id, kn_model::Point { x: x0, y: y0 }, kn_model::Point { x: x1, y: y1 });
                    let (c, w) = stroke.flatten().unwrap_or((theme.get("accent1").copied().unwrap_or(Color::BLACK), 1.0));
                    doc.set_stroke(h, id, Some(&kn_model::format::StrokeSpec { color: c, width: w, dash: kn_model::format::Dash::Solid }));
                }
                id
            } else {
                let what = if plain_text_box {
                    NewObject::TextBox
                } else {
                    NewObject::Shape(match prst.as_deref() {
                        Some("roundRect") => BasicShape::RoundedRectangle,
                        Some("ellipse") => BasicShape::Oval,
                        Some("triangle") => BasicShape::Triangle,
                        Some("rtTriangle") => BasicShape::RightTriangle,
                        Some("diamond") => BasicShape::Diamond,
                        Some("rightArrow") => BasicShape::Arrow,
                        Some("leftRightArrow") => BasicShape::DoubleArrow,
                        Some("star5") => BasicShape::Star,
                        Some("pentagon") | Some("homePlate") => BasicShape::Pentagon,
                        Some("wedgeRectCallout") | Some("wedgeRoundRectCallout") => BasicShape::Callout,
                        Some("wedgeEllipseCallout") | Some("cloudCallout") => BasicShape::QuoteBubble,
                        _ => BasicShape::Rectangle,
                    })
                };
                let id = doc.insert(h, slide, &what).ok();
                if let Some(id) = id {
                    let mut g2 = doc.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
                    (g2.x, g2.y, g2.w, g2.h, g2.angle, g2.hflip, g2.vflip) = (g.x, g.y, g.w.max(1.0), g.h.max(1.0), g.angle, g.hflip, g.vflip);
                    g2.auto_height = false;
                    // PowerPoint's "do not wrap" box grows with its text, as Keydeck's auto-width one
                    g2.auto_width = plain_text_box && txbody.and_then(|t| child(&t, "bodyPr")).is_some_and(|b| attr(&b, "wrap") == Some("none"));
                    doc.set_frame(h, id, g2);
                    if let (Some(c), None) = (cust, &prst) {
                        if let Some(path) = cust_path(c, g.w, g.h) {
                            doc.make_editable(h, id, &path);
                        }
                    }
                    if !plain_text_box {
                        let accent = theme.get("accent1").copied().unwrap_or(Color { r: 0.27, g: 0.45, b: 0.77, a: 1.0 });
                        let f = match fill {
                            Some(Some(c)) => kn_model::format::FillSpec::Color(c),
                            Some(None) => kn_model::format::FillSpec::None,
                            None if styled => kn_model::format::FillSpec::Color(accent),
                            None => kn_model::format::FillSpec::None,
                        };
                        doc.set_fill(h, id, &f);
                        let s = match stroke {
                            Some(s) => s,
                            None if styled => Some((Color { r: accent.r * 0.7, g: accent.g * 0.7, b: accent.b * 0.7, a: 1.0 }, 1.0)),
                            None => None,
                        };
                        let spec = s.map(|(c, w)| kn_model::format::StrokeSpec { color: c, width: w, dash: kn_model::format::Dash::Solid });
                        doc.set_stroke(h, id, spec.as_ref());
                    }
                    read_effects(doc, h, id, sppr, theme);
                }
                id
            };
            if let (Some(id), Some(tb)) = (id, txbody.filter(|_| has_text)) {
                fill_text(doc, h, id, tb, theme, size);
            }
        }
        _ => {}
    }
}

/// effectLst: an outer shadow and a reflection.
fn read_effects(doc: &mut Document, h: &mut History, id: ObjId, sppr: Option<Node>, theme: &HashMap<String, Color>) {
    let Some(el) = sppr.and_then(|p| child(&p, "effectLst")) else { return };
    if let Some(sh) = child(&el, "outerShdw") {
        if let Some(c) = color_of(sh, theme) {
            let shadow = kn_model::Shadow {
                color: Color { a: 1.0, ..c },
                angle: (360.0 - num(&sh, "dir").unwrap_or(2700000.0) / 60000.0).rem_euclid(360.0),
                offset: pt(num(&sh, "dist")),
                blur: pt(num(&sh, "blurRad")),
                opacity: c.a,
                kind: 0,
                height: 0.2,
                curve: 0.6,
            };
            doc.set_shadow_spec(h, id, Some(&shadow));
        }
    }
    if let Some(r) = child(&el, "reflection") {
        doc.set_reflection_amount(h, id, num(&r, "stA").unwrap_or(50000.0) / 100000.0);
    }
}

fn cust_path(cg: Node, w: f32, h: f32) -> Option<kn_model::Path> {
    let mut path = kn_model::Path { els: Vec::new() };
    for pp in cg.descendants().filter(|n| n.tag_name().name() == "path") {
        let pw = num(&pp, "w").filter(|v| *v > 0.0).unwrap_or(w * 12700.0);
        let ph = num(&pp, "h").filter(|v| *v > 0.0).unwrap_or(h * 12700.0);
        let pts = |n: Node| -> Vec<kn_model::Point> { n.children().filter(|c| c.tag_name().name() == "pt").map(|c| kn_model::Point { x: num(&c, "x").unwrap_or(0.0) / pw * w, y: num(&c, "y").unwrap_or(0.0) / ph * h }).collect() };
        for e in pp.children().filter(|c| c.is_element()) {
            let v = pts(e);
            match (e.tag_name().name(), v.as_slice()) {
                ("moveTo", [a]) => path.els.push(PathEl::Move(*a)),
                ("lnTo", [a]) => path.els.push(PathEl::Line(*a)),
                ("quadBezTo", [c, a]) => path.els.push(PathEl::Quad(*c, *a)),
                ("cubicBezTo", [a, b, c]) => path.els.push(PathEl::Cubic(*a, *b, *c)),
                ("close", _) => path.els.push(PathEl::Close),
                _ => {}
            }
        }
    }
    (!path.els.is_empty()).then_some(path)
}

/// A shape's text with its paragraphs and runs.
fn fill_text(doc: &mut Document, h: &mut History, id: ObjId, tb: Node, theme: &HashMap<String, Color>, size: f32) {
    use kn_model::format::TextProp;
    let Some(st) = doc.storage_of(id) else { return };
    let mut text = String::new();
    let mut runs: Vec<(usize, usize, Node)> = Vec::new();
    let mut paras: Vec<(usize, usize, Node)> = Vec::new();
    let ps: Vec<Node> = tb.children().filter(|p| p.tag_name().name() == "p").collect();
    for (i, p) in ps.iter().enumerate() {
        let start = text.len();
        for r in p.children() {
            match r.tag_name().name() {
                "r" | "fld" => {
                    let t = r.descendants().find(|e| e.tag_name().name() == "t").and_then(|e| e.text()).unwrap_or("");
                    let a = text.len();
                    text.push_str(t);
                    runs.push((a, text.len(), r));
                }
                "br" => text.push('\u{2028}'),
                _ => {}
            }
        }
        paras.push((start, text.len(), *p));
        if i + 1 < ps.len() {
            text.push('\n');
        }
    }
    let bulleted = |p: &Node| child(p, "pPr").is_some_and(|q| child(&q, "buChar").is_some() || child(&q, "buAutoNum").is_some());
    doc.set_storage_text(h, st, &text);
    // the inherited size first; runs with their own override it
    if !text.is_empty() {
        doc.set_text_prop_range(h, st, 0, text.len(), &TextProp::Size(size));
    }
    if let Some(bp) = child(&tb, "bodyPr") {
        if let Some(a) = attr(&bp, "anchor") {
            let v = match a {
                "ctr" => kn_model::VAlign::Middle,
                "b" => kn_model::VAlign::Bottom,
                _ => kn_model::VAlign::Top,
            };
            doc.set_text_prop(h, id, &TextProp::VAlign(v));
        }
        if let Some(l) = num(&bp, "lIns") {
            doc.set_text_layout(h, id, kn_model::format::TextLayoutChange::Inset(l / 12700.0));
        }
        if child(&bp, "normAutofit").is_some() {
            doc.set_text_layout(h, id, kn_model::format::TextLayoutChange::ShrinkToFit(true));
        }
    }
    // bullets: Keydeck's list style is per text box, so when every paragraph with text has one
    let with_text: Vec<&(usize, usize, Node)> = paras.iter().filter(|(a, b, _)| b > a).collect();
    let all_bullets = !with_text.is_empty() && with_text.iter().all(|(_, _, p)| bulleted(p));
    if all_bullets {
        doc.set_text_prop(h, id, &TextProp::Bullets(true));
        for (a, b, p) in &with_text {
            let lvl = child(p, "pPr").and_then(|q| num(&q, "lvl")).unwrap_or(0.0) as i32;
            if lvl > 0 {
                doc.change_list_level(h, st, *a, *b, lvl);
            }
        }
    }
    for (a, b, p) in &paras {
        let Some(ppr) = child(p, "pPr") else { continue };
        if let Some(al) = attr(&ppr, "algn") {
            let v = match al {
                "ctr" => kn_model::Align::Center,
                "r" => kn_model::Align::Right,
                "just" | "dist" => kn_model::Align::Justify,
                _ => kn_model::Align::Left,
            };
            doc.set_para_prop_range(h, st, *a, (*b).max(*a), &TextProp::Align(v));
        }
        if let Some(sp) = child(&ppr, "lnSpc").and_then(|l| child(&l, "spcPct")).and_then(|s| num(&s, "val")).filter(|v| (*v - 100000.0).abs() > 1.0) {
            doc.set_para_prop_range(h, st, *a, (*b).max(*a), &TextProp::LineSpacing(sp / 100000.0));
        }
        let pts = |tag: &str| child(&ppr, tag).and_then(|l| child(&l, "spcPts")).and_then(|s| num(&s, "val")).map(|v| v / 100.0);
        if let Some(v) = pts("spcBef").filter(|v| *v > 0.0) {
            doc.set_para_prop_range(h, st, *a, (*b).max(*a), &TextProp::SpaceBefore(v));
        }
        if let Some(v) = pts("spcAft").filter(|v| *v > 0.0) {
            doc.set_para_prop_range(h, st, *a, (*b).max(*a), &TextProp::SpaceAfter(v));
        }
        // indents (a bulleted paragraph's come with its list style)
        if !bulleted(p) {
            let left = pt(num(&ppr, "marL"));
            let first = left + pt(num(&ppr, "indent"));
            if left > 0.0 {
                doc.set_para_prop_range(h, st, *a, (*b).max(*a), &TextProp::LeftIndent(left));
            }
            if first.abs() > 0.01 && (first - left).abs() > 0.01 {
                doc.set_para_prop_range(h, st, *a, (*b).max(*a), &TextProp::FirstIndent(first.max(0.0)));
            }
        }
    }
    for (a, b, r) in runs {
        if b <= a {
            continue;
        }
        let Some(rpr) = child(&r, "rPr") else { continue };
        if let Some(sz) = num(&rpr, "sz") {
            doc.set_text_prop_range(h, st, a, b, &TextProp::Size(sz / 100.0));
        }
        if attr(&rpr, "b") == Some("1") {
            doc.set_text_prop_range(h, st, a, b, &TextProp::Bold(true));
        }
        if attr(&rpr, "i") == Some("1") {
            doc.set_text_prop_range(h, st, a, b, &TextProp::Italic(true));
        }
        if attr(&rpr, "u").is_some_and(|u| u != "none") {
            doc.set_text_prop_range(h, st, a, b, &TextProp::Underline(true));
        }
        if attr(&rpr, "strike").is_some_and(|s| s != "noStrike") {
            doc.set_text_prop_range(h, st, a, b, &TextProp::Strike(true));
        }
        if let Some(c) = child(&rpr, "solidFill").and_then(|s| color_of(s, theme)) {
            doc.set_text_prop_range(h, st, a, b, &TextProp::Color(c));
        }
        if let Some(f) = child(&rpr, "latin").and_then(|l| attr(&l, "typeface")).filter(|f| !f.starts_with('+')) {
            doc.set_text_prop_range(h, st, a, b, &TextProp::Font(f.replace(' ', "")));
        }
        if let Some(bl) = num(&rpr, "baseline").filter(|v| v.abs() > 1.0) {
            doc.set_text_prop_range(h, st, a, b, &TextProp::Script(if bl > 0.0 { 1 } else { -1 }));
        }
    }
}

fn add_table(doc: &mut Document, h: &mut History, slide: ObjId, tbl: Node, g: kn_model::Geometry, theme: &HashMap<String, Color>, size: f32) {
    use kn_model::tablecells::CellInput;
    use kn_model::tablerows::Axis;
    let cols: Vec<f32> = tbl.descendants().filter(|n| n.tag_name().name() == "gridCol").map(|c| pt(num(&c, "w")).max(12.0)).collect();
    let rows: Vec<Node> = tbl.children().filter(|n| n.tag_name().name() == "tr").collect();
    if cols.is_empty() || rows.is_empty() {
        return;
    }
    let Ok(id) = doc.insert(h, slide, &kn_model::insert::NewObject::Table { rows: 5, cols: 4 }) else { return };
    for which in [kn_model::tablerows::HeaderKind::Rows, kn_model::tablerows::HeaderKind::Columns, kn_model::tablerows::HeaderKind::Footer] {
        let _ = doc.set_table_header_count(h, id, which, 0);
    }
    let (mut r0, mut c0) = (5u32, 4u32);
    let (nr, nc) = (rows.len() as u32, cols.len() as u32);
    while r0 < nr && doc.table_insert(h, id, Axis::Row, r0, r0 - 1).is_ok() {
        r0 += 1;
    }
    while r0 > nr && doc.table_delete(h, id, Axis::Row, r0 - 1).is_ok() {
        r0 -= 1;
    }
    while c0 < nc && doc.table_insert(h, id, Axis::Column, c0, c0 - 1).is_ok() {
        c0 += 1;
    }
    while c0 > nc && doc.table_delete(h, id, Axis::Column, c0 - 1).is_ok() {
        c0 -= 1;
    }
    doc.set_table_sizes(h, id, false, &cols.iter().enumerate().map(|(i, w)| (i as u32, *w)).collect::<Vec<_>>());
    doc.set_table_sizes(h, id, true, &rows.iter().enumerate().map(|(i, r)| (i as u32, pt(num(r, "h")).max(14.0))).collect::<Vec<_>>());
    let _ = doc.set_cells_fill(h, id, (0, 0, r0 - 1, c0 - 1), None);
    let _ = doc.set_table_banded(h, id, false);
    let _ = doc.set_cells_text_prop(h, id, (0, 0, r0 - 1, c0 - 1), &kn_model::format::TextProp::Size(size));
    let mut merges = Vec::new();
    for (ri, tr) in rows.iter().enumerate() {
        for (ci, tc) in tr.children().filter(|n| n.tag_name().name() == "tc").enumerate() {
            let (r, c) = (ri as u32, ci as u32);
            if r >= r0 || c >= c0 {
                continue;
            }
            let gs = num(&tc, "gridSpan").unwrap_or(1.0) as u32;
            let rs = num(&tc, "rowSpan").unwrap_or(1.0) as u32;
            if gs > 1 || rs > 1 {
                merges.push(kn_model::CellRange { row: r, col: c, rows: rs.min(r0 - r), cols: gs.min(c0 - c) });
            }
            if attr(&tc, "hMerge") == Some("1") || attr(&tc, "vMerge") == Some("1") {
                continue;
            }
            let text: String = tc.descendants().filter(|n| n.tag_name().name() == "p").map(|p| p.descendants().filter(|t| t.tag_name().name() == "t").filter_map(|t| t.text()).collect::<String>()).collect::<Vec<_>>().join("\n");
            if !text.is_empty() {
                let _ = doc.set_cell(h, id, r, c, &CellInput::Text(text));
            }
            let tcpr = child(&tc, "tcPr");
            if let Some(f) = tcpr.and_then(|p| child(&p, "solidFill")).and_then(|s| color_of(s, theme)) {
                let _ = doc.set_cells_fill(h, id, (r, c, r, c), Some(&kn_model::format::FillSpec::Color(f)));
            }
            if let Some(rpr) = tc.descendants().find(|n| n.tag_name().name() == "rPr") {
                if let Some(sz) = num(&rpr, "sz") {
                    let _ = doc.set_cells_text_prop(h, id, (r, c, r, c), &kn_model::format::TextProp::Size(sz / 100.0));
                }
                if attr(&rpr, "b") == Some("1") {
                    let _ = doc.set_cells_text_prop(h, id, (r, c, r, c), &kn_model::format::TextProp::Bold(true));
                }
                if let Some(col) = child(&rpr, "solidFill").and_then(|s| color_of(s, theme)) {
                    let _ = doc.set_cells_text_prop(h, id, (r, c, r, c), &kn_model::format::TextProp::Color(col));
                }
            }
            for (tag, edge) in [("lnL", kn_model::tablefmt::BorderEdges::Left), ("lnR", kn_model::tablefmt::BorderEdges::Right), ("lnT", kn_model::tablefmt::BorderEdges::Top), ("lnB", kn_model::tablefmt::BorderEdges::Bottom)] {
                let Some(l) = tcpr.and_then(|p| child(&p, tag)) else { continue };
                let stroke = if child(&l, "noFill").is_some() {
                    None
                } else {
                    child(&l, "solidFill").and_then(|s| color_of(s, theme)).map(|c| kn_model::format::StrokeSpec { color: c, width: pt(num(&l, "w")).max(0.25), dash: kn_model::format::Dash::Solid })
                };
                let _ = doc.set_cells_border(h, id, (r, c, r, c), edge, stroke.as_ref());
            }
        }
    }
    for m in merges {
        let _ = doc.merge_cells(h, id, m);
    }
    let mut g2 = doc.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
    g2.x = g.x;
    g2.y = g.y;
    doc.set_frame(h, id, g2);
}

/// Keynote bytes of the document (for PowerPoint + Keydeck).
pub fn key_bytes(doc: &Document) -> Result<Vec<u8>, String> {
    doc.file.to_zip().map_err(|e| e.to_string())
}

/// Headless conversion between .key and .pptx (tests and the command line).
pub fn convert(input: &std::path::Path, output: &std::path::Path, hybrid: bool) -> Result<(), String> {
    let is_pptx = |p: &std::path::Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pptx"));
    let doc = if is_pptx(input) {
        let o = open(input, crate::themes::white().map_err(|e| e.to_string())?)?;
        println!("opened {} slides ({})", o.doc.slides().len(), if o.hybrid && !o.changed_elsewhere { "Keydeck copy" } else { "PowerPoint slides" });
        o.doc
    } else {
        Document::open(input).map_err(|e| e.to_string())?
    };
    if is_pptx(output) {
        let mut r = Renderer::new();
        let key = if hybrid { Some(key_bytes(&doc)?) } else { None };
        write(&doc, &mut r, output, key)?;
    } else {
        doc.file.save(output).map_err(|e| e.to_string())?;
    }
    println!("wrote {}", output.display());
    Ok(())
}

pub fn is_pptx(p: &std::path::Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pptx"))
}

/// Open a .pptx as a document (for the app's open thread).
pub fn open_document(p: &std::path::Path) -> Result<Document, kn_format::Error> {
    let theme = crate::themes::white()?;
    let mut o = open(p, theme).map_err(kn_format::Error::Invalid)?;
    o.doc.path = Some(p.to_path_buf());
    Ok(o.doc)
}

/// (PowerPoint + Keydeck, its Keydeck copy current) — None when it can't be read.
pub fn mode(p: &std::path::Path) -> Option<(bool, bool)> {
    let f = std::fs::File::open(p).ok()?;
    let mut z = zip::ZipArchive::new(f).ok()?;
    if z.by_name(KEY_PART).is_err() {
        return Some((false, false));
    }
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..z.len() {
        let Ok(mut e) = z.by_index(i) else { continue };
        let Ok(name) = e.name().map(|n| n.to_string()) else { continue };
        if (name.starts_with("ppt/slides/slide") && name.ends_with(".xml")) || name == PRINT_PART {
            let mut b = Vec::new();
            std::io::Read::read_to_end(&mut e, &mut b).ok()?;
            parts.push((name, b));
        }
    }
    let saved = parts.iter().find(|p| p.0 == PRINT_PART).map(|p| String::from_utf8_lossy(&p.1).trim().to_string());
    let now = slides_print(parts.iter().map(|p| (p.0.as_str(), p.1.as_slice())));
    Some((true, saved.as_deref() == Some(now.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn testdata(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(name)
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn powerpoint_round_trip_keeps_slides_and_objects() {
        let dir = std::env::temp_dir().join(format!("kn-pptx-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = Document::open(testdata("Keynote-Feature-Catalog.key")).unwrap();
        let mut r = Renderer::new();
        let out = dir.join("plain.pptx");
        write(&doc, &mut r, &out, None).unwrap();
        assert_eq!(mode(&out), Some((false, false)));
        let back = open(&out, crate::themes::white().unwrap()).unwrap();
        assert!(!back.hybrid);
        let (a, b) = (doc.slides(), back.doc.slides());
        assert_eq!(a.len(), b.len());
        // hidden slides stay hidden; every slide keeps its own objects and notes
        assert_eq!(a.iter().filter(|s| s.skipped).count(), b.iter().filter(|s| s.skipped).count());
        for i in 0..a.len() {
            let (sa, sb) = (doc.scene(&a, i).unwrap(), back.doc.scene(&b, i).unwrap());
            let own = |s: &kn_model::SlideScene| s.drawables.iter().filter(|d| !d.from_template).count();
            assert!(own(&sb) >= own(&sa).min(1), "slide {} lost its objects", i + 1);
            assert_eq!(doc.notes(a[i].slide).trim(), back.doc.notes(b[i].slide).trim(), "slide {} notes", i + 1);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn hybrid_uses_its_keynote_copy_until_the_slides_change() {
        let dir = std::env::temp_dir().join(format!("kn-pptx-hybrid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = Document::open(testdata("11-two-slides.key")).unwrap();
        let mut r = Renderer::new();
        let out = dir.join("hybrid.pptx");
        write(&doc, &mut r, &out, Some(key_bytes(&doc).unwrap())).unwrap();
        assert_eq!(mode(&out), Some((true, true)));
        let o = open(&out, crate::themes::white().unwrap()).unwrap();
        assert!(o.hybrid && !o.changed_elsewhere);
        // another app edits a slide: the slides win
        let mut pkg = read_zip(&out).unwrap();
        let s1 = pkg.files.get_mut("ppt/slides/slide1.xml").unwrap();
        *s1 = String::from_utf8_lossy(s1).replace("</p:spTree>", "<!-- edited --></p:spTree>").into_bytes();
        let edited = dir.join("edited.pptx");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&edited).unwrap());
            for (n, b) in &pkg.files {
                z.start_file(n.as_str(), zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(b).unwrap();
            }
            z.finish().unwrap();
        }
        assert_eq!(mode(&edited), Some((true, false)));
        let o = open(&edited, crate::themes::white().unwrap()).unwrap();
        assert!(o.hybrid && o.changed_elsewhere);
        assert_eq!(o.doc.slides().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
