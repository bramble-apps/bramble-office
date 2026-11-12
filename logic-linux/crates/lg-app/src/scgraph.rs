//! Smart Controls' parameter mapping graphs (Logic's Scaling ▸ Open): a curve from the screen
//! control's value (x, left to right) to the parameter's (y, bottom to top), made of up to 16
//! points with bent segments. The window: preset curves at the upper left, a click on the
//! curve adds a point, a drag moves it (its values next to the pointer), a double-click adds
//! one and edits its values, Option-drag a segment bends it; Invert, Reset, Copy and Paste.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::look;

pub const MAX: usize = 16;

/// The graph: points (x, y in 0..1, x rising), each segment's bend (-1..1, 0 straight).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Graph {
    pub n: u8,
    pub pts: [(f32, f32); MAX],
    pub bend: [f32; MAX],
}

impl Default for Graph {
    fn default() -> Self {
        Graph::from(&[(0.0, 0.0), (1.0, 1.0)])
    }
}

impl Graph {
    pub fn from(pts: &[(f32, f32)]) -> Graph {
        let mut g = Graph { n: 0, pts: [(0.0, 0.0); MAX], bend: [0.0; MAX] };
        for (i, p) in pts.iter().take(MAX).enumerate() {
            g.pts[i] = (p.0.clamp(0.0, 1.0), p.1.clamp(0.0, 1.0));
            g.n = i as u8 + 1;
        }
        g
    }

    pub fn points(&self) -> &[(f32, f32)] {
        &self.pts[..self.n as usize]
    }

    pub fn is_linear(&self) -> bool {
        *self == Graph::default()
    }

    /// The output for input `x`.
    pub fn apply(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let p = self.points();
        let Some(first) = p.first() else { return x };
        if x <= first.0 {
            return first.1;
        }
        for (k, w) in p.windows(2).enumerate() {
            let ((a, ya), (b, yb)) = (w[0], w[1]);
            if x <= b {
                let t = if b > a { (x - a) / (b - a) } else { 1.0 };
                let c = self.bend[k];
                let e = 1.0 + 4.0 * c.abs();
                let s = if c >= 0.0 { t.powf(e) } else { 1.0 - (1.0 - t).powf(e) };
                return ya + (yb - ya) * s;
            }
        }
        p.last().map_or(x, |l| l.1)
    }

    /// The input whose output is nearest `y` (the screen control's position for a value).
    pub fn inverse(&self, y: f32) -> f32 {
        if self.is_linear() {
            return y.clamp(0.0, 1.0);
        }
        (0..=256).map(|i| i as f32 / 256.0).min_by(|a, b| (self.apply(*a) - y).abs().total_cmp(&(self.apply(*b) - y).abs())).unwrap_or(y)
    }

    /// A point added at `x` on the curve (or at `y`); returns its index.
    pub fn add(&mut self, x: f32, y: Option<f32>) -> Option<usize> {
        if self.n as usize >= MAX {
            return None;
        }
        let y = y.unwrap_or_else(|| self.apply(x));
        let n = self.n as usize;
        let at = self.points().iter().position(|p| p.0 > x).unwrap_or(n);
        for i in (at..n).rev() {
            self.pts[i + 1] = self.pts[i];
            self.bend[i + 1] = self.bend[i];
        }
        self.pts[at] = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
        if at > 0 {
            self.bend[at] = self.bend[at - 1];
        }
        self.n += 1;
        Some(at)
    }

    pub fn remove(&mut self, k: usize) {
        let n = self.n as usize;
        if n <= 2 || k >= n {
            return;
        }
        for i in k..n - 1 {
            self.pts[i] = self.pts[i + 1];
            self.bend[i] = self.bend[i + 1];
        }
        self.n -= 1;
    }

    /// Moves point `k` (kept between its neighbours; the ends stay at the left and right).
    pub fn set(&mut self, k: usize, x: f32, y: f32) {
        let n = self.n as usize;
        if k >= n {
            return;
        }
        let lo = if k == 0 { 0.0 } else { self.pts[k - 1].0 };
        let hi = if k + 1 == n { 1.0 } else { self.pts[k + 1].0 };
        let x = if k == 0 { 0.0 } else if k + 1 == n { 1.0 } else { x.clamp(lo, hi) };
        self.pts[k] = (x, y.clamp(0.0, 1.0));
    }

    pub fn invert(&mut self) {
        for p in &mut self.pts[..self.n as usize] {
            p.1 = 1.0 - p.1;
        }
    }

    /// The preset curves (upper left of the window).
    pub fn preset(i: usize) -> Graph {
        match i {
            1 => Graph { bend: { let mut b = [0.0; MAX]; b[0] = 0.6; b }, ..Graph::default() },
            2 => Graph { bend: { let mut b = [0.0; MAX]; b[0] = -0.6; b }, ..Graph::default() },
            3 => {
                let mut g = Graph::from(&[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]);
                g.bend[0] = 0.6;
                g.bend[1] = -0.6;
                g
            }
            4 => Graph::from(&[(0.0, 0.0), (0.25, 0.0), (0.25, 0.33), (0.5, 0.33), (0.5, 0.66), (0.75, 0.66), (0.75, 1.0), (1.0, 1.0)]),
            _ => Graph::default(),
        }
    }
}

pub const PRESETS: [&str; 5] = ["Linear", "Exponential", "Logarithmic", "S-Curve", "Steps"];

/// The window state: which mapping (view, control, mapping index), the point dragged, a
/// point whose values are being typed, the copied graph.
#[derive(Default)]
pub struct State {
    pub open: Option<(crate::smartcontrols::View, usize, usize)>,
    drag: Option<usize>,
    bend: Option<(usize, f32, f32)>,
    editing: Option<usize>,
    pub clip: Option<Graph>,
}

/// The Parameter graph window (its title: the parameter's name).
pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some((v, ci, mi)) = app.sc.graph.open else { return };
    let s = crate::smartcontrols::screen(app, v);
    let Some(m) = s.controls.get(ci).and_then(|c| c.maps.get(mi)).copied() else {
        app.sc.graph.open = None;
        return;
    };
    let all = crate::smartcontrols::parameters(app, v);
    let title = all.iter().find(|x| x.0 == m.target).map_or("Parameter".to_string(), |x| x.2.name.clone());
    let mut g = m.graph;
    let before = g;
    let mut open = true;
    egui::Window::new(format!("Parameter: {title}")).id(egui::Id::unique("sc-graph")).open(&mut open).default_size(vec2(420.0, 380.0)).resizable(true).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (i, n) in PRESETS.iter().enumerate() {
                if ui.small_button(*n).clicked() {
                    g = Graph::preset(i);
                }
            }
        });
        let avail = ui.available_size();
        let (rect, _) = ui.allocate_exact_size(vec2(avail.x - 90.0, (avail.y - 30.0).max(200.0)), Sense::hover());
        // the buttons at the right
        let side = Rect::from_min_size(pos2(rect.max.x + 8.0, rect.min.y), vec2(80.0, rect.height()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(side).layout(egui::Layout::top_down(egui::Align::Min)));
        if child.button("Invert").clicked() {
            g.invert();
        }
        if child.button("Reset").clicked() {
            g = Graph::default();
        }
        child.separator();
        if child.button("Copy").clicked() {
            app.sc.graph.clip = Some(g);
        }
        if child.add_enabled(app.sc.graph.clip.is_some(), egui::Button::new("Paste")).clicked() {
            if let Some(c) = app.sc.graph.clip {
                g = c;
            }
        }
        draw(app, ui, rect, &mut g);
    });
    if !open {
        app.sc.graph.open = None;
    }
    if g != before {
        if let Some(mm) = app.sc.screens.get_mut(&v).and_then(|s| s.controls.get_mut(ci)).and_then(|c| c.maps.get_mut(mi)) {
            mm.graph = g;
        }
        app.dirty = true;
    }
}

fn draw(app: &mut App, ui: &mut egui::Ui, r: Rect, g: &mut Graph) {
    let p = ui.painter_at(r);
    p.rect_filled(r, 4.0, look::lcd_bg());
    for i in 1..4 {
        let f = i as f32 / 4.0;
        p.vline(r.min.x + f * r.width(), r.y_range(), Stroke::new(1.0, look::grid_line()));
        p.hline(r.x_range(), r.max.y - f * r.height(), Stroke::new(1.0, look::grid_line()));
    }
    let to_pos = |x: f32, y: f32| pos2(r.min.x + x * r.width(), r.max.y - y * r.height());
    let from_pos = |q: egui::Pos2| (((q.x - r.min.x) / r.width()).clamp(0.0, 1.0), ((r.max.y - q.y) / r.height()).clamp(0.0, 1.0));
    let col = Color32::from_rgb(0xf0, 0xc0, 0x40);
    let line: Vec<egui::Pos2> = (0..=128).map(|i| i as f32 / 128.0).map(|x| to_pos(x, g.apply(x))).collect();
    p.add(egui::Shape::line(line, Stroke::new(2.0, col)));
    p.text(r.left_bottom() + vec2(4.0, -4.0), Align2::LEFT_BOTTOM, "input →", look::regular(9.5), look::lcd_dim());
    p.text(r.left_top() + vec2(4.0, 4.0), Align2::LEFT_TOP, "↑ output", look::regular(9.5), look::lcd_dim());
    let bg = ui.interact(r, ui.scope_id().with("sc-graph-bg"), Sense::click_and_drag());
    let alt = ui.input(|i| i.modifiers.alt);
    let mut on_point = false;
    let pts: Vec<(f32, f32)> = g.points().to_vec();
    for (k, &(x, y)) in pts.iter().enumerate() {
        let c = to_pos(x, y);
        let resp = ui.interact(Rect::from_center_size(c, vec2(12.0, 12.0)), ui.scope_id().with(("sc-graph-pt", k)), Sense::click_and_drag());
        on_point |= resp.hovered() || resp.dragged();
        p.circle_filled(c, if resp.hovered() || resp.dragged() { 5.0 } else { 3.5 }, Color32::WHITE);
        if resp.drag_started() {
            app.sc.graph.drag = Some(k);
        }
        if let (true, Some(pp), Some(dk)) = (resp.dragged(), resp.interact_pointer_pos(), app.sc.graph.drag) {
            if dk == k {
                let (nx, ny) = from_pos(pp);
                g.set(k, nx, ny);
                let (sx, sy) = g.pts[k];
                p.text(pp + vec2(10.0, -10.0), Align2::LEFT_BOTTOM, format!("in {:.0} · out {:.0}", sx * 100.0, sy * 100.0), look::regular(10.0), Color32::WHITE);
            }
        }
        if resp.drag_stopped() {
            app.sc.graph.drag = None;
        }
        if resp.double_clicked() {
            app.sc.graph.editing = Some(k);
        }
        resp.context_menu(|ui| {
            if ui.button("Delete Point").clicked() {
                g.remove(k);
                ui.close();
            }
        });
    }
    if !on_point {
        if bg.drag_started() && alt {
            // Option-drag a segment: bend it
            let o = ui.input(|i| i.pointer.press_origin()).unwrap_or(r.center());
            let (x, _) = from_pos(o);
            if let Some(k) = pts.windows(2).position(|w| x >= w[0].0 && x <= w[1].0) {
                app.sc.graph.bend = Some((k, o.y, g.bend[k]));
            }
        }
        if let (true, Some(pp), Some((k, y0, orig))) = (bg.dragged(), bg.interact_pointer_pos(), app.sc.graph.bend) {
            let rising = pts.get(k + 1).is_some_and(|b| b.1 >= pts[k].1);
            let d = (pp.y - y0) / r.height() * 2.0 * if rising { 1.0 } else { -1.0 };
            g.bend[k] = (orig + d).clamp(-1.0, 1.0);
        }
        if bg.drag_stopped() {
            app.sc.graph.bend = None;
        }
        if bg.clicked() && !alt {
            if let Some(pp) = bg.interact_pointer_pos() {
                g.add(from_pos(pp).0, None);
            }
        }
        if bg.double_clicked() {
            if let Some(pp) = bg.interact_pointer_pos() {
                app.sc.graph.editing = g.points().iter().position(|q| (q.0 - from_pos(pp).0).abs() < 0.01).or_else(|| g.add(from_pos(pp).0, None));
            }
        }
    }
    // a point's values typed (after a double-click)
    if let Some(k) = app.sc.graph.editing.filter(|k| *k < g.n as usize) {
        let (x, y) = g.pts[k];
        let er = Rect::from_min_size(r.min + vec2(8.0, 8.0), vec2(190.0, 24.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(er).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let (mut xi, mut yo) = (x * 100.0, y * 100.0);
        child.label("In");
        let a = child.add(egui::DragValue::new(&mut xi).range(0.0..=100.0).speed(0.5));
        child.label("Out");
        let b = child.add(egui::DragValue::new(&mut yo).range(0.0..=100.0).speed(0.5));
        if a.changed() || b.changed() {
            g.set(k, xi / 100.0, yo / 100.0);
        }
        if child.small_button("✓").clicked() {
            app.sc.graph.editing = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphs_scale_and_invert() {
        let g = Graph::default();
        assert_eq!(g.apply(0.3), 0.3);
        assert!(g.is_linear());
        let e = Graph::preset(1);
        assert!(e.apply(0.5) < 0.3);
        assert!((e.inverse(e.apply(0.5)) - 0.5).abs() < 0.01);
        let mut s = Graph::preset(4);
        assert!((s.apply(0.4) - 0.33).abs() < 0.01);
        s.invert();
        assert!((s.apply(0.4) - 0.67).abs() < 0.01);
        let mut g = Graph::default();
        let k = g.add(0.5, Some(0.9)).unwrap();
        assert_eq!(k, 1);
        assert!((g.apply(0.5) - 0.9).abs() < 1e-6);
        g.set(1, 2.0, 0.2);
        assert_eq!(g.pts[1], (1.0, 0.2));
        g.remove(1);
        assert_eq!(g.n, 2);
        // the ends stay at the edges
        g.set(0, 0.5, 0.5);
        assert_eq!(g.pts[0].0, 0.0);
    }
}
