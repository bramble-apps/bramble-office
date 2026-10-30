//! The toolbar's Chart popover (Keynote, Numbers and Pages): 2D / 3D / Interactive tabs, a
//! preview tile of every chart type drawn in the document's chart colours, and dots (with
//! arrows) to page through the theme's chart styles, as the Mac's popover does. Picking a tile
//! returns its menu name ("2D Column", "3D Pie", "Interactive Bubble") and the style page.

use egui::{Color32, Pos2, Rect, Shape, Stroke, Vec2};

use crate::theme;

const TABS: [&str; 3] = ["2D", "3D", "Interactive"];
const TYPES_2D: [&str; 14] = ["Column", "Stacked Column", "Bar", "Stacked Bar", "Line", "Area", "Stacked Area", "Pie", "Donut", "Scatter", "Bubble", "Mixed", "2-Axis", "Radar"];
const TYPES_3D: [&str; 8] = ["Column", "Stacked Column", "Bar", "Stacked Bar", "Line", "Area", "Stacked Area", "Pie"];
const TYPES_INTERACTIVE: [&str; 4] = ["Column", "Bar", "Scatter", "Bubble"];

/// A picked chart: its menu name ("2D Stacked Bar") and the style page it was picked on.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    pub name: String,
    pub style: usize,
}

/// The style pages' scroll state: where the strip is (in pages, fractional while moving), how
/// fast it moves (pages a second) and the page it springs to once the fingers stop.
#[derive(Clone, Copy, Default)]
struct Strip {
    pos: f32,
    vel: f32,
    /// last scroll or drag input (the strip settles a moment after the input stops)
    last_input: f64,
    target: Option<f32>,
}

/// The popover's contents. `styles`: each chart style's series colours (the document's
/// presets, in order); none gives Keynote's default colours.
pub fn picker(ui: &mut egui::Ui, styles: &[Vec<Color32>]) -> Option<Picked> {
    let id = ui.make_persistent_id("chart-picker");
    let (mut tab, page0): (usize, usize) = ui.data(|d| d.get_temp(id)).unwrap_or((0, 0));
    let fallback = vec![Color32::from_rgb(0x00, 0xa2, 0xff), Color32::from_rgb(0x61, 0xd8, 0x36), Color32::from_rgb(0x99, 0x99, 0x99), Color32::from_rgb(0xf8, 0xba, 0x00), Color32::from_rgb(0xef, 0x5f, 0xa7), Color32::from_rgb(0x00, 0xc7, 0xbe)];
    let pages = styles.len().max(1);
    let last = (pages - 1) as f32;
    let sid = id.with("strip");
    let mut strip: Strip = ui.data(|d| d.get_temp(sid)).unwrap_or(Strip { pos: page0.min(pages - 1) as f32, ..Default::default() });
    let mut picked = None;
    let (tile, gap) = (egui::vec2(84.0, 76.0), 8.0);
    let width = 3.0 * tile.x + 2.0 * gap;
    ui.set_width(width);
    ui.allocate_ui(egui::vec2(ui.available_width(), 28.0), |ui| crate::chartdata::switch(ui, &TABS, &mut tab));
    ui.add_space(8.0);
    let (types, prefix): (&[&str], &str) = match tab {
        0 => (&TYPES_2D, "2D"),
        1 => (&TYPES_3D, "3D"),
        _ => (&TYPES_INTERACTIVE, "Interactive"),
    };
    let rows = types.len().div_ceil(3) as f32;
    let (grid, _) = ui.allocate_exact_size(egui::vec2(width, rows * tile.y + (rows - 1.0) * gap), egui::Sense::hover());
    // the pages sit side by side; a swipe, a sideways scroll (Shift + wheel too) or ← / → moves
    // the strip, one or more pages in one go, and it springs onto the nearest page with a
    // little bounce when the input stops
    let page_w = width + 24.0;
    let swipe = ui.interact(grid, id.with("swipe"), egui::Sense::drag());
    let (now, dt, over, dx, key) = ui.input(|i| {
        let over = i.pointer.hover_pos().is_some_and(|p| grid.expand(12.0).contains(p));
        let d = i.raw.events.iter().fold(Vec2::ZERO, |acc, e| match e {
            egui::Event::MouseWheel { unit, delta, .. } => acc + *delta * if *unit == egui::MouseWheelUnit::Point { 1.0 } else { 40.0 },
            _ => acc,
        });
        let dx = if d.x.abs() > d.y.abs() { d.x } else if i.modifiers.shift { d.y } else { 0.0 };
        let key = if i.key_pressed(egui::Key::ArrowLeft) { -1.0 } else if i.key_pressed(egui::Key::ArrowRight) { 1.0 } else { 0.0 };
        (i.time, i.stable_dt.clamp(0.001, 0.033), over, dx, key)
    });
    if pages > 1 {
        // past either end the strip moves a third as far (a rubber band)
        let follow = |pos: f32, d: f32| if (pos < 0.0 && d < 0.0) || (pos > last && d > 0.0) { d * 0.33 } else { d };
        let moved = if swipe.dragged() {
            -swipe.drag_delta().x / page_w
        } else if over && dx != 0.0 {
            -dx / page_w
        } else {
            0.0
        };
        if moved != 0.0 {
            let d = follow(strip.pos, moved);
            strip.pos += d;
            strip.vel = strip.vel * 0.5 + (d / dt) * 0.5;
            strip.last_input = now;
            strip.target = None;
        }
        if key != 0.0 {
            strip.target = Some((strip.target.unwrap_or(strip.pos).round() + key).clamp(0.0, last));
        }
        // the input stopped (fingers lifted, the wheel paused): aim for a page, a flick going on
        // to the next one
        let idle = !swipe.dragged() && now - strip.last_input > if swipe.drag_stopped() { 0.0 } else { 0.09 };
        if strip.target.is_none() && idle && (strip.pos.fract().abs() > 1e-4 || strip.vel.abs() > 1e-3 || strip.pos < 0.0 || strip.pos > last) {
            strip.target = Some((strip.pos + (strip.vel * 0.12).clamp(-1.0, 1.0)).round().clamp(0.0, last));
        }
        if let Some(t) = strip.target {
            // a spring a little short of critical damping: it overshoots and settles
            let (k, c) = (260.0, 2.0 * 260.0_f32.sqrt() * 0.62);
            let a = k * (t - strip.pos) - c * strip.vel;
            strip.vel += a * dt;
            strip.pos += strip.vel * dt;
            if (t - strip.pos).abs() < 5e-4 && strip.vel.abs() < 5e-3 {
                strip.pos = t;
                strip.vel = 0.0;
                strip.target = None;
            }
        }
        if strip.target.is_some() || moved != 0.0 || !idle {
            ui.ctx().request_repaint();
        }
    } else {
        strip = Strip::default();
    }
    let page = strip.pos.round().clamp(0.0, last) as usize;
    let moving = (strip.pos - strip.pos.round()).abs() > 0.02 || swipe.dragged();
    let painter = ui.painter().with_clip_rect(grid.expand(2.0));
    let lo = strip.pos.floor().max(0.0) as usize;
    let hi = (strip.pos.ceil().max(0.0) as usize).min(pages - 1);
    for k in lo..=hi {
        let shift = (k as f32 - strip.pos) * page_w;
        if shift.abs() >= page_w {
            continue;
        }
        let cols: Vec<Color32> = styles.get(k).filter(|c| !c.is_empty()).cloned().unwrap_or_else(|| fallback.clone());
        for (i, ty) in types.iter().enumerate() {
            let (row, col) = ((i / 3) as f32, (i % 3) as f32);
            let r = Rect::from_min_size(grid.min + egui::vec2(shift + col * (tile.x + gap), row * (tile.y + gap)), tile);
            let resp = ui.interact(r.intersect(grid), id.with(("tile", k, tab, i)), if moving { egui::Sense::hover() } else { egui::Sense::click() });
            let hot = resp.hovered() && !moving;
            let bg = if hot { theme::ACCENT().gamma_multiply(0.25) } else { Color32::from_gray(0xf2) };
            painter.rect_filled(r, 8.0, bg);
            if hot {
                painter.rect_stroke(r, 8.0, Stroke::new(1.5, theme::ACCENT()), egui::StrokeKind::Inside);
            }
            preview(&painter, r.shrink(10.0), ty, &cols, tab == 1, tab == 2);
            let name = format!("{prefix} {ty}");
            if !moving && resp.on_hover_text(&name).clicked() {
                picked = Some(Picked { name, style: k });
            }
        }
    }
    // the chart styles: ‹ • • • › (the Mac's dots under the tiles)
    if pages > 1 {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let total = pages as f32 * 14.0 + 2.0 * 22.0;
            ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
            let mut go = None;
            if ui.add_enabled(page > 0, egui::Button::new("‹").frame(false)).clicked() {
                go = Some(page as f32 - 1.0);
            }
            for k in 0..pages {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
                ui.painter().circle_filled(r.center(), 3.5, if k == page { theme::TEXT() } else { theme::TEXT_DIM().gamma_multiply(0.5) });
                if resp.clicked() {
                    go = Some(k as f32);
                }
            }
            if ui.add_enabled(page + 1 < pages, egui::Button::new("›").frame(false)).clicked() {
                go = Some(page as f32 + 1.0);
            }
            if go.is_some() {
                strip.target = go;
                ui.ctx().request_repaint();
            }
        });
    }
    ui.data_mut(|d| {
        d.insert_temp(id, (tab, page));
        d.insert_temp(sid, strip);
    });
    picked
}

fn col(cols: &[Color32], i: usize) -> Color32 {
    cols[i % cols.len().max(1)]
}

/// A darker shade (3D sides and tops).
fn shade(c: Color32, k: f32) -> Color32 {
    Color32::from_rgb((c.r() as f32 * k) as u8, (c.g() as f32 * k) as u8, (c.b() as f32 * k) as u8)
}

fn lighten(c: Color32) -> Color32 {
    let l = |v: u8| v as f32 + (255.0 - v as f32) * 0.3;
    Color32::from_rgb(l(c.r()) as u8, l(c.g()) as u8, l(c.b()) as u8)
}

/// The document's chart styles (its presets) and each one's series colours, for `picker`.
pub fn styles(doc: &kn_model::Document) -> (Vec<kn_format::ObjId>, Vec<Vec<Color32>>) {
    let ids = doc.chart_presets();
    let cols = ids.iter().map(|p| doc.chart_preset_colors(*p).into_iter().map(|c| Color32::from_rgb((c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8)).collect()).collect();
    (ids, cols)
}

/// A 3D bar: front face plus top and side.
fn bar3d(p: &egui::Painter, r: Rect, c: Color32, depth: Vec2) {
    p.add(Shape::convex_polygon(vec![r.left_top(), r.left_top() + depth, r.right_top() + depth, r.right_top()], lighten(c), Stroke::NONE));
    p.add(Shape::convex_polygon(vec![r.right_top(), r.right_top() + depth, r.right_bottom() + depth, r.right_bottom()], shade(c, 0.7), Stroke::NONE));
    p.rect_filled(r, 0.0, c);
}

/// A small picture of chart type `ty` in rect `r`.
pub fn preview(p: &egui::Painter, r: Rect, ty: &str, cols: &[Color32], three_d: bool, interactive: bool) {
    let axis = Stroke::new(1.0, Color32::from_gray(0xb0));
    let depth = if three_d { Vec2::new(4.0, -4.0) } else { Vec2::ZERO };
    // interactive charts: a slider under the plot
    let r = if interactive {
        let s = Rect::from_min_max(Pos2::new(r.min.x + 6.0, r.max.y - 4.0), Pos2::new(r.max.x - 6.0, r.max.y - 2.0));
        p.rect_filled(s, 1.0, Color32::from_gray(0xc4));
        p.circle_filled(Pos2::new(s.min.x + s.width() * 0.35, s.center().y), 3.0, Color32::WHITE);
        p.circle_stroke(Pos2::new(s.min.x + s.width() * 0.35, s.center().y), 3.0, Stroke::new(0.8, Color32::from_gray(0x90)));
        Rect::from_min_max(r.min, Pos2::new(r.max.x, r.max.y - 9.0))
    } else {
        r
    };
    let base = |p: &egui::Painter| {
        p.line_segment([r.left_bottom(), r.right_bottom()], axis);
    };
    let sets = [[0.45, 0.7], [0.65, 0.5], [0.9, 0.75]];
    match ty {
        "Column" | "2-Axis" | "Mixed" => {
            base(p);
            let n = 3;
            let w = r.width() / (n as f32 * 3.0);
            for (k, s) in sets.iter().enumerate() {
                for (j, v) in s.iter().enumerate() {
                    let x = r.min.x + w * 0.5 + k as f32 * w * 3.0 + j as f32 * w;
                    let br = Rect::from_min_max(Pos2::new(x, r.max.y - v * (r.height() - 4.0)), Pos2::new(x + w * 0.9, r.max.y));
                    if three_d {
                        bar3d(p, br, col(cols, j), depth);
                    } else if ty != "Column" && j == 1 {
                        // mixed / 2-axis: the second series as a line
                    } else {
                        p.rect_filled(br, 0.0, col(cols, j));
                    }
                }
            }
            if ty != "Column" && !three_d {
                let pts: Vec<Pos2> = sets.iter().enumerate().map(|(k, s)| Pos2::new(r.min.x + w * 2.0 + k as f32 * w * 3.0, r.max.y - s[1] * (r.height() - 4.0))).collect();
                p.add(Shape::line(pts.clone(), Stroke::new(2.0, col(cols, 1))));
                for q in pts {
                    p.circle_filled(q, 2.0, col(cols, 1));
                }
                if ty == "2-Axis" {
                    p.line_segment([r.right_top(), r.right_bottom()], axis);
                }
            }
            if ty != "Column" || three_d {
                p.line_segment([r.left_top(), r.left_bottom()], axis);
            }
        }
        "Stacked Column" => {
            base(p);
            let w = r.width() / 5.0;
            for (k, s) in sets.iter().enumerate() {
                let x = r.min.x + w * 0.6 + k as f32 * w * 1.6;
                let mut y = r.max.y;
                for (j, v) in s.iter().enumerate() {
                    let h = v * (r.height() - 4.0) / 1.7;
                    let br = Rect::from_min_max(Pos2::new(x, y - h), Pos2::new(x + w, y));
                    if three_d && j == s.len() - 1 {
                        bar3d(p, br, col(cols, j), depth);
                    } else {
                        if three_d {
                            p.add(Shape::convex_polygon(vec![br.right_top(), br.right_top() + depth, br.right_bottom() + depth, br.right_bottom()], shade(col(cols, j), 0.7), Stroke::NONE));
                        }
                        p.rect_filled(br, 0.0, col(cols, j));
                    }
                    y -= h;
                }
            }
        }
        "Bar" => {
            p.line_segment([r.left_top(), r.left_bottom()], axis);
            let h = r.height() / 9.0;
            for (k, s) in sets.iter().enumerate() {
                for (j, v) in s.iter().enumerate() {
                    let y = r.min.y + h * 0.5 + k as f32 * h * 3.0 + j as f32 * h;
                    let br = Rect::from_min_max(Pos2::new(r.min.x, y), Pos2::new(r.min.x + v * (r.width() - 4.0), y + h * 0.9));
                    if three_d {
                        bar3d(p, br, col(cols, j), depth);
                    } else {
                        p.rect_filled(br, 0.0, col(cols, j));
                    }
                }
            }
        }
        "Stacked Bar" => {
            p.line_segment([r.left_top(), r.left_bottom()], axis);
            let h = r.height() / 5.0;
            for (k, s) in sets.iter().enumerate() {
                let y = r.min.y + h * 0.6 + k as f32 * h * 1.6;
                let mut x = r.min.x;
                for (j, v) in s.iter().enumerate() {
                    let w = v * (r.width() - 4.0) / 1.7;
                    let br = Rect::from_min_max(Pos2::new(x, y), Pos2::new(x + w, y + h));
                    if three_d {
                        bar3d(p, br, col(cols, j), depth);
                    } else {
                        p.rect_filled(br, 0.0, col(cols, j));
                    }
                    x += w;
                }
            }
        }
        "Line" => {
            base(p);
            p.line_segment([r.left_top(), r.left_bottom()], axis);
            for j in 0..2 {
                let ys = if j == 0 { [0.3, 0.55, 0.45, 0.8] } else { [0.15, 0.3, 0.6, 0.5] };
                let pts: Vec<Pos2> = ys.iter().enumerate().map(|(k, v)| Pos2::new(r.min.x + 4.0 + k as f32 * (r.width() - 8.0) / 3.0, r.max.y - v * (r.height() - 4.0))).collect();
                if three_d {
                    // a ribbon: its back edge, then its face
                    let back: Vec<Pos2> = pts.iter().map(|q| *q + depth).collect();
                    p.add(Shape::line(back, Stroke::new(4.0, shade(col(cols, j), 0.65))));
                    p.add(Shape::line(pts.clone(), Stroke::new(4.0, col(cols, j))));
                } else {
                    p.add(Shape::line(pts.clone(), Stroke::new(2.0, col(cols, j))));
                    for q in pts {
                        p.circle_filled(q, 2.2, col(cols, j));
                    }
                }
            }
        }
        "Area" | "Stacked Area" => {
            base(p);
            let series: [[f32; 4]; 2] = if ty == "Area" { [[0.55, 0.8, 0.6, 0.9], [0.25, 0.45, 0.3, 0.5]] } else { [[0.35, 0.5, 0.4, 0.55], [0.2, 0.3, 0.25, 0.35]] };
            let mut below = [0.0f32; 4];
            for (j, s) in series.iter().enumerate() {
                let tops: Vec<f32> = s.iter().zip(below).map(|(v, b)| if ty == "Area" { *v } else { v + b }).collect();
                let x = |k: usize| r.min.x + k as f32 * r.width() / 3.0;
                let y = |v: f32| r.max.y - v * (r.height() - 4.0);
                // as vertical strips (each convex), back to front
                for k in 0..3 {
                    let (b0, b1) = if ty == "Area" { (0.0, 0.0) } else { (below[k], below[k + 1]) };
                    let pts = vec![Pos2::new(x(k), y(b0)), Pos2::new(x(k), y(tops[k])), Pos2::new(x(k + 1), y(tops[k + 1])), Pos2::new(x(k + 1), y(b1))];
                    if three_d {
                        p.add(Shape::convex_polygon(pts.iter().map(|q| *q + depth * (1.0 - j as f32)).collect(), shade(col(cols, j), 0.75), Stroke::NONE));
                    }
                    p.add(Shape::convex_polygon(pts, col(cols, j).gamma_multiply(if ty == "Area" && j == 0 { 0.9 } else { 1.0 }), Stroke::NONE));
                }
                if ty != "Area" {
                    below = [tops[0], tops[1], tops[2], tops[3]];
                }
            }
        }
        "Pie" | "Donut" => {
            let c = r.center();
            let rad = r.width().min(r.height()) / 2.0 - 1.0;
            let parts = [0.38, 0.24, 0.2, 0.18];
            let mut a = -std::f32::consts::FRAC_PI_2;
            let squash = if three_d { 0.6 } else { 1.0 };
            let at = |ang: f32, rr: f32, lift: f32| Pos2::new(c.x + rr * ang.cos(), c.y + lift + rr * ang.sin() * squash);
            if three_d {
                // the rim
                let mut a2 = a;
                for (j, f) in parts.iter().enumerate() {
                    let b = a2 + f * std::f32::consts::TAU;
                    let steps = 10;
                    for s in 0..steps {
                        let (u, v) = (a2 + (b - a2) * s as f32 / steps as f32, a2 + (b - a2) * (s + 1) as f32 / steps as f32);
                        if u.sin() > -0.2 || v.sin() > -0.2 {
                            p.add(Shape::convex_polygon(vec![at(u, rad, 0.0), at(v, rad, 0.0), at(v, rad, 6.0), at(u, rad, 6.0)], shade(col(cols, j), 0.7), Stroke::NONE));
                        }
                    }
                    a2 = b;
                }
            }
            for (j, f) in parts.iter().enumerate() {
                let b = a + f * std::f32::consts::TAU;
                let steps = 16;
                let mut pts = vec![c];
                for s in 0..=steps {
                    pts.push(at(a + (b - a) * s as f32 / steps as f32, rad, 0.0));
                }
                // (every wedge here is under half the pie, so convex)
                p.add(Shape::convex_polygon(pts, col(cols, j), Stroke::new(0.6, col(cols, j))));
                a = b;
            }
            if ty == "Donut" {
                let hole = Rect::from_center_size(c, Vec2::new(rad, rad * squash));
                p.add(Shape::ellipse_filled(hole.center(), hole.size() / 2.0 * 1.05, Color32::from_gray(0xf2)));
            }
        }
        "Scatter" | "Bubble" => {
            base(p);
            p.line_segment([r.left_top(), r.left_bottom()], axis);
            let pts = [(0.15, 0.3), (0.3, 0.55), (0.45, 0.4), (0.6, 0.75), (0.75, 0.6), (0.85, 0.85), (0.25, 0.15), (0.55, 0.25), (0.8, 0.35)];
            for (k, (x, y)) in pts.iter().enumerate() {
                let q = Pos2::new(r.min.x + x * r.width(), r.max.y - y * r.height());
                let j = k / 6 + usize::from(k >= 6);
                if ty == "Bubble" {
                    let rr = 2.5 + ((k * 7) % 5) as f32 * 1.4;
                    p.circle_filled(q, rr, col(cols, j).gamma_multiply(0.85));
                } else {
                    p.circle_filled(q, 2.6, col(cols, j));
                }
            }
        }
        "Radar" => {
            let c = r.center();
            let rad = r.width().min(r.height()) / 2.0;
            let n = 6;
            let at = |k: usize, f: f32| {
                let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::TAU / n as f32;
                Pos2::new(c.x + rad * f * a.cos(), c.y + rad * f * a.sin())
            };
            for ring in [0.5, 1.0] {
                let pts: Vec<Pos2> = (0..n).map(|k| at(k, ring)).collect();
                p.add(Shape::closed_line(pts, axis));
            }
            for (j, vals) in [[0.9, 0.6, 0.8, 0.5, 0.7, 0.65], [0.5, 0.85, 0.4, 0.75, 0.45, 0.35]].iter().enumerate() {
                let pts: Vec<Pos2> = vals.iter().enumerate().map(|(k, f)| at(k, *f)).collect();
                for k in 0..n {
                    p.add(Shape::convex_polygon(vec![c, pts[k], pts[(k + 1) % n]], col(cols, j).gamma_multiply(0.55), Stroke::NONE));
                }
                p.add(Shape::closed_line(pts, Stroke::new(1.5, col(cols, j))));
            }
        }
        _ => {}
    }
}
