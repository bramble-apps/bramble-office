//! File > Export To > PDF: one page per sheet ("Single page" in Numbers' PDF export), the
//! sheet's content fitted to a Letter page in the sheet's orientation, drawn by Keydeck's
//! renderer and embedded as JPEG (as Keydeck's own PDF export does).

use kn_render::Renderer;
use nm_core::Workbook;
use tiny_skia as sk;

/// Device pixels per page point.
const DPI_SCALE: f32 = 2.5;

struct Page {
    w: f32,
    h: f32,
    px: (u32, u32),
    jpg: Vec<u8>,
}

/// A sheet's content box (sheet points), table titles included.
fn content_box(drawables: &[nm_core::kn_model::Drawable]) -> (f32, f32, f32, f32) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for d in drawables {
        let g = crate::tiles::frame(d);
        x0 = x0.min(g.x);
        y0 = y0.min(g.y);
        x1 = x1.max(g.x + g.w);
        y1 = y1.max(g.y + g.h);
    }
    if drawables.is_empty() {
        return (0.0, 0.0, 1.0, 1.0);
    }
    // table names sit above their tables
    (x0, y0 - 24.0, x1, y1)
}

/// How a sheet splits into pages under its print setup: (page size, scale, columns, rows of
/// pages, content box).
fn layout(setup: &nm_core::sheets::PageSetup, b: (f32, f32, f32, f32)) -> ((f32, f32), f32, usize, usize) {
    let (pw, ph) = if setup.portrait { (setup.paper.0.min(setup.paper.1), setup.paper.0.max(setup.paper.1)) } else { (setup.paper.0.max(setup.paper.1), setup.paper.0.min(setup.paper.1)) };
    let [mt, ml, mb, mr] = setup.margins;
    // (room for the page numbers at the bottom)
    let (aw, ah) = ((pw - ml - mr).max(36.0), (ph - mt - mb - if setup.page_numbers { 14.0 } else { 0.0 }).max(36.0));
    let (cw, ch) = (b.2 - b.0, b.3 - b.1);
    let s = if setup.autofit { (aw / cw).min(1.0) } else { setup.scale.clamp(0.1, 4.0) };
    let nx = ((cw * s) / aw).ceil().max(1.0) as usize;
    let ny = ((ch * s) / ah).ceil().max(1.0) as usize;
    ((pw, ph), s, nx, ny)
}

/// How many pages sheet `s` prints on.
pub fn page_count(wb: &Workbook, s: &nm_core::Sheet) -> usize {
    let setup = nm_core::sheets::page_setup(&wb.doc, s.id);
    let (_, _, nx, ny) = layout(&setup, content_box(&wb.drawables(s)));
    nx * ny
}

/// Render every sheet of `wb` (its print setup: paper, orientation, scale or Fit, margins, page
/// order, page numbers) and write the PDF; returns the page count.
pub fn export(wb: &Workbook, out: &std::path::Path) -> Result<usize, String> {
    export_with(wb, out, Default::default())
}

/// As [`export`], an Excel workbook's cell extras (rotated text, diagonals) drawn as on the sheet.
pub fn export_with(wb: &Workbook, out: &std::path::Path, extras: std::collections::HashMap<nm_core::kn_format::ObjId, std::collections::HashMap<(u32, u32), nm_core::xlstyle::Extra>>) -> Result<usize, String> {
    let mut r = Renderer::new();
    let mut tiles = crate::tiles::Tiles::for_print(extras);
    let mut pages = Vec::new();
    for s in wb.sheets().into_iter().filter(|s| !s.hidden) {
        let setup = nm_core::sheets::page_setup(&wb.doc, s.id);
        let drawables = wb.drawables(&s);
        let bx = content_box(&drawables);
        let ((pw, ph), scale, nx, ny) = layout(&setup, bx);
        let [mt, ml, _, _] = setup.margins;
        let (aw, ah) = ((pw - setup.margins[1] - setup.margins[3]) / scale, (ph - mt - setup.margins[2] - if setup.page_numbers { 14.0 } else { 0.0 }) / scale);
        let order: Vec<(usize, usize)> = if setup.down_first { (0..nx).flat_map(|x| (0..ny).map(move |y| (x, y))).collect() } else { (0..ny).flat_map(|y| (0..nx).map(move |x| (x, y))).collect() };
        let total = order.len();
        let first = setup.start_page.unwrap_or(1).max(1) as usize;
        for (k, (px, py)) in order.into_iter().enumerate() {
            let (wpx, hpx) = ((pw * DPI_SCALE) as u32, (ph * DPI_SCALE) as u32);
            let mut pm = sk::Pixmap::new(wpx, hpx).ok_or("page too big")?;
            pm.fill(sk::Color::WHITE);
            let k2 = scale * DPI_SCALE;
            // this page's part of the sheet
            let (sx, sy) = (bx.0 + px as f32 * aw, bx.1 + py as f32 * ah);
            let base = sk::Transform::from_row(k2, 0.0, 0.0, k2, ml * DPI_SCALE - sx * k2, mt * DPI_SCALE - sy * k2);
            // (drawn into a clip of the page's printable area)
            let mut part = sk::Pixmap::new(wpx, hpx).ok_or("page too big")?;
            for d in &drawables {
                tiles.draw_any(&wb.doc, d, &mut part, base);
                if let nm_core::kn_model::DrawableKind::Table(t) = &d.kind {
                    if wb.doc.table_name_shown(d.id) && !t.name.is_empty() {
                        let style = nm_core::kn_model::CharStyle { size: 14.0, ..Default::default() };
                        let tb = kn_render::simple_text_box(&t.name, style, nm_core::kn_model::Align::Center, nm_core::kn_model::VAlign::Bottom);
                        let g = crate::tiles::frame(d);
                        let tl = kn_render::text::layout(&mut r.fonts, &tb, g.w, 22.0, false, false);
                        r.draw_text(&tl, &mut part, base.pre_translate(g.x, g.y - 24.0));
                    }
                }
            }
            let clip = sk::Rect::from_xywh(ml * DPI_SCALE, mt * DPI_SCALE, aw * k2, ah * k2);
            if let Some(clip) = clip {
                let mut mask = sk::Mask::new(wpx, hpx).ok_or("page too big")?;
                mask.fill_path(&sk::PathBuilder::from_rect(clip), sk::FillRule::Winding, false, sk::Transform::identity());
                pm.draw_pixmap(0, 0, part.as_ref(), &sk::PixmapPaint::default(), sk::Transform::identity(), Some(&mask));
            }
            // Page n of m
            if setup.page_numbers {
                let text = format!("Page {} of {}", first + k, first + total - 1);
                let style = nm_core::kn_model::CharStyle { size: 9.0, color: nm_core::kn_model::Color { r: 0.35, g: 0.35, b: 0.35, a: 1.0 }, ..Default::default() };
                let tb = kn_render::simple_text_box(&text, style, nm_core::kn_model::Align::Center, nm_core::kn_model::VAlign::Bottom);
                let tl = kn_render::text::layout(&mut r.fonts, &tb, pw, 14.0, false, false);
                r.draw_text(&tl, &mut pm, sk::Transform::from_scale(DPI_SCALE, DPI_SCALE).pre_translate(0.0, ph - setup.margins[2] - 12.0));
            }
            let rgb: Vec<u8> = pm.pixels().iter().flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue()]
            }).collect();
            let mut jpg = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90)
                .encode(&rgb, wpx, hpx, image::ExtendedColorType::Rgb8)
                .map_err(|e| e.to_string())?;
            pages.push(Page { w: pw, h: ph, px: (wpx, hpx), jpg });
        }
    }
    write_pdf(&pages, out)?;
    Ok(pages.len())
}

fn write_pdf(pages: &[Page], path: &std::path::Path) -> Result<(), String> {
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let n = pages.len();
    // 1 catalog, 2 pages, then per page: page, content, image
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
    for (i, p) in pages.iter().enumerate() {
        let (content_id, image_id) = (page_obj(i) + 1, page_obj(i) + 2);
        let (pw, ph) = (p.w, p.h);
        obj(&mut out, &mut offsets, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw} {ph}] /Resources << /XObject << /Im0 {image_id} 0 R >> >> /Contents {content_id} 0 R >>").as_bytes());
        let content = format!("q {pw} 0 0 {ph} 0 0 cm /Im0 Do Q");
        obj(&mut out, &mut offsets, format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()).as_bytes());
        let mut body = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            p.px.0,
            p.px.1,
            p.jpg.len()
        )
        .into_bytes();
        body.extend(&p.jpg);
        body.extend(b"\nendstream");
        obj(&mut out, &mut offsets, &body);
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
    std::fs::write(path, out).map_err(|e| e.to_string())
}
