//! Draw a laid-out page with kn-render: background, body lines, tables of contents, headers,
//! footers, footnotes and the page's objects.

use kn_render::Renderer;
use kn_render::text::TextLayout;
use tiny_skia as sk;

use crate::flow::{Layout, Page, Piece};
use crate::Pages;

/// The lines `lines` of a piece as a TextLayout of their own (glyphs, decorations, highlights).
pub fn piece_lines(pc: &Piece, lines: &[usize]) -> TextLayout {
    let tl = &pc.tl;
    let mut out = TextLayout { scale: tl.scale, ..Default::default() };
    for &li in lines {
        for &gi in &pc.line_glyphs[li] {
            out.glyphs.push(tl.glyphs[gi].clone());
        }
        let (a, b) = pc.bands[li];
        let (a, b) = (if li == 0 { f32::NEG_INFINITY } else { a }, if li + 1 == pc.bands.len() { f32::INFINITY } else { b });
        out.decos.extend(tl.decos.iter().filter(|d| d.y >= a && d.y < b).cloned());
        out.highlights.extend(tl.highlights.iter().filter(|h| {
            let mid = h.1 + h.3 / 2.0;
            mid >= a && mid < b
        }));
    }
    out
}

/// Draw one placed piece-line group with an offset.
fn draw_lines(r: &mut Renderer, pm: &mut sk::Pixmap, base: sk::Transform, pc: &Piece, lines: &[usize], dx: f32, dy: f32) {
    let tl = piece_lines(pc, lines);
    let t = base.pre_translate(dx, dy);
    if t.kx != 0.0 || t.ky != 0.0 || t.sx <= 0.0 || t.sy <= 0.0 {
        r.draw_text(&tl, pm, t);
        return;
    }
    // highlights under the glyphs, the glyphs from the cache, underlines and strikes on top
    r.draw_text(&TextLayout { glyphs: Vec::new(), decos: Vec::new(), ..tl.clone() }, pm, t);
    for g in &tl.glyphs {
        draw_glyph(r, pm, t, g);
    }
    if !tl.decos.is_empty() {
        r.draw_text(&TextLayout { glyphs: Vec::new(), highlights: Vec::new(), ..tl }, pm, t);
    }
}

/// A rasterised glyph: its pixels and where its top-left sits from the pen position (pixels).
struct CachedGlyph {
    pm: Option<sk::Pixmap>,
    ox: i32,
    oy: i32,
}

thread_local! {
    /// Keyed by a hash of (font, glyph, weight, size in 1/8 px, quarter-pixel offset, colour).
    static GLYPHS: std::cell::RefCell<std::collections::HashMap<u64, CachedGlyph>> = Default::default();
}

/// Draw one glyph through the cache: the same outline kn-render fills, rasterised once per
/// pixel size, quarter-pixel offset and colour (filling outlines anew for every strip was most
/// of a strip's time).
fn draw_glyph(r: &mut Renderer, pm: &mut sk::Pixmap, t: sk::Transform, g: &kn_render::text::GlyphPos) {
    let (px, py) = (t.tx + g.x * t.sx, t.ty + g.y * t.sy);
    if px < -200.0 || py < -200.0 || px > pm.width() as f32 + 200.0 || py > pm.height() as f32 + 200.0 {
        return;
    }
    // the pen position in quarter pixels: whole pixels and the quarter left over
    let (qx, qy) = ((px * 4.0).round() as i64, (py * 4.0).round() as i64);
    let (ix, iy) = (qx.div_euclid(4) as i32, qy.div_euclid(4) as i32);
    let (fx, fy) = (qx.rem_euclid(4) as u8, qy.rem_euclid(4) as u8);
    let c = g.color;
    let rgba = [(c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8, (c.a * 255.0).round() as u8];
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (g.font_id, g.glyph_id, g.weight.0, (g.size * t.sx * 8.0).round() as u32, ((t.sy / t.sx) * 1000.0).round() as u32, fx, fy, rgba).hash(&mut h);
        h.finish()
    };
    GLYPHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() > 40_000 {
            cache.clear();
        }
        let e = cache.entry(key).or_insert_with(|| rasterize(r, g, t.sx, t.sy, f32::from(fx) / 4.0, f32::from(fy) / 4.0));
        if let Some(gp) = &e.pm {
            pm.draw_pixmap(ix + e.ox, iy + e.oy, gp.as_ref(), &sk::PixmapPaint::default(), sk::Transform::identity(), None);
        }
    });
}

fn rasterize(r: &mut Renderer, g: &kn_render::text::GlyphPos, sx: f32, sy: f32, fx: f32, fy: f32) -> CachedGlyph {
    use swash::zeno::Command;
    let none = CachedGlyph { pm: None, ox: 0, oy: 0 };
    let Some(cmds) = r.fonts.outline(g.font_id, g.glyph_id, g.weight) else { return none };
    let s = g.size / kn_render::fonts::OUTLINE_SIZE;
    let tx = |v: swash::zeno::Vector| (fx + v.x * s * sx, fy - v.y * s * sy);
    let mut b = sk::PathBuilder::new();
    for c in cmds {
        match *c {
            Command::MoveTo(p) => {
                let (x, y) = tx(p);
                b.move_to(x, y)
            }
            Command::LineTo(p) => {
                let (x, y) = tx(p);
                b.line_to(x, y)
            }
            Command::QuadTo(c1, p) => {
                let ((x1, y1), (x, y)) = (tx(c1), tx(p));
                b.quad_to(x1, y1, x, y)
            }
            Command::CurveTo(c1, c2, p) => {
                let ((x1, y1), (x2, y2), (x, y)) = (tx(c1), tx(c2), tx(p));
                b.cubic_to(x1, y1, x2, y2, x, y)
            }
            Command::Close => b.close(),
        }
    }
    let Some(path) = b.finish() else { return none };
    let bd = path.bounds();
    let (ox, oy) = (bd.left().floor() as i32 - 1, bd.top().floor() as i32 - 1);
    let (w, h) = ((bd.right().ceil() as i32 - ox + 1).max(1) as u32, (bd.bottom().ceil() as i32 - oy + 1).max(1) as u32);
    let Some(mut gp) = sk::Pixmap::new(w, h) else { return none };
    gp.fill_path(&path, &kn_render::paint::solid(g.color), sk::FillRule::Winding, sk::Transform::from_translate(-ox as f32, -oy as f32), None);
    CachedGlyph { pm: Some(gp), ox, oy }
}

/// What to draw besides the content (the app draws selections and carets itself).
#[derive(Clone, Copy, Default)]
pub struct RenderOpts {
    /// Skip objects (the app draws them in their own layer while dragging).
    pub skip_objects: bool,
    /// Show invisibles (¶ at paragraph ends).
    pub invisibles: bool,
    /// Only what crosses this band of the page (page points, top..bottom) is drawn.
    pub clip: Option<(f32, f32)>,
    /// Only the page's base: its background and section layout objects (wallpaper).
    pub base_only: bool,
    /// Everything but the base (drawn over a copy of it the app keeps).
    pub skip_base: bool,
}

/// Draw page `index` into `pm` with `base` mapping page points to pixels.
pub fn draw_page(pages: &Pages, layout: &Layout, index: usize, r: &mut Renderer, pm: &mut sk::Pixmap, base: sk::Transform, opts: RenderOpts) {
    let Some(page) = layout.pages.get(index) else { return };
    let ds = if opts.skip_objects { Vec::new() } else { crate::flow::page_drawables(pages, page) };
    draw_page_with(pages, layout, index, r, pm, base, opts, &ds);
}

/// [`draw_page`] with the page's objects given (the app keeps them).
#[allow(clippy::too_many_arguments)]
pub fn draw_page_with(pages: &Pages, layout: &Layout, index: usize, r: &mut Renderer, pm: &mut sk::Pixmap, base: sk::Transform, opts: RenderOpts, drawables: &[kn_model::Drawable]) {
    r.fonts.word = pages.word_layout;
    let Some(page) = layout.pages.get(index) else { return };
    let setup = layout.page_setup(index);
    // section layout objects (wallpaper) go under the text, everything else over it
    let is_layout = |d: &kn_model::Drawable| page.objects.iter().any(|o| o.id == d.id && o.layout);
    let in_clip = |d: &kn_model::Drawable| {
        opts.clip.is_none_or(|(a, b)| {
            let m = d.geom.w.max(d.geom.h) * 0.5 + 30.0;
            !(d.geom.y + d.geom.h + m < a || d.geom.y - m > b)
        })
    };
    // the base: background and wallpaper
    if !opts.skip_base {
        match &page.background {
            Some(fill) => r.draw_fill_rect(&pages.doc, pm, fill, setup.width, setup.height, base),
            None => {
                if let Some(rect) = sk::Rect::from_xywh(0.0, 0.0, setup.width, setup.height) {
                    pm.fill_rect(rect, &kn_render::paint::solid(kn_model::Color::WHITE), base, None);
                }
            }
        }
        for d in drawables.iter().filter(|d| is_layout(d) && in_clip(d)) {
            r.draw_drawable(&pages.doc, d, pm, base);
        }
    }
    if opts.base_only {
        return;
    }
    draw_body(pages, layout, page, r, pm, base, opts);
    for d in drawables.iter().filter(|d| !is_layout(d)) {
        if let Some((a, b)) = opts.clip {
            // (rotated objects and shadows reach a little past their frames)
            let m = d.geom.w.max(d.geom.h) * 0.5 + 30.0;
            if d.geom.y + d.geom.h + m < a || d.geom.y - m > b {
                continue;
            }
        }
        r.draw_drawable(&pages.doc, d, pm, base);
    }
}

fn draw_body(_pages: &Pages, layout: &Layout, page: &Page, r: &mut Renderer, pm: &mut sk::Pixmap, base: sk::Transform, opts: RenderOpts) {
    // paragraph backgrounds and borders
    if let Some(body) = &layout.body {
        let mut i = 0;
        while i < page.lines.len() {
            let para = page.lines[i].para;
            let ex = &body.extra[para];
            let mut j = i;
            while j < page.lines.len() && page.lines[j].para == para {
                j += 1;
            }
            // (a drop cap's tall line isn't the paragraph's: its bar stays one text line high)
            let is_cap = |l: &crate::flow::PlacedLine| page.caps.iter().any(|c| std::sync::Arc::ptr_eq(&c.piece, &l.piece));
            let text_lines: Vec<&crate::flow::PlacedLine> = page.lines[i..j].iter().filter(|l| !is_cap(l)).collect();
            if (ex.fill.is_some() || ex.border.is_some()) && !text_lines.is_empty() {
                let top = text_lines.iter().map(|l| l.band().0).fold(f32::MAX, f32::min);
                let bottom = text_lines.iter().map(|l| l.band().1).fold(f32::MIN, f32::max);
                let col = layout.page_column(page.index, page.lines[i].column);
                if let Some(rect) = sk::Rect::from_xywh(col.0 - 4.0, top - 2.0, col.1 + 8.0, bottom - top + 4.0) {
                    if let Some(c) = ex.fill {
                        pm.fill_rect(rect, &kn_render::paint::solid(c), base, None);
                    }
                    if let Some((c, w)) = ex.border {
                        // Line (top) / bottom rules, or a box; at Word's distance from the text
                        // when the document gives one (the rule's middle half its width further)
                        let path = if ex.border_edges & 3 != 0 && ex.border_edges != 15 {
                            let (cx0, cx1) = (col.0, col.0 + col.1);
                            let mut pb = sk::PathBuilder::new();
                            let d = |off: f32| ex.border_offset.map(|o| o + w / 2.0).unwrap_or(off);
                            if ex.border_edges & 1 != 0 {
                                let y = top - d(2.0);
                                pb.move_to(cx0, y);
                                pb.line_to(cx1, y);
                            }
                            if ex.border_edges & 2 != 0 {
                                let y = bottom + d(2.0);
                                pb.move_to(cx0, y);
                                pb.line_to(cx1, y);
                            }
                            pb.finish()
                        } else {
                            Some(sk::PathBuilder::from_rect(rect))
                        };
                        if let Some(path) = path {
                            let stroke = sk::Stroke { width: w.max(0.5), ..Default::default() };
                            pm.stroke_path(&path, &kn_render::paint::solid(c), &stroke, base, None);
                        }
                    }
                }
            }
            i = j;
        }
    }
    // drop caps on a shape: the box (over the paragraph's bar, under the characters)
    for c in &page.caps {
        if let Some(fill) = c.fill {
            let (x, y, w, h) = c.rect;
            let r = c.radius.min(w / 2.0).min(h / 2.0);
            let path = if r > 0.3 { rounded_rect(x, y, w, h, r) } else { sk::Rect::from_xywh(x, y, w, h).map(sk::PathBuilder::from_rect) };
            if let Some(path) = path {
                pm.fill_path(&path, &kn_render::paint::solid(fill), sk::FillRule::Winding, base, None);
            }
        }
    }
    // body lines: group consecutive lines of the same piece
    let visible = |l: &crate::flow::PlacedLine| opts.clip.is_none_or(|(a, b)| {
        let (t, bt) = l.band();
        bt + 12.0 >= a && t - 12.0 <= b
    });
    let mut i = 0;
    while i < page.lines.len() {
        let l = &page.lines[i];
        if !visible(l) {
            i += 1;
            continue;
        }
        let mut lines = vec![l.line];
        let mut j = i + 1;
        while j < page.lines.len() && std::sync::Arc::ptr_eq(&page.lines[j].piece, &l.piece) && page.lines[j].dx == l.dx && page.lines[j].dy == l.dy && visible(&page.lines[j]) {
            lines.push(page.lines[j].line);
            j += 1;
        }
        draw_lines(r, pm, base, &l.piece, &lines, l.dx, l.dy);
        i = j;
    }
    if opts.invisibles {
        if let Some(body) = &layout.body {
            let mark_style = kn_model::CharStyle { size: 10.0, color: kn_model::Color { r: 0.35, g: 0.55, b: 0.95, a: 1.0 }, ..Default::default() };
            for l in &page.lines {
                let (_, end) = l.range();
                let p = &body.tb.paragraphs[l.para];
                if end == p.end && l.line + 1 == l.piece.tl.lines.len() {
                    let mark = match body.extra[l.para].brk {
                        Some(crate::body::Break::Page) => "¶ ⤓",
                        Some(crate::body::Break::Column) => "¶ ⇥",
                        Some(crate::body::Break::Section) => "¶ §",
                        None => "¶",
                    };
                    let tb = kn_render::simple_text_box(mark, mark_style.clone(), kn_model::Align::Left, kn_model::VAlign::Top);
                    let line = &l.piece.tl.lines[l.line];
                    let x = l.dx + line.x + line.width + 1.0;
                    let tl = kn_render::text::layout(&mut r.fonts, &tb, 60.0, 20.0, true, true);
                    let base_y = l.dy + line.baseline - tl.lines.first().map(|x| x.baseline).unwrap_or(8.0);
                    r.draw_text(&tl, pm, base.pre_translate(x, base_y));
                }
            }
        }
    }
    // tables of contents: title at the indent, the page number at the right margin
    for b in &page.toc {
        for e in &b.entries {
            let style = e.style.clone().unwrap_or_else(|| b.style.clone());
            let col = layout.page_column(page.index, 0);
            let indent = e.indent.unwrap_or((e.level.saturating_sub(1)) as f32 * 18.0);
            let tb = kn_render::simple_text_box(&e.text, style.clone(), kn_model::Align::Left, kn_model::VAlign::Top);
            let tl = kn_render::text::layout(&mut r.fonts, &tb, col.1 - indent - 40.0, style.size * 2.0, false, true);
            r.draw_text(&tl, pm, base.pre_translate(col.0 + indent, e.y));
            let num = kn_render::simple_text_box(&e.page.to_string(), style.clone(), kn_model::Align::Right, kn_model::VAlign::Top);
            let tl2 = kn_render::text::layout(&mut r.fonts, &num, 40.0, style.size * 2.0, false, true);
            r.draw_text(&tl2, pm, base.pre_translate(col.0 + col.1 - 40.0, e.y));
        }
    }
    // headers, footers
    for hf in page.headers.iter().chain(&page.footers) {
        let lines: Vec<usize> = (0..hf.piece.tl.lines.len()).collect();
        draw_lines(r, pm, base, &hf.piece, &lines, hf.dx, hf.dy);
    }
    // footnotes, under a short rule
    if let Some(first) = page.footnotes.first() {
        let y = first.dy - 8.0;
        if let Some(rect) = sk::Rect::from_xywh(first.dx, y, 100.0, 0.5) {
            pm.fill_rect(rect, &kn_render::paint::solid(kn_model::Color { r: 0.5, g: 0.5, b: 0.5, a: 1.0 }), base, None);
        }
        for f in &page.footnotes {
            let lines: Vec<usize> = (0..f.piece.tl.lines.len()).collect();
            draw_lines(r, pm, base, &f.piece, &lines, f.dx, f.dy);
        }
    }
}

/// Render a page to a new pixmap `scale` pixels per point.
pub fn render_page(pages: &Pages, layout: &Layout, index: usize, r: &mut Renderer, scale: f32, opts: RenderOpts) -> Option<sk::Pixmap> {
    let s = layout.page_setup(index);
    let mut pm = sk::Pixmap::new((s.width * scale).ceil().max(1.0) as u32, (s.height * scale).ceil().max(1.0) as u32)?;
    draw_page(pages, layout, index, r, &mut pm, sk::Transform::from_scale(scale, scale), opts);
    Some(pm)
}

/// A rounded rectangle path.
fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<sk::Path> {
    let k = 0.5523 * r;
    let mut pb = sk::PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}
