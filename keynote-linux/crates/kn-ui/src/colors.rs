//! The macOS Colors panel, shared by Keydeck and Numdeck: a mode strip (Wheel, Sliders,
//! Palette), the round colour wheel with a brightness slider, RGB / HSB sliders with a hex
//! field, Apple's crayon colours, an opacity slider, the current colour and a strip of saved
//! colours. [`color_well`] is the inspector's colour well, opening the panel in a popover.

use egui::{Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, epaint::Vertex, pos2, vec2};

/// What the panel remembers: its mode, the hue / saturation / brightness being edited (kept,
/// so a grey or black keeps the hue it was dragged from), and the saved colours.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorPanelState {
    /// 0 wheel, 1 sliders, 2 palette.
    pub mode: u8,
    /// Sliders: false RGB, true HSB.
    pub hsb: bool,
    pub hsv: [f32; 3],
    /// The colour `hsv` stands for (to notice a colour changed from outside).
    pub last: Color32,
    pub saved: Vec<Color32>,
    pub hex: String,
}

impl Default for ColorPanelState {
    fn default() -> Self {
        ColorPanelState { mode: 0, hsb: false, hsv: [0.0, 0.0, 1.0], last: Color32::WHITE, saved: Vec::new(), hex: String::new() }
    }
}

/// HSV (each 0..1) to an opaque colour.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// An opaque colour (each 0..1) to HSV.
pub fn rgb_to_hsv(r: f32, g: f32, b: f32) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 1e-6 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if max <= 1e-6 { 0.0 } else { d / max };
    [h, s, max]
}

fn to32(rgb: [f32; 3], a: u8) -> Color32 {
    let k = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(k(rgb[0]), k(rgb[1]), k(rgb[2]), a)
}

fn from32(c: Color32) -> [f32; 3] {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    [f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0]
}

/// "#RRGGBB", "#RGB" or "#RRGGBBAA" (the # optional) to a colour.
pub fn parse_hex(s: &str) -> Option<Color32> {
    let t = s.trim().trim_start_matches('#');
    let full: String = match t.len() {
        3 => t.chars().flat_map(|c| [c, c]).collect::<String>() + "ff",
        6 => format!("{t}ff"),
        8 => t.to_string(),
        _ => return None,
    };
    let v = u32::from_str_radix(&full, 16).ok()?;
    Some(Color32::from_rgba_unmultiplied((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// "#RRGGBB", or "#RRGGBBAA" when the colour isn't opaque.
pub fn hex_of(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 { format!("#{r:02X}{g:02X}{b:02X}") } else { format!("#{r:02X}{g:02X}{b:02X}{a:02X}") }
}

impl ColorPanelState {
    /// The saved colours as a line of hex values (for an app's settings file).
    pub fn saved_to_string(&self) -> String {
        self.saved.iter().map(|c| hex_of(*c)).collect::<Vec<_>>().join(",")
    }

    /// The saved colours from [`saved_to_string`]'s line.
    pub fn saved_from_string(&mut self, s: &str) {
        self.saved = s.split(',').filter_map(parse_hex).collect();
    }
}

/// Where hue / saturation sit on a wheel of radius `r` about `centre` (red to the right,
/// hues going round anticlockwise as on the Mac, saturation outwards).
pub fn wheel_point(centre: Pos2, r: f32, h: f32, s: f32) -> Pos2 {
    let a = h * std::f32::consts::TAU;
    centre + vec2(a.cos(), -a.sin()) * (s * r)
}

/// The hue / saturation at a point of the wheel (outside it: on its edge).
pub fn wheel_hs(centre: Pos2, r: f32, p: Pos2) -> (f32, f32) {
    let d = p - centre;
    let h = (-d.y).atan2(d.x) / std::f32::consts::TAU;
    (h.rem_euclid(1.0), (d.length() / r).clamp(0.0, 1.0))
}

/// Apple's crayon box (the Colors panel's Pencils / crayon palette), 6 rows of 8.
pub const CRAYONS: [u32; 48] = [
    0x000000, 0x191919, 0x333333, 0x4C4C4C, 0x666666, 0x7F7F7F, 0x999999, 0xB2B2B2,
    0xCCCCCC, 0xE5E5E5, 0xFFFFFF, 0x941100, 0x945200, 0x929000, 0x4F8F00, 0x008F00,
    0x009051, 0x009193, 0x005493, 0x011993, 0x531B93, 0x942193, 0x941751, 0xFF2600,
    0xFF9300, 0xFFFB00, 0x8EFA00, 0x00F900, 0x00FA92, 0x00FDFF, 0x0096FF, 0x0433FF,
    0x9437FF, 0xFF40FF, 0xFF2F92, 0xFF7E79, 0xFFD479, 0xFFFC79, 0xD4FB79, 0x73FA79,
    0x73FCD6, 0x73FDFF, 0x76D6FF, 0x7A81FF, 0xD783FF, 0xFF85FF, 0xFF8AD8, 0xFFFFFF,
];

/// The see-through checkerboard inside a rounded rectangle (nothing past its corners: a
/// rounded colour on top leaves no white corners).
fn checker_rounded(painter: &egui::Painter, r: Rect, radius: f32) {
    painter.rect_filled(r, radius, Color32::from_gray(0xff));
    let inside = |q: egui::Pos2| {
        // the point lies in the rounded rectangle
        let cx = q.x.clamp(r.min.x + radius, r.max.x - radius);
        let cy = q.y.clamp(r.min.y + radius, r.max.y - radius);
        (q - pos2(cx, cy)).length() <= radius + 0.01
    };
    let k = 5.0;
    let (mut y, mut row) = (r.min.y, 0);
    while y < r.max.y {
        let mut x = r.min.x + if row % 2 == 0 { 0.0 } else { k };
        while x < r.max.x {
            let cell = Rect::from_min_max(pos2(x, y), pos2((x + k).min(r.max.x), (y + k).min(r.max.y)));
            if [cell.left_top(), cell.right_top(), cell.left_bottom(), cell.right_bottom()].iter().all(|q| inside(*q)) {
                painter.rect_filled(cell, 0.0, Color32::from_gray(0xc8));
            }
            x += 2.0 * k;
        }
        y += k;
        row += 1;
    }
}

/// A capsule (rounded ends) filled left to right with `stops`: the colour follows the ends.
fn capsule_gradient(painter: &egui::Painter, r: Rect, stops: &[Color32]) {
    let rad = r.height() / 2.0;
    let n = 48usize;
    let colour_at = |t: f32| -> Color32 {
        let m = stops.len().max(2) - 1;
        let f = (t * m as f32).clamp(0.0, m as f32);
        let i = (f.floor() as usize).min(m.saturating_sub(1));
        let k = f - i as f32;
        let (a, b) = (stops[i.min(stops.len() - 1)], stops[(i + 1).min(stops.len() - 1)]);
        let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
        Color32::from_rgba_premultiplied(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()), mix(a.a(), b.a()))
    };
    // the x positions: round the ends by angle (so they come out round, not pointed), straight
    // between
    let mut xs: Vec<f32> = Vec::new();
    for k in 0..=12 {
        let a = std::f32::consts::PI * (1.0 - k as f32 / 12.0) / 2.0;
        xs.push(r.min.x + rad - rad * a.sin());
    }
    for k in 1..n {
        xs.push(r.min.x + rad + (r.width() - 2.0 * rad) * k as f32 / n as f32);
    }
    for k in 0..=12 {
        let a = std::f32::consts::PI * k as f32 / 12.0 / 2.0;
        xs.push(r.max.x - rad + rad * a.sin());
    }
    let mut mesh = Mesh::default();
    for (s, &x) in xs.iter().enumerate() {
        let t = ((x - r.min.x) / r.width()).clamp(0.0, 1.0);
        // half the height there: full between the ends, a circle's at them
        let dx = (r.min.x + rad - x).max(x - (r.max.x - rad)).max(0.0);
        let half = (rad * rad - dx * dx).max(0.0).sqrt();
        let c = colour_at(t);
        let base = mesh.vertices.len() as u32;
        mesh.vertices.push(Vertex { pos: pos2(x, r.center().y - half), uv: egui::epaint::WHITE_UV, color: c });
        mesh.vertices.push(Vertex { pos: pos2(x, r.center().y + half), uv: egui::epaint::WHITE_UV, color: c });
        if s > 0 {
            mesh.add_triangle(base - 2, base - 1, base);
            mesh.add_triangle(base - 1, base, base + 1);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// A horizontal track filled with `stops` (left to right) and a round knob at `t` (0..1);
/// returns the new `t` while it's dragged or clicked.
fn track(ui: &mut egui::Ui, w: f32, stops: &[Color32], t: f32, checkered: bool) -> Option<f32> {
    // the track spans the full width (every track and bar on the same edges), round-ended like
    // its knob; the knob travels its half-width in from each end, so it never sticks out
    let (r, resp) = ui.allocate_exact_size(vec2(w, 14.0), Sense::click_and_drag());
    let travel = r.shrink2(vec2(7.0, 0.0));
    let p = ui.painter();
    if checkered {
        checker_rounded(p, r, r.height() / 2.0);
    }
    capsule_gradient(p, r, stops);
    p.rect_stroke(r, r.height() / 2.0, Stroke::new(1.0, Color32::from_black_alpha(60)), egui::StrokeKind::Inside);
    let kx = travel.min.x + travel.width() * t.clamp(0.0, 1.0);
    p.circle(pos2(kx, r.center().y), 7.0, Color32::WHITE, Stroke::new(1.0, Color32::from_black_alpha(90)));
    if (resp.dragged() || resp.clicked())
        && let Some(q) = resp.interact_pointer_pos()
    {
        return Some(((q.x - travel.min.x) / travel.width()).clamp(0.0, 1.0));
    }
    None
}

/// The mode strip's three icons: wheel, sliders, palette.
fn mode_icon(p: &egui::Painter, r: Rect, mode: u8, on: bool) {
    let c = r.center();
    let col = if on { Color32::WHITE } else { crate::theme::TEXT_DIM() };
    match mode {
        0 => {
            for k in 0..12 {
                let a0 = k as f32 / 12.0 * std::f32::consts::TAU;
                let a1 = (k + 1) as f32 / 12.0 * std::f32::consts::TAU;
                let rgb = to32(hsv_to_rgb(k as f32 / 12.0, 0.8, 1.0), 255);
                p.add(Shape::convex_polygon(vec![c, c + vec2(a0.cos(), -a0.sin()) * 7.0, c + vec2(a1.cos(), -a1.sin()) * 7.0], rgb, Stroke::NONE));
            }
        }
        1 => {
            for (i, x) in [0.3f32, 0.7, 0.45].iter().enumerate() {
                let y = c.y - 4.0 + i as f32 * 4.0;
                p.line_segment([pos2(c.x - 7.0, y), pos2(c.x + 7.0, y)], Stroke::new(1.2, col));
                p.circle_filled(pos2(c.x - 7.0 + 14.0 * x, y), 1.8, col);
            }
        }
        _ => {
            for i in 0..3 {
                for j in 0..3 {
                    let q = pos2(c.x - 6.0 + j as f32 * 4.5, c.y - 6.0 + i as f32 * 4.5);
                    p.rect_filled(Rect::from_min_size(q, vec2(3.4, 3.4)), 0.6, col);
                }
            }
        }
    }
}

/// The macOS Colors panel; `width` is its content width. Returns true when the colour changed.
pub fn color_panel(ui: &mut egui::Ui, st: &mut ColorPanelState, c: &mut Color32, width: f32) -> bool {
    let alpha = c.a();
    // a colour changed from outside: edit that one
    if c.to_opaque() != st.last.to_opaque() {
        let [r, g, b] = from32(*c);
        let [h, s, v] = rgb_to_hsv(r, g, b);
        // (greys keep the hue they had)
        st.hsv = [if s > 0.001 { h } else { st.hsv[0] }, s, v];
        st.last = *c;
        st.hex = hex_of(*c);
    }
    let mut changed = false;
    let mut hsv = st.hsv;
    // the mode strip
    ui.horizontal(|ui| {
        ui.add_space((width - 3.0 * 34.0) / 2.0);
        for m in 0..3u8 {
            let (r, resp) = ui.allocate_exact_size(vec2(32.0, 24.0), Sense::click());
            let on = st.mode == m;
            if on {
                ui.painter().rect_filled(r, 6.0, crate::theme::ACCENT());
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
            }
            mode_icon(ui.painter(), r, m, on);
            if resp.on_hover_text(["Color Wheel", "Color Sliders", "Color Palettes"][m as usize]).clicked() {
                st.mode = m;
            }
        }
    });
    ui.add_space(8.0);
    match st.mode {
        0 => {
            // the wheel: hue round it, saturation outwards, at the brightness below
            let d = width.min(220.0);
            ui.horizontal(|ui| {
                ui.add_space((width - d) / 2.0);
                let (r, resp) = ui.allocate_exact_size(vec2(d, d), Sense::click_and_drag());
                let (centre, rad) = (r.center(), d / 2.0 - 2.0);
                let mut mesh = Mesh::default();
                let (seg, rings) = (96usize, 12usize);
                mesh.vertices.push(Vertex { pos: centre, uv: egui::epaint::WHITE_UV, color: to32(hsv_to_rgb(0.0, 0.0, hsv[2]), 255) });
                for k in 1..=rings {
                    let s = k as f32 / rings as f32;
                    for i in 0..seg {
                        let h = i as f32 / seg as f32;
                        mesh.vertices.push(Vertex { pos: wheel_point(centre, rad, h, s), uv: egui::epaint::WHITE_UV, color: to32(hsv_to_rgb(h, s, hsv[2]), 255) });
                    }
                }
                let idx = |k: usize, i: usize| (1 + (k - 1) * seg + i % seg) as u32;
                for i in 0..seg {
                    mesh.add_triangle(0, idx(1, i), idx(1, i + 1));
                }
                for k in 2..=rings {
                    for i in 0..seg {
                        mesh.add_triangle(idx(k - 1, i), idx(k, i), idx(k, i + 1));
                        mesh.add_triangle(idx(k - 1, i), idx(k, i + 1), idx(k - 1, i + 1));
                    }
                }
                ui.painter().add(Shape::mesh(mesh));
                ui.painter().circle_stroke(centre, rad, Stroke::new(1.0, Color32::from_black_alpha(70)));
                let at = wheel_point(centre, rad, hsv[0], hsv[1]);
                ui.painter().circle_stroke(at, 6.0, Stroke::new(2.0, Color32::WHITE));
                ui.painter().circle_stroke(at, 7.0, Stroke::new(1.0, Color32::from_black_alpha(120)));
                if (resp.dragged() || resp.clicked())
                    && let Some(p) = resp.interact_pointer_pos()
                {
                    let (h, s) = wheel_hs(centre, rad, p);
                    hsv[0] = h;
                    hsv[1] = s;
                    changed = true;
                }
            });
            ui.add_space(8.0);
            let stops = [to32(hsv_to_rgb(hsv[0], hsv[1], 0.0), 255), to32(hsv_to_rgb(hsv[0], hsv[1], 1.0), 255)];
            if let Some(t) = track(ui, width, &stops, hsv[2], false) {
                hsv[2] = t;
                changed = true;
            }
        }
        1 => {
            ui.horizontal(|ui| {
                for (k, name) in ["RGB", "HSB"].iter().enumerate() {
                    if ui.selectable_label(st.hsb == (k == 1), *name).clicked() {
                        st.hsb = k == 1;
                    }
                }
            });
            ui.add_space(4.0);
            let w = width - 56.0;
            if st.hsb {
                let labels = ["Hue", "Saturation", "Brightness"];
                for k in 0..3 {
                    ui.label(egui::RichText::new(labels[k]).small());
                    ui.horizontal(|ui| {
                        let stops: Vec<Color32> = (0..=12)
                            .map(|i| {
                                let t = i as f32 / 12.0;
                                let mut x = hsv;
                                x[k] = t;
                                to32(hsv_to_rgb(x[0], x[1], x[2]), 255)
                            })
                            .collect();
                        if let Some(t) = track(ui, w, &stops, hsv[k], false) {
                            hsv[k] = t;
                            changed = true;
                        }
                        let (mut v, max) = if k == 0 { ((hsv[0] * 360.0).round(), 360.0) } else { ((hsv[k] * 100.0).round(), 100.0) };
                        if ui.add(egui::DragValue::new(&mut v).range(0.0..=max).speed(1.0)).changed() {
                            hsv[k] = v / max;
                            changed = true;
                        }
                    });
                }
            } else {
                let mut rgb = hsv_to_rgb(hsv[0], hsv[1], hsv[2]);
                let labels = ["Red", "Green", "Blue"];
                let mut rgb_changed = false;
                for k in 0..3 {
                    ui.label(egui::RichText::new(labels[k]).small());
                    ui.horizontal(|ui| {
                        let mut lo = rgb;
                        lo[k] = 0.0;
                        let mut hi = rgb;
                        hi[k] = 1.0;
                        if let Some(t) = track(ui, w, &[to32(lo, 255), to32(hi, 255)], rgb[k], false) {
                            rgb[k] = t;
                            rgb_changed = true;
                        }
                        let mut v = (rgb[k] * 255.0).round();
                        if ui.add(egui::DragValue::new(&mut v).range(0.0..=255.0).speed(1.0)).changed() {
                            rgb[k] = v / 255.0;
                            rgb_changed = true;
                        }
                    });
                }
                if rgb_changed {
                    let n = rgb_to_hsv(rgb[0], rgb[1], rgb[2]);
                    hsv = [if n[1] > 0.001 { n[0] } else { hsv[0] }, n[1], n[2]];
                    changed = true;
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Hex").small());
                let r = ui.add(egui::TextEdit::singleline(&mut st.hex).desired_width(80.0).font(egui::TextStyle::Monospace));
                if r.lost_focus()
                    && let Some(p) = parse_hex(&st.hex)
                {
                    let [r2, g2, b2] = from32(p);
                    hsv = rgb_to_hsv(r2, g2, b2);
                    changed = true;
                }
            });
        }
        _ => {
            // Apple's crayons
            let cell = ((width - 7.0 * 4.0) / 8.0).floor();
            egui::Grid::new(ui.scope_id().with("crayons")).spacing(vec2(4.0, 4.0)).min_col_width(0.0).min_row_height(0.0).show(ui, |ui| {
                for (i, v) in CRAYONS.iter().enumerate() {
                    let col = Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, *v as u8);
                    let (r, resp) = ui.allocate_exact_size(vec2(cell, cell * 0.7), Sense::click());
                    ui.painter().rect_filled(r, 3.0, col);
                    ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_black_alpha(50)), egui::StrokeKind::Inside);
                    if col.to_opaque() == c.to_opaque() {
                        ui.painter().rect_stroke(r.expand(1.5), 4.0, Stroke::new(2.0, crate::theme::ACCENT()), egui::StrokeKind::Outside);
                    }
                    if resp.clicked() {
                        let [r2, g2, b2] = from32(col);
                        let n = rgb_to_hsv(r2, g2, b2);
                        hsv = [if n[1] > 0.001 { n[0] } else { hsv[0] }, n[1], n[2]];
                        changed = true;
                    }
                    if i % 8 == 7 {
                        ui.end_row();
                    }
                }
            });
        }
    }
    // opacity
    ui.add_space(8.0);
    let mut a = alpha;
    ui.label(egui::RichText::new(format!("Opacity  {}%", (f32::from(alpha) / 2.55).round())).small());
    let opaque = to32(hsv_to_rgb(hsv[0], hsv[1], hsv[2]), 255);
    if let Some(t) = track(ui, width, &[opaque.gamma_multiply(0.0), opaque], f32::from(alpha) / 255.0, true) {
        a = (t * 255.0).round() as u8;
        changed = true;
    }
    // the colour, wide, then the saved colours (click to use; + saves this one)
    ui.add_space(8.0);
    // the eyedropper (Apple's magnifier, left of the colour): a colour picked from the window
    let mut picked: Option<Color32> = None;
    let r = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // (in the Colors window only: a popover closes on the click that picks)
            if !ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::unique(IN_WINDOW))).unwrap_or(false) {
                return ui.allocate_exact_size(vec2(width, 26.0), Sense::hover()).0;
            }
            let (er, eresp) = ui.allocate_exact_size(vec2(30.0, 26.0), Sense::click());
            let on = eyedropper_active(ui.ctx());
            ui.painter().rect_filled(er, 6.0, if on { crate::theme::ACCENT() } else if eresp.hovered() { crate::theme::gray(0x48) } else { crate::theme::gray(0x38) });
            draw_eyedropper(ui.painter(), er.shrink(4.0), if on { Color32::WHITE } else { crate::theme::TEXT() });
            if eresp.on_hover_text("Pick a colour from the window").clicked() {
                start_eyedropper(ui.ctx());
            }
            ui.allocate_exact_size(vec2(width - 36.0, 26.0), Sense::hover()).0
        })
        .inner;
    if let Some(p) = eyedropper_result(ui.ctx()) {
        picked = Some(p);
    }
    let now = to32(hsv_to_rgb(hsv[0], hsv[1], hsv[2]), a);
    if a < 255 {
        checker_rounded(ui.painter(), r, 4.0);
    }
    ui.painter().rect_filled(r, 4.0, now);
    ui.painter().rect_stroke(r, 4.0, Stroke::new(1.0, Color32::from_black_alpha(70)), egui::StrokeKind::Inside);
    ui.add_space(6.0);
    let slot = 18.0;
    let per_row = ((width + 4.0) / (slot + 4.0)).floor().max(1.0) as usize;
    let mut use_saved: Option<Color32> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        let (r, resp) = ui.allocate_exact_size(vec2(slot, slot), Sense::click());
        ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, crate::theme::TEXT_DIM()), egui::StrokeKind::Inside);
        let (c0, k) = (r.center(), 4.5);
        ui.painter().line_segment([c0 - vec2(k, 0.0), c0 + vec2(k, 0.0)], Stroke::new(1.3, crate::theme::TEXT_DIM()));
        ui.painter().line_segment([c0 - vec2(0.0, k), c0 + vec2(0.0, k)], Stroke::new(1.3, crate::theme::TEXT_DIM()));
        if resp.on_hover_text("Save this colour").clicked() && !st.saved.contains(&now) {
            st.saved.insert(0, now);
            st.saved.truncate(per_row * 2 - 1);
        }
        for s in &st.saved {
            let (r, resp) = ui.allocate_exact_size(vec2(slot, slot), Sense::click());
            if s.a() < 255 {
                checker_rounded(ui.painter(), r, 3.0);
            }
            ui.painter().rect_filled(r, 3.0, *s);
            ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_black_alpha(60)), egui::StrokeKind::Inside);
            if resp.clicked() {
                use_saved = Some(*s);
            }
        }
    });
    if let Some(s) = use_saved.or(picked) {
        let [r2, g2, b2] = from32(s);
        let n = rgb_to_hsv(r2, g2, b2);
        hsv = [if n[1] > 0.001 { n[0] } else { hsv[0] }, n[1], n[2]];
        a = s.a();
        changed = true;
    }
    if changed {
        st.hsv = hsv;
        *c = to32(hsv_to_rgb(hsv[0], hsv[1], hsv[2]), a);
        st.last = *c;
        st.hex = hex_of(*c);
    }
    changed
}

/// The inspector popover's preset colours (Numbers / Keynote: a row of greys, then five rows
/// of twelve hues from deep to light), 6 x 12.
pub fn presets() -> Vec<Color32> {
    let mut out: Vec<Color32> = (0..12).map(|i| {
        let v = (255.0 * (1.0 - i as f32 / 11.0)).round() as u8;
        Color32::from_rgb(v, v, v)
    }).collect();
    // twelve hues round the wheel (Apple's order: blues, greens, yellows, oranges, reds, purples)
    let hues = [0.58f32, 0.53, 0.47, 0.38, 0.28, 0.17, 0.13, 0.08, 0.02, 0.95, 0.85, 0.75];
    for (s, v) in [(1.0f32, 0.45f32), (1.0, 0.7), (0.9, 0.95), (0.55, 1.0), (0.25, 1.0)] {
        for h in hues {
            out.push(to32(hsv_to_rgb(h, s, v), 255));
        }
    }
    out
}

const WELL_TARGET: &str = "kn-ui-colors-target";
const WELL_VALUE: &str = "kn-ui-colors-value";
/// The window's colour as the well last saw it: a change there is the user's, to apply; the
/// well's own colour changing (undo, another edit) moves the window to it instead.
const WELL_SEEN: &str = "kn-ui-colors-seen";

/// The inspector's colour well (Numbers / Keynote): a swatch that opens a popover of preset
/// colours, joined to a small colour-wheel button that opens the Colors window
/// ([`colors_window`]) on this colour. Returns true when the colour changed.
pub fn color_well(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, c: &mut Color32, width: f32) -> bool {
    let id = ui.scope_id().with(("color-well", id));
    let (r, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
    let wheel = Rect::from_min_max(pos2(r.max.x - 24.0, r.min.y), r.max);
    let swatch = Rect::from_min_max(r.min, pos2(wheel.min.x, r.max.y));
    let sresp = ui.interact(swatch, id.with("swatch"), Sense::click());
    let wresp = ui.interact(wheel, id.with("wheel"), Sense::click());
    let p = ui.painter();
    // one capsule: the swatch, then the wheel button
    p.rect_filled(r, 6.0, ui.visuals().widgets.inactive.weak_bg_fill);
    let sw = swatch.shrink2(vec2(3.0, 3.0));
    if c.a() < 255 {
        checker_rounded(p, sw, 4.0);
    }
    p.rect_filled(sw, 4.0, *c);
    p.rect_stroke(sw, 4.0, Stroke::new(1.0, Color32::from_black_alpha(60)), egui::StrokeKind::Inside);
    for k in 0..24 {
        let a0 = k as f32 / 24.0 * std::f32::consts::TAU;
        let a1 = (k + 1) as f32 / 24.0 * std::f32::consts::TAU;
        let cc = wheel.center();
        let col = to32(hsv_to_rgb(k as f32 / 24.0, 0.85, 1.0), 255);
        p.add(Shape::convex_polygon(vec![cc, cc + vec2(a0.cos(), -a0.sin()) * 8.0, cc + vec2(a1.cos(), -a1.sin()) * 8.0], col, Stroke::NONE));
    }
    p.circle_stroke(wheel.center(), 8.0, Stroke::new(1.0, Color32::from_black_alpha(60)));
    if sresp.hovered() || wresp.hovered() {
        p.rect_stroke(r, 6.0, Stroke::new(1.0, crate::theme::ACCENT()), egui::StrokeKind::Inside);
    }
    let mut changed = false;
    // the presets popover
    egui::Popup::from_toggle_button_response(&sresp).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
        let presets = presets();
        egui::Grid::new(id.with("presets")).spacing(vec2(3.0, 3.0)).show(ui, |ui| {
            for (i, pc) in presets.iter().enumerate() {
                let (q, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
                ui.painter().rect_filled(q, 2.0, *pc);
                ui.painter().rect_stroke(q, 2.0, Stroke::new(1.0, Color32::from_black_alpha(50)), egui::StrokeKind::Inside);
                if pc.to_opaque() == c.to_opaque() {
                    ui.painter().rect_stroke(q.expand(1.5), 3.0, Stroke::new(2.0, crate::theme::ACCENT()), egui::StrokeKind::Outside);
                }
                if resp.clicked() {
                    // (the well's opacity stays)
                    *c = Color32::from_rgba_unmultiplied(pc.r(), pc.g(), pc.b(), c.a());
                    changed = true;
                }
                if i % 12 == 11 {
                    ui.end_row();
                }
            }
        });
    });
    // the wheel button: the Colors window edits this well's colour
    if wresp.on_hover_text("Show Colors").clicked() {
        ui.ctx().data_mut(|d| {
            d.insert_temp(egui::Id::unique(WELL_TARGET), Some(id));
            d.insert_temp(egui::Id::unique(WELL_VALUE), *c);
            d.insert_temp(egui::Id::unique(WELL_SEEN), *c);
        });
    }
    let target: Option<egui::Id> = ui.ctx().data(|d| d.get_temp::<Option<egui::Id>>(egui::Id::unique(WELL_TARGET))).flatten();
    if target == Some(id) {
        let v: Option<Color32> = ui.ctx().data(|d| d.get_temp(egui::Id::unique(WELL_VALUE)));
        let seen: Option<Color32> = ui.ctx().data(|d| d.get_temp(egui::Id::unique(WELL_SEEN)));
        match v {
            // the window was changed: its colour goes to the well
            Some(v) if Some(v) != seen && v != *c => {
                *c = v;
                changed = true;
            }
            // the well's colour changed elsewhere (undo, a preset, another edit): the window
            // follows it rather than putting its old colour back
            Some(v) if v != *c => {
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(WELL_VALUE), *c));
            }
            _ => {}
        }
        ui.ctx().data_mut(|d| {
            d.insert_temp(egui::Id::unique(WELL_VALUE), *c);
            d.insert_temp(egui::Id::unique(WELL_SEEN), *c);
        });
    }
    changed
}

/// The Colors window for the colour well whose wheel button was clicked last (call once a
/// frame). `state` is the app's (it saves `state.saved`). Returns true while it is open.
pub fn colors_window(ctx: &egui::Context, state: &mut ColorPanelState) -> bool {
    let tid = egui::Id::unique(WELL_TARGET);
    let Some(_) = ctx.data(|d| d.get_temp::<Option<egui::Id>>(tid)).flatten() else { return false };
    let vid = egui::Id::unique(WELL_VALUE);
    let mut c: Color32 = ctx.data(|d| d.get_temp(vid)).unwrap_or(Color32::WHITE);
    let mut open = true;
    eyedropper_overlay(ctx);
    let mut close = false;
    egui::Window::new("Colors").title_bar(false).collapsible(false).resizable(false).frame(window_frame(ctx)).min_width(PANEL_W).max_width(PANEL_W).default_pos(ctx.content_rect().right_top() + vec2(-560.0, 80.0)).open(&mut open).show(ctx, |ui| {
        header(ui, &mut close);
        padded(ui, |ui| {
            color_panel(ui, state, &mut c, 232.0);
        });
    });
    open &= !close;
    ctx.data_mut(|d| d.insert_temp(vid, c));
    if !open {
        ctx.data_mut(|d| d.insert_temp::<Option<egui::Id>>(tid, None));
    }
    open
}

/// The Colors window on its own (View > Show Colors): edits `c`; returns true when it changed.
pub fn colors_window_for(ctx: &egui::Context, state: &mut ColorPanelState, c: &mut Color32, open: &mut bool) -> bool {
    let mut changed = false;
    eyedropper_overlay(ctx);
    let mut close = false;
    egui::Window::new("Colors").title_bar(false).collapsible(false).resizable(false).frame(window_frame(ctx)).min_width(PANEL_W).max_width(PANEL_W).default_pos(ctx.content_rect().right_top() + vec2(-560.0, 80.0)).open(open).show(ctx, |ui| {
        header(ui, &mut close);
        padded(ui, |ui| {
            changed = color_panel(ui, state, c, 232.0);
        });
    });
    *open &= !close;
    changed
}

/// An app's own Colors window (View > Show Colors): the panel with the suite's frame, margins
/// and eyedropper, then `extra` below it (the app's buttons: Fill Selection, Apply to Text…).
/// `id` keeps it apart from a colour well's window. Returns true when the colour changed.
pub fn colors_window_with(ctx: &egui::Context, id: &str, state: &mut ColorPanelState, c: &mut Color32, open: &mut bool, extra: impl FnOnce(&mut egui::Ui)) -> bool {
    let mut changed = false;
    eyedropper_overlay(ctx);
    let mut close = false;
    egui::Window::new("Colors").id(egui::Id::unique(id)).title_bar(false).collapsible(false).resizable(false).frame(window_frame(ctx)).min_width(PANEL_W).max_width(PANEL_W).default_pos(ctx.content_rect().right_top() + vec2(-560.0, 80.0)).open(open).show(ctx, |ui| {
        header(ui, &mut close);
        padded(ui, |ui| {
            changed = color_panel(ui, state, c, 232.0);
            ui.add_space(8.0);
            extra(ui);
        });
    });
    *open &= !close;
    changed
}

/// The Colors window's width: the 232 pt panel and 12 pt each side.
const PANEL_W: f32 = 232.0 + 24.0;

/// The window's frame with no side margin, so its title strip spans the whole window; the
/// margins are inside, round the panel ([`padded`]).
fn window_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::symmetric(0, 6))
}

/// The window's title row, drawn inside its frame (egui's own title strip is a separate frame
/// whose corner doesn't meet the window's): "Colors" centred, a close × at the right, a line
/// under it the window's whole width. It doesn't take drags, so the window moves by it.
fn header(ui: &mut egui::Ui, close: &mut bool) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
    let p = ui.painter();
    p.text(r.center(), egui::Align2::CENTER_CENTER, "Colors", egui::TextStyle::Heading.resolve(ui.style()), crate::theme::TEXT());
    // the × as egui draws every window's (its size, glyph and hover: window.rs close_button)
    let s = ui.spacing().icon_width;
    let x = egui::Rect::from_center_size(pos2(r.max.x - 8.0 - s / 2.0, r.center().y), vec2(s, s));
    let resp = ui.interact(x, ui.scope_id().with("colors-close"), Sense::click());
    let v = ui.style().interact(&resp);
    let g = x.shrink(2.0).expand(v.expansion);
    p.line_segment([g.left_top(), g.right_bottom()], v.fg_stroke);
    p.line_segment([g.right_top(), g.left_bottom()], v.fg_stroke);
    if resp.on_hover_text("Close").clicked() {
        *close = true;
    }
    p.hline(r.x_range(), r.max.y + 2.0, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    ui.add_space(6.0);
}

fn padded(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(IN_WINDOW), true));
    egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 4, bottom: 8 }).show(ui, |ui| {
        ui.set_width(232.0);
        add(ui);
    });
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(IN_WINDOW), false));
}

const IN_WINDOW: &str = "kn-colors-in-window";

// ------------------------------------------------------------------------------ eyedropper

const PICKING: &str = "kn-colors-eyedropper-picking";
const SHOT: &str = "kn-colors-eyedropper-shot";
const RESULT: &str = "kn-colors-eyedropper-result";

fn eyedropper_active(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(PICKING))).unwrap_or(false)
}

fn start_eyedropper(ctx: &egui::Context) {
    let on = !eyedropper_active(ctx);
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(PICKING), on));
}

/// While picking: the whole window takes the next click (nothing under it is touched; the
/// overlay itself is fully transparent, so it doesn't tint what is sampled) with a crosshair;
/// the click asks for a picture of the window, its own callback keeps it (other screenshots,
/// Keydeck's snapshots and recordings, never come here). Escape stops.
fn eyedropper_overlay(ctx: &egui::Context) {
    if !eyedropper_active(ctx) {
        return;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(PICKING), false));
        return;
    }
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::unique("kn-colors-eyedropper-overlay")).order(egui::Order::Foreground).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
        let resp = ui.allocate_rect(egui::Rect::from_min_size(screen.min, screen.size()), Sense::click());
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique(PICKING), false));
            let ppp = ctx.pixels_per_point();
            let c2 = ctx.clone();
            let cb = egui::ScreenshotCallback::new(move |img: std::sync::Arc<egui::ColorImage>| {
                if let Some(col) = sample(&img, p, ppp) {
                    c2.data_mut(|d| d.insert_temp(egui::Id::unique(RESULT), Some(col)));
                }
                c2.request_repaint();
            });
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique(SHOT), true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
        }
    });
}

/// The colour the eyedropper picked (once).
fn eyedropper_result(ctx: &egui::Context) -> Option<Color32> {
    let id = egui::Id::unique(RESULT);
    let r = ctx.data(|d| d.get_temp::<Option<Color32>>(id)).flatten();
    if r.is_some() {
        ctx.data_mut(|d| d.insert_temp::<Option<Color32>>(id, None));
    }
    r
}

/// The pixel of a window picture under point `p` (points; `ppp` device pixels a point).
pub fn sample(img: &egui::ColorImage, p: egui::Pos2, ppp: f32) -> Option<Color32> {
    let (x, y) = ((p.x * ppp).floor(), (p.y * ppp).floor());
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let (x, y) = (x as usize, y as usize);
    let [w, h] = img.size;
    (x < w && y < h).then(|| img.pixels[y * w + x].to_opaque())
}

/// A pipette (the colour picker's eyedropper): a fine tip bottom left, the glass tube, a collar
/// across it and the rubber bulb top right.
fn draw_eyedropper(p: &egui::Painter, r: egui::Rect, col: Color32) {
    let s = r.width().min(r.height());
    let at = |x: f32, y: f32| pos2(r.center().x + (x - 0.5) * s, r.center().y + (y - 0.5) * s);
    let thin = Stroke::new((s * 0.08).max(1.2), col);
    // tip and tube
    p.line_segment([at(0.08, 0.92), at(0.2, 0.8)], thin);
    let (a, b) = (at(0.2, 0.8), at(0.55, 0.45));
    let n = vec2(1.0, 1.0).normalized() * (s * 0.07);
    p.add(Shape::closed_line(vec![a - n, b - n, b + n, a + n], thin));
    // collar
    let m = vec2(1.0, 1.0).normalized() * (s * 0.16);
    p.line_segment([at(0.57, 0.43) - m, at(0.57, 0.43) + m], Stroke::new((s * 0.12).max(1.8), col));
    // bulb: a round-ended bar
    let (c0, c1) = (at(0.66, 0.34), at(0.8, 0.2));
    let w = s * 0.12;
    p.line_segment([c0, c1], Stroke::new(w * 2.0, col));
    p.circle_filled(c1, w, col);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips() {
        for &(r, g, b) in &[(0.2f32, 0.82, 0.48), (1.0, 0.0, 0.0), (0.1, 0.1, 0.1), (0.95, 0.6, 0.05)] {
            let [h, s, v] = rgb_to_hsv(r, g, b);
            let back = hsv_to_rgb(h, s, v);
            for (x, y) in [r, g, b].iter().zip(back.iter()) {
                assert!((x - y).abs() < 1e-4, "{r},{g},{b} -> {back:?}");
            }
        }
    }

    #[test]
    fn wheel_points_and_back() {
        let c = pos2(100.0, 100.0);
        for &(h, s) in &[(0.0f32, 1.0f32), (0.25, 0.5), (0.6, 0.9), (0.9, 0.3)] {
            let p = wheel_point(c, 80.0, h, s);
            let (h2, s2) = wheel_hs(c, 80.0, p);
            assert!((h - h2).abs() < 1e-4 && (s - s2).abs() < 1e-4, "{h},{s} -> {h2},{s2}");
        }
        // red to the right, outside clamps to the edge
        assert_eq!(wheel_point(c, 80.0, 0.0, 1.0), pos2(180.0, 100.0));
        assert_eq!(wheel_hs(c, 80.0, pos2(300.0, 100.0)).1, 1.0);
    }

    #[test]
    fn eyedropper_samples_the_pixel_under_the_point() {
        let mut img = egui::ColorImage::filled([4, 4], Color32::WHITE);
        img.pixels[2 * 4 + 3] = Color32::from_rgb(10, 20, 30);
        // (1.5, 1.0) points at 2 px a point is pixel (3, 2)
        assert_eq!(sample(&img, pos2(1.5, 1.0), 2.0), Some(Color32::from_rgb(10, 20, 30)));
        assert_eq!(sample(&img, pos2(0.0, 0.0), 2.0), Some(Color32::WHITE));
        assert_eq!(sample(&img, pos2(5.0, 1.0), 2.0), None);
        assert_eq!(sample(&img, pos2(-1.0, 1.0), 2.0), None);
    }

    #[test]
    fn window_strip_spans_the_window() {
        // the frame has no side margin: the title strip is as wide as the window
        let ctx = egui::Context::default();
        let f = window_frame(&ctx);
        assert_eq!((f.inner_margin.left, f.inner_margin.right), (0, 0));
        assert_eq!(PANEL_W, 232.0 + 24.0);
    }

    #[test]
    fn hex() {
        assert_eq!(parse_hex("#33D17A"), Some(Color32::from_rgb(0x33, 0xd1, 0x7a)));
        assert_eq!(parse_hex("f80"), Some(Color32::from_rgb(0xff, 0x88, 0x00)));
        assert_eq!(parse_hex("nope"), None);
        assert_eq!(hex_of(Color32::from_rgb(1, 2, 255)), "#0102FF");
        let half = parse_hex("#FF000080").unwrap();
        assert_eq!(half.a(), 0x80);
        assert_eq!(hex_of(half), "#FF000080");
    }

    #[test]
    fn saved_colours_as_text() {
        let mut st = ColorPanelState { saved: vec![Color32::from_rgb(0x33, 0xd1, 0x7a), parse_hex("#00000080").unwrap()], ..Default::default() };
        let line = st.saved_to_string();
        assert_eq!(line, "#33D17A,#00000080");
        let before = st.saved.clone();
        st.saved.clear();
        st.saved_from_string(&line);
        assert_eq!(st.saved, before);
        assert_eq!(presets().len(), 72);
    }
}
