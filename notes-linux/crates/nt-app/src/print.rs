//! File > Export To > PDF and File > Print…: a note laid out on letter pages with the suite's
//! text renderer (kn-render), its pictures between the paragraphs, written as a PDF of page
//! pictures (Letters' way: each page drawn at 200 dpi, as JPEG). Print hands that PDF to the
//! system's printing (CUPS' lp), with a sheet to choose the printer and copies.

use std::path::{Path, PathBuf};

use kn_model::{Align, CharStyle, Color, ListLabel, Run, VAlign};
use kn_render::tiny_skia as sk;
use nt_core::model::{AttachmentKind, Note, ParaStyle};

/// US Letter, in points, and its margins.
const PAGE_W: f32 = 612.0;
const PAGE_H: f32 = 792.0;
const MARGIN: f32 = 54.0;
/// Pixels per point the pages are drawn at (200 dpi).
const SCALE: f32 = 200.0 / 72.0;

/// What goes down the page: a paragraph's text, or a picture.
enum Block {
    Text(kn_render::text::TextLayout, f32),
    Picture(sk::Pixmap, f32, f32),
}

/// The note's pages, drawn (Letter, portrait).
pub fn pages(note: &Note) -> Vec<sk::Pixmap> {
    pages_on(note, PAGE_W, PAGE_H)
}

/// How a PDF is laid out (File > Export…'s PDF card): the page turned, and the note's collapsed
/// sections left out or not.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Layout {
    pub landscape: bool,
    pub sections: nt_core::export::Sections,
}

impl Layout {
    fn page(&self) -> (f32, f32) {
        if self.landscape { (PAGE_H, PAGE_W) } else { (PAGE_W, PAGE_H) }
    }
}

/// The note's pages on a page `page_w` × `page_h` pt.
fn pages_on(note: &Note, page_w: f32, page_h: f32) -> Vec<sk::Pixmap> {
    let mut r = kn_render::Renderer::new();
    let width = page_w - 2.0 * MARGIN;
    let blocks = blocks(note, &mut r, width);
    let (pw, ph) = ((page_w * SCALE) as u32, (page_h * SCALE) as u32);
    let new_page = || {
        let mut pm = sk::Pixmap::new(pw, ph).expect("page");
        pm.fill(sk::Color::WHITE);
        pm
    };
    let mut out = vec![new_page()];
    let (top, bottom) = (MARGIN, page_h - MARGIN);
    let mut y = top;
    for b in blocks {
        match b {
            Block::Text(tl, h) => {
                // a paragraph that doesn't fit goes on, line by line, onto the next page
                let mut start = 0.0;
                while start < h {
                    let room = bottom - y;
                    // the most of it, by whole lines, that fits here
                    let fit = tl.lines.iter().map(|l| l.top + l.height - start).filter(|e| *e > 0.0 && *e <= room).fold(0.0f32, f32::max);
                    if fit <= 0.0 {
                        if y > top {
                            out.push(new_page());
                            y = top;
                            continue;
                        }
                        // (a single line taller than the page: as much as there's room for)
                        let pm = out.last_mut().unwrap();
                        draw_text_part(&mut r, &tl, pm, MARGIN, y, start, room);
                        start += room;
                        continue;
                    }
                    let pm = out.last_mut().unwrap();
                    draw_text_part(&mut r, &tl, pm, MARGIN, y, start, fit);
                    let lines_end = tl.lines.last().map(|l| l.top + l.height).unwrap_or(h);
                    if start + fit >= lines_end - 0.5 {
                        // every line drawn: the space after it too, then the next block
                        y += h - start;
                        start = h;
                    } else {
                        start += fit;
                        out.push(new_page());
                        y = top;
                    }
                }
            }
            Block::Picture(img, w, h) => {
                if y + h > bottom && y > top {
                    out.push(new_page());
                    y = top;
                }
                let pm = out.last_mut().unwrap();
                let t = sk::Transform::from_scale(SCALE * w / img.width() as f32, SCALE * h / img.height() as f32).post_translate(MARGIN * SCALE, y * SCALE);
                pm.draw_pixmap(0, 0, img.as_ref(), &sk::PixmapPaint { quality: sk::FilterQuality::Bicubic, ..Default::default() }, t, None);
                y += h + 8.0;
            }
        }
    }
    out
}

/// Draws the part of a paragraph's layout from `start` to `start + h` (points) at (x, y).
fn draw_text_part(r: &mut kn_render::Renderer, tl: &kn_render::text::TextLayout, pm: &mut sk::Pixmap, x: f32, y: f32, start: f32, h: f32) {
    let mut part = sk::Pixmap::new(pm.width(), ((h + 2.0) * SCALE).ceil() as u32).expect("part");
    r.draw_text(tl, &mut part, sk::Transform::from_scale(SCALE, SCALE).post_translate(x * SCALE, -start * SCALE));
    pm.draw_pixmap(0, (y * SCALE) as i32, part.as_ref(), &sk::PixmapPaint::default(), sk::Transform::identity(), None);
}

/// The note as blocks: each paragraph's text laid out at `width`, each picture scaled to fit.
fn blocks(note: &Note, r: &mut kn_render::Renderer, width: f32) -> Vec<Block> {
    let mut out = Vec::new();
    let mut numbers: Vec<u32> = Vec::new();
    for p in &note.body.paragraphs {
        // pictures in the paragraph go after its text
        let mut pictures = Vec::new();
        let mut text = String::new();
        let mut runs: Vec<Run> = Vec::new();
        let base = style_of(p.style);
        // the list's label (bullet, dash, number, checkbox), as Notes shows it
        // (lists hang: wrapped lines line up with the text after the label)
        let indent = 24.0 * p.indent as f32;
        if p.style == ParaStyle::Numbered {
            numbers.resize(p.indent as usize + 1, 0);
            numbers[p.indent as usize] += 1;
        } else {
            numbers.clear();
        }
        let label = match p.style {
            ParaStyle::Bulleted => "•  ".to_string(),
            ParaStyle::Dashed => "–  ".to_string(),
            ParaStyle::Numbered => format!("{}.  ", numbers.last().copied().unwrap_or(1)),
            ParaStyle::Checklist => if p.checked { "☑  " } else { "☐  " }.to_string(),
            _ => String::new(),
        };

        for s in &p.spans {
            if let Some(tok) = &s.token {
                if tok.is_inline() {
                    // tags, links to notes, dividers, math: the text Notes shows for them
                    let shown = tok.alt.clone();
                    if !shown.is_empty() {
                        let start = text.len();
                        text.push_str(&shown);
                        runs.push(Run { start, end: text.len(), style: CharStyle { color: Color::rgba(0.85, 0.6, 0.0, 1.0), ..base.clone() } });
                    }
                } else if let Some(a) = note.attachment(&tok.id) {
                    if let Some(pic) = picture(a, width) {
                        pictures.push(pic);
                    } else {
                        // files, audio, links: their name
                        let start = text.len();
                        let name = if a.name.is_empty() { a.url.clone().unwrap_or_else(|| "Attachment".into()) } else { a.name.clone() };
                        text.push_str(&format!("[{name}]"));
                        runs.push(Run { start, end: text.len(), style: CharStyle { italic: true, ..base.clone() } });
                    }
                }
                continue;
            }
            let start = text.len();
            text.push_str(&s.text);
            let mut st = base.clone();
            st.bold |= s.bold;
            st.italic |= s.italic;
            st.underline |= s.underline || s.link.is_some();
            st.strike |= s.strike;
            st.script = s.script;
            if let Some([r, g, b, a]) = s.color {
                st.color = Color::rgba(r, g, b, a);
            }
            if s.link.is_some() {
                st.color = Color::rgba(0.85, 0.6, 0.0, 1.0);
            }
            runs.push(Run { start, end: text.len(), style: st });
        }
        if !text.trim().is_empty() || pictures.is_empty() {
            let shown = if text.is_empty() { " ".to_string() } else { text };
            let mut tb = kn_render::simple_text_box(&shown, base.clone(), Align::Left, VAlign::Top);
            tb.padding = [0.0, 0.0, 0.0, 0.0];
            if let Some(para) = tb.paragraphs.first_mut() {
                para.runs = if runs.is_empty() { vec![Run { start: 0, end: shown.len(), style: base.clone() }] } else { runs };
                para.space_after = space_after(p.style);
                // (a list's label hangs: every line of its text lines up after it)
                para.list_indent = indent;
                if !label.trim().is_empty() {
                    para.label = Some(ListLabel::Bullet(label.trim().to_string()));
                    para.label_indent = 1.5;
                    para.label_scale = 1.0;
                } else if indent > 0.0 {
                    para.left_indent = indent;
                }
            }
            let tl = kn_render::text::layout(&mut r.fonts, &tb, width, 10_000.0, false, true);
            let h = tl.lines.last().map(|l| l.top + l.height).unwrap_or(base.size * 1.3) + space_after(p.style);
            out.push(Block::Text(tl, h));
        }
        for (img, w, h) in pictures {
            out.push(Block::Picture(img, w, h));
        }
    }
    out
}

/// A paragraph style's text (Notes' sizes on paper: Title 24, Heading 18, Subheading 15, Body 12).
fn style_of(s: ParaStyle) -> CharStyle {
    let (font, size, bold) = match s {
        ParaStyle::Title => ("SFProDisplay-Bold", 24.0, true),
        ParaStyle::Heading => ("SFProDisplay-Bold", 18.0, true),
        ParaStyle::Subheading => ("SFProText-Semibold", 15.0, true),
        ParaStyle::Monostyled => ("Menlo-Regular", 11.0, false),
        _ => ("SFProText-Regular", 12.0, false),
    };
    CharStyle { font_name: font.into(), size, bold, color: Color::BLACK, ..CharStyle::default() }
}

fn space_after(s: ParaStyle) -> f32 {
    match s {
        ParaStyle::Title => 8.0,
        ParaStyle::Heading | ParaStyle::Subheading => 5.0,
        _ => 3.0,
    }
}

/// An attachment's picture, scaled to the page's width (at most 4 inches tall): images, scans
/// (their pages), drawings (their fallback image), PDFs (their preview).
fn picture(a: &nt_core::model::Attachment, width: f32) -> Option<(sk::Pixmap, f32, f32)> {
    let path: Option<PathBuf> = match a.kind {
        AttachmentKind::Image => a.file.clone().or(a.preview.clone()),
        AttachmentKind::Scan => a.parts.first().and_then(|p| p.display_file().cloned()).or(a.preview.clone()),
        AttachmentKind::Drawing => a.fallback.clone().filter(|p| p.extension().is_some_and(|e| e == "png" || e == "jpg" || e == "jpeg")).or(a.preview.clone()),
        AttachmentKind::Pdf => a.preview.clone(),
        _ => None,
    };
    let img = image::open(path?).ok()?.into_rgba8();
    let (iw, ih) = (img.width() as f32, img.height() as f32);
    if iw < 1.0 || ih < 1.0 {
        return None;
    }
    let mut w = width.min(iw / 2.0).max(40.0);
    let mut h = w * ih / iw;
    if h > 288.0 {
        h = 288.0;
        w = h * iw / ih;
    }
    let mut pm = sk::Pixmap::new(img.width(), img.height())?;
    for (px, c) in pm.pixels_mut().iter_mut().zip(img.pixels()) {
        let [r, g, b, al] = c.0;
        *px = sk::ColorU8::from_rgba(r, g, b, al).premultiply();
    }
    Some((pm, w, h))
}

/// Writes the note as a PDF of its pages; how many pages.
pub fn to_pdf(note: &Note, path: &Path) -> Result<usize, String> {
    to_pdf_with(note, path, Layout::default())
}

/// The note as a PDF laid out as `layout` asks (its pages: how many).
pub fn to_pdf_with(note: &Note, path: &Path, layout: Layout) -> Result<usize, String> {
    let (page_w, page_h) = layout.page();
    let shown;
    let note = if layout.sections == nt_core::export::Sections::AsShown {
        shown = Note { body: nt_core::export::shown_body(&note.body, layout.sections), ..note.clone() };
        &shown
    } else {
        note
    };
    let pages = pages_on(note, page_w, page_h);
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
        let rgb: Vec<u8> = pm
            .pixels()
            .iter()
            .flat_map(|c| {
                let c = c.demultiply();
                [c.red(), c.green(), c.blue()]
            })
            .collect();
        let mut jpg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90).encode(&rgb, pm.width(), pm.height(), image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
        obj(&mut out, &mut offsets, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_w} {page_h}] /Resources << /XObject << /Im0 {image_id} 0 R >> >> /Contents {content_id} 0 R >>").as_bytes());
        let content = format!("q {page_w} 0 0 {page_h} 0 0 cm /Im0 Do Q");
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

/// The printers CUPS knows (lpstat), the default first; empty without CUPS.
pub fn printers() -> Vec<String> {
    let run = |args: &[&str]| std::process::Command::new("lpstat").args(args).stdin(std::process::Stdio::null()).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    let mut out: Vec<String> = run(&["-e"]).map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default();
    if let Some(d) = run(&["-d"]).and_then(|s| s.rsplit(':').next().map(|x| x.trim().to_string())).filter(|d| !d.is_empty() && !d.contains(' ')) {
        out.retain(|p| *p != d);
        out.insert(0, d);
    }
    out
}

/// Prints a PDF on a printer (lp), `copies` times.
pub fn print(pdf: &Path, printer: &str, copies: u32) -> Result<(), String> {
    let out = std::process::Command::new("lp").args(["-d", printer, "-n", &copies.max(1).to_string()]).arg(pdf).stdin(std::process::Stdio::null()).output().map_err(|e| format!("lp: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nt_core::model::{Body, Paragraph};

    #[test]
    fn a_long_note_runs_over_pages() {
        let mut paras = vec![Paragraph::plain(ParaStyle::Title, "Printing")];
        for i in 0..120 {
            paras.push(Paragraph::plain(if i % 10 == 0 { ParaStyle::Heading } else { ParaStyle::Body }, &format!("Line {i} of a note that runs on over several pages when it's printed.")));
        }
        let note = Note { title: "Printing".into(), body: Body { paragraphs: paras, ..Default::default() }, ..Default::default() };
        let pages = pages(&note);
        assert!(pages.len() >= 3, "{} pages", pages.len());
        let dir = std::env::temp_dir().join(format!("nt-print-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("note.pdf");
        assert_eq!(to_pdf(&note, &p).unwrap(), pages.len());
        let bytes = std::fs::read(&p).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.4") && bytes.ends_with(b"%%EOF\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
