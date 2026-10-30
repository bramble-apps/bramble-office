//! PowerPoint export with editable text (as Keynote's): plain text boxes become PowerPoint text
//! boxes with their fonts, sizes, colours, alignment and spacing; everything else stays in the
//! slide picture underneath.

use kn_format::ObjId;
use kn_model::{Align, Caps, Color, Document, DrawableKind, LineSpacing, SlideScene, VAlign};
use kn_render::{tiny_skia as sk, Filter, Renderer};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn emu(pt: f32) -> i64 {
    (pt * 12700.0).round() as i64
}

fn hex(c: Color) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("{:02X}{:02X}{:02X}", b(c.r), b(c.g), b(c.b))
}

fn fill(c: Color) -> String {
    if c.a < 0.999 {
        format!(r#"<a:solidFill><a:srgbClr val="{}"><a:alpha val="{}"/></a:srgbClr></a:solidFill>"#, hex(c), (c.a.clamp(0.0, 1.0) * 100000.0).round() as i64)
    } else {
        format!(r#"<a:solidFill><a:srgbClr val="{}"/></a:solidFill>"#, hex(c))
    }
}

/// The slide's text boxes that become PowerPoint text (unrotated, no fill or border, on the
/// slide itself), as `<p:sp>` XML with ids from 10, and their object ids.
pub fn text_boxes(r: &mut Renderer, scene: &SlideScene) -> (Vec<ObjId>, String) {
    let mut ids = Vec::new();
    let mut xml = String::new();
    let mut n = 10;
    for d in &scene.drawables {
        let DrawableKind::Shape(s) = &d.kind else { continue };
        let Some(tb) = &s.text else { continue };
        if d.from_template || s.is_line || tb.text.trim().is_empty() || !matches!(s.fill, kn_model::Fill::None) || s.stroke.is_some() {
            continue;
        }
        let a = d.geom.angle.rem_euclid(360.0);
        if a > 0.5 && a < 359.5 || d.opacity < 0.99 {
            continue;
        }
        let Some((tl, g)) = r.text_layout(d) else { continue };
        let [pl, pt, pr, pb] = tb.padding;
        let anchor = match tb.valign {
            VAlign::Top => "t",
            VAlign::Middle => "ctr",
            VAlign::Bottom => "b",
        };
        // shrunk to fit: say by how much, so PowerPoint starts from the same size
        let fit = if tb.shrink_to_fit && tl.scale < 0.999 {
            format!(r#"<a:normAutofit fontScale="{}"/>"#, (tl.scale * 100000.0).round() as i64)
        } else if tb.shrink_to_fit {
            "<a:normAutofit/>".to_string()
        } else {
            "<a:noAutofit/>".to_string()
        };
        let paras = paragraphs_xml(tb);
        xml.push_str(&format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{n}" name="Text {n}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="square" lIns="{}" tIns="{}" rIns="{}" bIns="{}" anchor="{anchor}">{fit}</a:bodyPr><a:lstStyle/>{paras}</p:txBody></p:sp>"#,
            emu(g.x),
            emu(g.y),
            emu(g.w.max(1.0)),
            emu(g.h.max(1.0)),
            emu(pl),
            emu(pt),
            emu(pr),
            emu(pb)
        ));
        n += 1;
        ids.push(d.id);
    }
    (ids, xml)
}

/// A text box's paragraphs as PowerPoint's (<a:p>…): alignment, spacing, bullets, runs with
/// their fonts, sizes, colours, styles and highlights.
pub fn paragraphs_xml(tb: &kn_model::TextBox) -> String {
let mut paras = String::new();
    for p in &tb.paragraphs {
        let algn = match p.align {
            Align::Center => "ctr",
            Align::Right => "r",
            Align::Justify => "just",
            _ => "l",
        };
        let spacing = match p.line_spacing {
            LineSpacing::Relative(x) => format!(r#"<a:lnSpc><a:spcPct val="{}"/></a:lnSpc>"#, (x * 100000.0).round() as i64),
            LineSpacing::AtLeast(x) | LineSpacing::Exactly(x) | LineSpacing::AtMost(x) => format!(r#"<a:lnSpc><a:spcPts val="{}"/></a:lnSpc>"#, (x * 100.0).round() as i64),
            _ => String::new(),
        };
        let bullet = match &p.label {
            Some(kn_model::ListLabel::Bullet(b)) => format!(r#"<a:buChar char="{}"/>"#, esc(b)),
            Some(kn_model::ListLabel::Number(_)) => r#"<a:buAutoNum type="arabicPeriod"/>"#.to_string(),
            _ => "<a:buNone/>".to_string(),
        };
        let indent = if p.label.is_some() { format!(r#" marL="{}" indent="{}""#, emu(p.left_indent.max(p.list_indent + 18.0)), -emu(18.0)) } else { format!(r#" marL="{}""#, emu(p.left_indent)) };
        paras.push_str(&format!(
            r#"<a:p><a:pPr algn="{algn}" lvl="{}"{indent}>{spacing}<a:spcBef><a:spcPts val="{}"/></a:spcBef><a:spcAft><a:spcPts val="{}"/></a:spcAft>{bullet}</a:pPr>"#,
            p.level.min(8),
            (p.space_before * 100.0).round() as i64,
            (p.space_after * 100.0).round() as i64
        ));
        let mut last_size = 18.0;
        for run in &p.runs {
            let text = tb.text.get(run.start..run.end).unwrap_or("").replace(['\u{2028}', '\u{2029}', '\n'], "\u{000B}");
            if text.is_empty() {
                continue;
            }
            let st = &run.style;
            last_size = st.size;
            let (family, weight, italic) = kn_render::fonts::parse_ps_name(&st.font_name);
            let b = st.bold || weight >= 600;
            let i = st.italic || italic;
            let cap = match st.caps {
                Caps::All => r#" cap="all""#,
                Caps::Small => r#" cap="small""#,
                _ => "",
            };
            let mut attrs = format!(r#" lang="en-US" sz="{}" b="{}" i="{}"{cap}"#, (st.size * 100.0).round() as i64, u8::from(b), u8::from(i));
            if st.underline {
                attrs.push_str(r#" u="sng""#);
            }
            if st.strike {
                attrs.push_str(r#" strike="sngStrike""#);
            }
            if st.tracking.abs() > 1e-4 {
                attrs.push_str(&format!(r#" spc="{}""#, (st.tracking * st.size * 100.0).round() as i64));
            }
            if st.script != 0 {
                attrs.push_str(if st.script > 0 { r#" baseline="30000""# } else { r#" baseline="-25000""# });
            }
            let hl = st.background.map(|c| format!(r#"<a:highlight><a:srgbClr val="{}"/></a:highlight>"#, hex(c))).unwrap_or_default();
            // a line break inside a paragraph is <a:br/> in PowerPoint
            for (k, piece) in text.split('\u{000B}').enumerate() {
                if k > 0 {
                    paras.push_str(&format!(r#"<a:br><a:rPr lang="en-US" sz="{}"/></a:br>"#, (st.size * 100.0).round() as i64));
                }
                if piece.is_empty() {
                    continue;
                }
                paras.push_str(&format!(
                    r#"<a:r><a:rPr{attrs}>{}{hl}<a:latin typeface="{}"/><a:cs typeface="{}"/></a:rPr><a:t>{}</a:t></a:r>"#,
                    fill(st.color),
                    esc(&family),
                    esc(&family),
                    esc(piece)
                ));
            }
        }
        paras.push_str(&format!(r#"<a:endParaRPr lang="en-US" sz="{}"/></a:p>"#, (last_size * 100.0).round() as i64));
    }
    if paras.is_empty() {
        paras.push_str("<a:p/>");
    }
    paras
}

/// The slide picture without the text boxes that are exported as text.
pub fn render_without(r: &mut Renderer, doc: &Document, scene: &SlideScene, width: u32, skip: &[ObjId]) -> Option<sk::Pixmap> {
    let scale = width as f32 / scene.size.w;
    let h = (scene.size.h * scale).round().max(1.0) as u32;
    let mut pm = sk::Pixmap::new(width.max(1), h)?;
    r.draw_scene(doc, scene, &mut pm, sk::Transform::from_scale(scale, scale), Filter::Except(skip), true);
    Some(pm)
}
