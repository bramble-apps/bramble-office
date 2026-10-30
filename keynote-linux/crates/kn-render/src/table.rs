//! Table drawing: cell fills (merged ranges as one cell), formatted cell text with the resolved
//! cell styles, then the grid strokes on top.

use kn_model::*;
use tiny_skia as sk;

use crate::paint::{frame_transform, sk_stroke, solid};
use crate::{simple_text_box, text, Renderer};

/// Cumulative edge positions (n + 1 values) of sizes scaled by `scale`.
fn edges(sizes: &[f32], n: u32, default: f32, scale: f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(n as usize + 1);
    let mut acc = 0.0;
    out.push(acc);
    for i in 0..n as usize {
        acc += sizes.get(i).copied().unwrap_or(default) * scale;
        out.push(acc);
    }
    out
}

/// Cell `i`'s text box as drawn: a rich text cell's own runs (mixed fonts, sizes, styles) with
/// the cell's vertical alignment, else [`cell_text_box`] of its plain text.
pub fn cell_box(t: &TableData, i: usize, s: &str, style: &TableCellStyle) -> kn_model::TextBox {
    match t.rich.get(i).and_then(|r| r.as_ref()) {
        Some(tb) => {
            let mut tb = tb.clone();
            tb.valign = style.valign;
            tb
        }
        None => cell_text_box(s, style),
    }
}

/// A cell's text: one paragraph per line, each with the cell's spacing and indents.
pub fn cell_text_box(s: &str, style: &TableCellStyle) -> kn_model::TextBox {
    let mut tb = simple_text_box(s, style.text.clone(), style.align, style.valign);
    if style.para == kn_model::CellPara::default() {
        return tb;
    }
    let proto = tb.paragraphs[0].clone();
    let mut paras = Vec::new();
    let mut start = 0;
    for line in s.split('\n') {
        let end = start + line.len();
        let mut p = proto.clone();
        p.start = start;
        p.end = end;
        p.runs = vec![kn_model::Run { start, end, style: style.text.clone() }];
        p.line_spacing = style.para.line_spacing;
        p.space_before = style.para.space_before;
        p.space_after = style.para.space_after;
        p.left_indent = style.para.left_indent;
        p.right_indent = style.para.right_indent;
        p.first_line_indent = style.para.first_line_indent;
        paras.push(p);
        start = end + 1;
    }
    tb.paragraphs = paras;
    tb
}

/// A plain decimal ("0.661290323", as the Automatic format shows a result) that is too wide for
/// a cell `w` wide, rounded to the most decimals that fit, as Numbers shows it (it never wraps a
/// number); `None` when the text isn't such a number or already fits.
pub fn fit_decimals(fonts: &mut crate::fonts::Fonts, t: &TableData, i: usize, s: &str, style: &TableCellStyle, w: f32) -> Option<String> {
    if t.rich.get(i).is_some_and(|r| r.is_some()) {
        return None;
    }
    let (int, frac) = s.strip_prefix('-').unwrap_or(s).split_once('.')?;
    if int.is_empty() || frac.is_empty() || !int.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: f64 = s.parse().ok()?;
    let room = w - style.padding[0] - style.padding[2];
    let mut width = |text: &str| {
        let mut tb = cell_text_box(text, style);
        tb.padding = [0.0; 4];
        text::layout(fonts, &tb, w, 0.0, true, true).content_w
    };
    if width(s) <= room {
        return None;
    }
    for d in (0..frac.len()).rev() {
        let mut n = format!("{v:.d$}");
        if n.contains('.') {
            n = n.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        if d == 0 || width(&n) <= room {
            return Some(n);
        }
    }
    None
}

/// Rows that fit their content (`t.auto_rows`, by model row; `view` maps shown rows to model
/// rows, for Numbers' categorized tables) grow to their tallest cell's text when it takes more
/// than one line (a new line, or wrapping), laid out as the cell draws it; rows never get
/// smaller than they are, and a merge spanning rows is left as it is. Shared by Keydeck, Values
/// and Letters.
pub fn fit_rows(fonts: &mut crate::fonts::Fonts, t: &mut TableData, view: Option<&[Option<u32>]>) {
    if t.auto_rows.is_empty() {
        return;
    }
    for r in 0..t.rows {
        let model = match view {
            Some(v) => match v.get(r as usize).copied().flatten() {
                Some(m) => m,
                None => continue,
            },
            None => r,
        };
        if !t.auto_rows.get(model as usize).copied().unwrap_or(false) {
            continue;
        }
        let mut need = t.row_heights.get(r as usize).copied().unwrap_or(0.0);
        if need <= 0.0 {
            continue; // hidden
        }
        for c in 0..t.cols {
            let cols = match t.merge_at(r, c) {
                Some(m) if m.row != r || m.col != c || m.rows > 1 => continue,
                Some(m) => m.cols,
                None => 1,
            };
            let i = (r * t.cols + c) as usize;
            let text = t.cells.get(i).map(String::as_str).unwrap_or("");
            if text.is_empty() {
                continue;
            }
            let Some(style) = t.cell_styles.get(i) else { continue };
            if !style.wrap && !text.contains('\n') && t.rich.get(i).is_none_or(|r| r.is_none()) {
                continue;
            }
            let w: f32 = t.col_widths.iter().skip(c as usize).take(cols as usize).sum();
            // (a number is rounded to fit, never wrapped: it doesn't make the row taller)
            if fit_decimals(fonts, t, i, text, style, w).is_some() {
                continue;
            }
            let mut tb = cell_box(t, i, text, style);
            tb.padding = style.padding;
            let tl = text::layout(fonts, &tb, w, 0.0, !style.wrap, true);
            // (one line keeps the row's own height: font metrics differ slightly from Apple's)
            if tl.lines.len() > 1 {
                need = need.max(tl.content_h + style.padding[1] + style.padding[3]);
            }
        }
        if let Some(h) = t.row_heights.get_mut(r as usize) {
            *h = need;
        }
    }
}

/// Fallback look when the model has no resolved styles (e.g. tables built in code).
fn fallback_style(tab: &TableData, r: u32, c: u32) -> TableCellStyle {
    let header = r < tab.header_rows || c < tab.header_cols;
    let mut s = TableCellStyle::default();
    if header {
        s.fill = Fill::Color(Color::rgba(0.0, 0.30, 0.50, 1.0));
        s.text.color = Color::WHITE;
        s.text.bold = true;
    }
    s.text.size = 20.0;
    s
}

pub fn draw_table(r: &mut Renderer, doc: &Document, d: &Drawable, tab: &TableData, pm: &mut sk::Pixmap, base: sk::Transform) {
    // rows grow to fit their lines (as the apps lay them out; a no-op on a table already fitted)
    let mut fitted = tab.clone();
    fit_rows(&mut r.fonts, &mut fitted, None);
    let tab = &fitted;
    let g = d.geom;
    // Keynote lays tables out at their row/column sizes (the frame only positions them); fall
    // back to stretching when the sizes are missing.
    let t = base.pre_concat(frame_transform(&tab.frame(&g), true));
    let (tw, th) = (tab.width(), tab.height());
    let sx = if tw > 0.0 { 1.0 } else { g.w / tab.cols.max(1) as f32 / 100.0 };
    let sy = if th > 0.0 { 1.0 } else { g.h / tab.rows.max(1) as f32 / 40.0 };
    let xs = edges(&tab.col_widths, tab.cols, 100.0, sx);
    let ys = edges(&tab.row_heights, tab.rows, 40.0, sy);
    // one device pixel in table units: fills overlap by this much so adjacent cells leave no seams
    let px = 1.0 / t.sx.hypot(t.ky).max(1e-3);

    // fills and text, one pass per (merged) cell
    for row in 0..tab.rows {
        for col in 0..tab.cols {
            let (r1, c1) = match tab.merge_at(row, col) {
                Some(m) if m.row != row || m.col != col => continue,
                Some(m) => ((m.row + m.rows).min(tab.rows), (m.col + m.cols).min(tab.cols)),
                None => (row + 1, col + 1),
            };
            let i = (row * tab.cols + col) as usize;
            let style = tab.cell_styles.get(i).cloned().unwrap_or_else(|| fallback_style(tab, row, col));
            let (x, y) = (xs[col as usize], ys[row as usize]);
            let (w, h) = (xs[c1 as usize] - x, ys[r1 as usize] - y);
            if w <= 0.0 || h <= 0.0 {
                continue;
            }
            let ct = t.pre_translate(x, y);
            if style.fill != Fill::None {
                let ow = if c1 < tab.cols { px } else { 0.0 };
                let oh = if r1 < tab.rows { px } else { 0.0 };
                r.draw_fill_rect(doc, pm, &style.fill, w + ow, h + oh, ct);
            }
            let s = tab.cells.get(i).map(String::as_str).unwrap_or("");
            // accounting currency ("€" U+200E TAB "1234.57"): the part before the tab at the
            // left edge, the rest at the right edge, on one line. Any other tab (a Pages cell's
            // multi-line text, say) is laid out as ordinary text.
            let accounting = s.split_once('\t').filter(|(left, _)| left.ends_with('\u{200E}') && !s.contains('\n'));
            if let Some((left, right)) = accounting {
                for (part, align) in [(left.trim_end_matches('\u{200E}'), Align::Left), (right, Align::Right)] {
                    let mut tb = simple_text_box(part, style.text.clone(), align, style.valign);
                    tb.padding = style.padding;
                    let tl = text::layout(&mut r.fonts, &tb, w, h, false, false);
                    r.draw_text(&tl, pm, ct);
                }
            } else if !s.is_empty() {
                let fitted = fit_decimals(&mut r.fonts, tab, i, s, &style, w);
                let (s, wrap) = match &fitted {
                    Some(n) => (n.as_str(), false),
                    None => (s, style.wrap),
                };
                let mut tb = cell_box(tab, i, s, &style);
                tb.padding = style.padding;
                let tl = text::layout(&mut r.fonts, &tb, w, h, !wrap, false);
                r.draw_text(&tl, pm, ct);
            }
        }
    }

    // grid strokes, skipping edges inside merged ranges
    let inside = |row: u32, col: u32, other_row: u32, other_col: u32| {
        tab.merge_at(row, col).is_some_and(|m| m.contains(other_row, other_col))
    };
    // the segments drawn: horizontal ones at row boundary b, column c; vertical ones in row r at
    // column boundary b
    let hseg = |b: u32, c: u32| -> Option<&Stroke> {
        let s = tab.h_strokes.get((b * tab.cols + c) as usize)?.as_ref()?;
        (!(b > 0 && b < tab.rows && inside(b - 1, c, b, c))).then_some(s)
    };
    let vseg = |row: u32, b: u32| -> Option<&Stroke> {
        let s = tab.v_strokes.get((row * (tab.cols + 1) + b) as usize)?.as_ref()?;
        (!(b > 0 && b < tab.cols && inside(row, b - 1, row, b))).then_some(s)
    };
    // an L corner (a horizontal and a vertical line both ending at a grid point, neither running
    // on through it) leaves the corner square unpainted: the wider of the two (the horizontal on
    // a tie) reaches into it by half the other's width, so it is painted once, translucent
    // strokes too. Straight runs and T joins have no gap.
    let corner = |b: u32, c: u32| -> Option<(bool, f32)> {
        let left = if c > 0 { hseg(b, c - 1) } else { None };
        let right = if c < tab.cols { hseg(b, c) } else { None };
        let up = if b > 0 { vseg(b - 1, c) } else { None };
        let down = if b < tab.rows { vseg(b, c) } else { None };
        let h = match (left, right) {
            (Some(_), Some(_)) => return None,
            (Some(s), None) | (None, Some(s)) => s,
            _ => return None,
        };
        let v = match (up, down) {
            (Some(_), Some(_)) => return None,
            (Some(s), None) | (None, Some(s)) => s,
            _ => return None,
        };
        // (true: the horizontal owns the corner; the extension)
        Some(if h.width >= v.width { (true, v.width / 2.0) } else { (false, h.width / 2.0) })
    };
    let mut segs: Vec<(&Stroke, f32, f32, f32, f32)> = Vec::new();
    for b in 0..=tab.rows {
        for c in 0..tab.cols {
            let Some(s) = hseg(b, c) else { continue };
            let y = ys[b as usize];
            let ext = |cc: u32| corner(b, cc).filter(|(h, _)| *h).map(|(_, e)| e).unwrap_or(0.0);
            segs.push((s, xs[c as usize] - ext(c), y, xs[c as usize + 1] + ext(c + 1), y));
        }
    }
    for row in 0..tab.rows {
        for b in 0..=tab.cols {
            let Some(s) = vseg(row, b) else { continue };
            let x = xs[b as usize];
            let ext = |bb: u32| corner(bb, b).filter(|(h, _)| !*h).map(|(_, e)| e).unwrap_or(0.0);
            segs.push((s, x, ys[row as usize] - ext(row), x, ys[row as usize + 1] + ext(row + 1)));
        }
    }
    // thin lines first so heavier separators win at crossings
    segs.sort_by(|a, b| a.0.width.total_cmp(&b.0.width));
    for (s, x0, y0, x1, y1) in segs {
        let mut pb = sk::PathBuilder::new();
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        if let Some(p) = pb.finish() {
            pm.stroke_path(&p, &solid(s.color), &sk_stroke(s), t, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(w: f32, a: f32) -> Stroke {
        Stroke { color: Color::rgba(0.0, 0.0, 0.0, a), width: w, cap: LineCap::Butt, join: LineJoin::Miter, miter_limit: 4.0, dash: Vec::new(), phase: 0.0, brush: None }
    }

    /// A 2×2 table of 40-point cells with only an outline of `s`, drawn at 1:1 at (20, 20).
    fn draw(s: Stroke) -> sk::Pixmap {
        let mut tab = TableData { rows: 2, cols: 2, row_heights: vec![40.0; 2], col_widths: vec![40.0; 2], cells: vec![String::new(); 4], ..Default::default() };
        tab.h_strokes = vec![None; 6];
        tab.v_strokes = vec![None; 6];
        for c in 0..2 {
            tab.h_strokes[c] = Some(s.clone());
            tab.h_strokes[4 + c] = Some(s.clone());
        }
        for r in 0..2 {
            tab.v_strokes[r * 3] = Some(s.clone());
            tab.v_strokes[r * 3 + 2] = Some(s.clone());
        }
        let doc = Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/05-table.key")).unwrap();
        let d = Drawable { id: 0, geom: Geometry { x: 20.0, y: 20.0, w: 80.0, h: 80.0, ..Default::default() }, opacity: 1.0, shadow: None, reflection: None, locked: false, hyperlink: None, alt_text: None, placeholder: None, from_template: false, kind: DrawableKind::Table(tab.clone()) };
        let mut pm = sk::Pixmap::new(120, 120).unwrap();
        pm.fill(sk::Color::WHITE);
        let mut r = Renderer::new();
        draw_table(&mut r, &doc, &d, &tab, &mut pm, sk::Transform::identity());
        pm
    }

    fn at(pm: &sk::Pixmap, x: u32, y: u32) -> u8 {
        pm.pixel(x, y).unwrap().demultiply().red()
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn outline_corners_are_filled() {
        // a 6 pt outline: the outer corner square of each corner is painted, nothing beyond it
        let pm = draw(stroke(6.0, 1.0));
        for (x, y) in [(18, 18), (101, 18), (18, 101), (101, 101)] {
            assert!(at(&pm, x, y) < 40, "corner ({x},{y}) is filled: {}", at(&pm, x, y));
        }
        for (x, y) in [(15, 15), (105, 105), (15, 60)] {
            assert!(at(&pm, x, y) > 230, "({x},{y}) outside the outline stays white");
        }
        // the middle of the sides as before
        assert!(at(&pm, 60, 20) < 40 && at(&pm, 20, 60) < 40);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn translucent_corner_painted_once() {
        // half-transparent: the corner square is as dark as the sides, not darker (painted once)
        let pm = draw(stroke(6.0, 0.5));
        let side = at(&pm, 60, 18);
        let corner = at(&pm, 18, 18);
        assert!((i32::from(side) - i32::from(corner)).abs() <= 3, "side {side} corner {corner}");
        assert!(corner < 200, "the corner is painted: {corner}");
    }
}

#[cfg(test)]
mod cell_text_tests {
    use super::*;

    /// A row at the default height grows to three lines of text; a row resized by hand and a
    /// one-line row keep their heights.
    #[test]
    fn rows_fit_their_lines() {
        let mut st = TableCellStyle::default();
        st.text.size = 12.0;
        st.wrap = true;
        let mut t = TableData { rows: 3, cols: 1, row_heights: vec![20.0; 3], col_widths: vec![200.0], cells: vec!["a\nb\nc".into(), "a\nb\nc".into(), "one".into()], cell_styles: vec![st; 3], auto_rows: vec![true, false, true], ..Default::default() };
        let mut fonts = crate::fonts::Fonts::new();
        fit_rows(&mut fonts, &mut t, None);
        assert!(t.row_heights[0] > 40.0, "grew: {}", t.row_heights[0]);
        assert_eq!(&t.row_heights[1..], [20.0, 20.0]);
    }

    /// A long decimal in a narrow cell is rounded to fit and doesn't make its row taller; one
    /// that fits, and text, are left alone.
    #[test]
    fn numbers_round_to_fit() {
        let mut st = TableCellStyle::default();
        st.text.size = 11.0;
        st.wrap = true;
        let cells: Vec<String> = ["0.661290323", "0.7", "Kingston Black orchard", "-12.3456789"].iter().map(|s| s.to_string()).collect();
        let mut t = TableData { rows: 4, cols: 1, row_heights: vec![20.0; 4], col_widths: vec![48.0], cells: cells.clone(), cell_styles: vec![st.clone(); 4], auto_rows: vec![true; 4], ..Default::default() };
        let mut fonts = crate::fonts::Fonts::new();
        let fit = fit_decimals(&mut fonts, &t, 0, &cells[0], &st, 48.0).expect("rounded");
        assert!(fit.starts_with("0.6") && fit.len() < cells[0].len(), "{fit}");
        assert_eq!(fit_decimals(&mut fonts, &t, 1, &cells[1], &st, 48.0), None);
        assert_eq!(fit_decimals(&mut fonts, &t, 2, &cells[2], &st, 48.0), None);
        let neg = fit_decimals(&mut fonts, &t, 3, &cells[3], &st, 48.0).expect("rounded");
        assert!(neg.starts_with("-12") && !neg.ends_with('.'), "{neg}");
        fit_rows(&mut fonts, &mut t, None);
        assert_eq!(t.row_heights[0], 20.0, "a number doesn't wrap");
        assert!(t.row_heights[2] > 20.0, "text still wraps");
    }

    /// A cell with spacing and indents: one paragraph per line, each carrying them; a plain cell
    /// stays a single paragraph.
    #[test]
    fn lines_become_paragraphs() {
        let mut st = TableCellStyle::default();
        st.para = kn_model::CellPara { line_spacing: kn_model::LineSpacing::Relative(1.5), space_before: 4.0, space_after: 6.0, left_indent: 8.0, right_indent: 3.0, first_line_indent: 12.0 };
        let tb = cell_text_box("one\ntwo", &st);
        assert_eq!(tb.paragraphs.len(), 2);
        assert_eq!((tb.paragraphs[1].start, tb.paragraphs[1].end), (4, 7));
        assert_eq!((tb.paragraphs[1].left_indent, tb.paragraphs[1].first_line_indent, tb.paragraphs[1].space_after), (8.0, 12.0, 6.0));
        assert_eq!(cell_text_box("one\ntwo", &TableCellStyle::default()).paragraphs.len(), 1);
    }
}
