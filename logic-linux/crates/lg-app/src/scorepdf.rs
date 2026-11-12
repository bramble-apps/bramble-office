//! Printing the score (Logic: File ▸ Print ⌘P, File ▸ Page Setup, the Print dialog's PDF
//! button, the Score's Camera tool): the Page view as Studio draws it, every page, written as
//! a vector PDF (lines, shapes and the music and text fonts' outlines), without what is shown
//! only on screen (the playhead, selection colours, page shadows, hidden items). Print sends
//! the PDF to the system's printer queue (CUPS `lp`); PDF… saves it.
//!
//! The Score draws one frame for print (score.rs `printing`); its shapes are read back from
//! egui's paint list and turned into PDF content here.

use crate::app::App;
use crate::score;
use egui::epaint::{ClippedShape, Shape};
use egui::{Color32, Pos2, Rect};
use std::fmt::Write as _;

pub const COMMANDS: &[&str] = &["Print", "Page Setup…", "Set Camera Tool", "Camera Tool"];

/// Paper sizes (Page Setup): name, width and height in points (portrait).
pub const PAPERS: [(&str, f32, f32); 4] = [("A4", 595.28, 841.89), ("US Letter", 612.0, 792.0), ("US Legal", 612.0, 1008.0), ("A3", 841.89, 1190.55)];

/// What the print frame is for.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// Save every page to a PDF file.
    Pdf(std::path::PathBuf),
    /// Send to a printer (name, "" = the default) with copies.
    Print(String, u32),
    /// The Camera tool: an area of the page view (screen rect) to a PDF file.
    Camera(Rect, std::path::PathBuf),
}

/// The page size in points (Page Setup's paper and orientation).
pub fn paper(st: &score::State) -> (f32, f32) {
    let (_, w, h) = PAPERS[st.paper as usize % PAPERS.len()];
    if st.landscape { (h, w) } else { (w, h) }
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    match name {
        "Print" => {
            app.bottom = crate::app::Bottom::PianoRoll;
            score::set_editor_tab(ctx, score::EditorTab::Score);
            dialog_open(ctx, Dlg::Print { printer: 0, copies: 1 });
        }
        "Page Setup…" => dialog_open(ctx, Dlg::Setup),
        "Set Camera Tool" | "Camera Tool" => score::with_state(ctx, |s| s.tool = 8),
        _ => return false,
    }
    true
}

#[derive(Clone, Debug)]
enum Dlg {
    Print { printer: usize, copies: u32 },
    Setup,
}

fn dlg_id() -> egui::Id {
    egui::Id::unique("lg-score-print-dialog")
}

fn dialog_open(ctx: &egui::Context, d: Dlg) {
    ctx.data_mut(|m| m.insert_temp(dlg_id(), Some(d)));
}

/// The printers CUPS knows (`lpstat -e`), the default first.
fn printers() -> Vec<String> {
    let out = std::process::Command::new("lpstat").arg("-e").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// Asks where to save a PDF (tests: LG_EXPORT_PDF).
pub fn pdf_path(app: &App, title: &str) -> Option<std::path::PathBuf> {
    if app.automated {
        return std::env::var("LG_EXPORT_PDF").ok().map(std::path::PathBuf::from);
    }
    kn_ui::files::FileDialog::new().set_title("Save as PDF").set_file_name(format!("{title}.pdf")).add_filter("PDF", &["pdf"]).save_file()
}

/// The Print and Page Setup dialogs.
pub fn dialogs(app: &mut App, ctx: &egui::Context, st: &mut score::State) {
    let Some(mut d) = ctx.data(|m| m.get_temp::<Option<Dlg>>(dlg_id())).flatten() else { return };
    let mut open = true;
    let mut close = false;
    let title = app.project.as_ref().map_or("Score".to_string(), |p| p.name.clone());
    match &mut d {
        Dlg::Print { printer, copies } => {
            let list = printers();
            egui::Window::new("Print").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                egui::Grid::new("pr-grid").num_columns(2).show(ui, |ui| {
                    ui.label("Printer:");
                    if list.is_empty() {
                        ui.weak("No printers");
                    } else {
                        egui::ComboBox::from_id_salt("pr-printer").selected_text(list[(*printer).min(list.len() - 1)].clone()).show_ui(ui, |ui| {
                            for (k, n) in list.iter().enumerate() {
                                ui.selectable_value(printer, k, n);
                            }
                        });
                    }
                    ui.end_row();
                    ui.label("Copies:");
                    ui.add(egui::DragValue::new(copies).range(1..=99));
                    ui.end_row();
                    ui.label("Paper:");
                    ui.label(format!("{}{}", PAPERS[st.paper as usize % PAPERS.len()].0, if st.landscape { ", landscape" } else { "" }));
                    ui.end_row();
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("PDF…").on_hover_text("Save the score as a PDF").clicked() {
                        if let Some(path) = pdf_path(app, &title) {
                            request(ctx, st, Job::Pdf(path));
                        }
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.add_enabled(!list.is_empty(), egui::Button::new("Print")).clicked() {
                        let name = list.get(*printer).cloned().unwrap_or_default();
                        request(ctx, st, Job::Print(name, *copies));
                        close = true;
                    }
                });
            });
        }
        Dlg::Setup => {
            egui::Window::new("Page Setup").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                egui::Grid::new("ps-grid").num_columns(2).show(ui, |ui| {
                    ui.label("Paper Size:");
                    egui::ComboBox::from_id_salt("ps-paper").selected_text(PAPERS[st.paper as usize % PAPERS.len()].0).show_ui(ui, |ui| {
                        for (k, p) in PAPERS.iter().enumerate() {
                            ui.selectable_value(&mut st.paper, k as u8, p.0);
                        }
                    });
                    ui.end_row();
                    ui.label("Orientation:");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut st.landscape, false, "Portrait");
                        ui.selectable_value(&mut st.landscape, true, "Landscape");
                    });
                    ui.end_row();
                });
                if ui.button("OK").clicked() {
                    close = true;
                }
            });
        }
    }
    let keep = open && !close;
    ctx.data_mut(|m| m.insert_temp(dlg_id(), keep.then_some(d)));
}

/// Asks the Score for a print frame (Page view; drawn next frame).
pub fn request(ctx: &egui::Context, st: &mut score::State, job: Job) {
    st.print_job = Some((job, st.view));
    st.view = score::View::Page;
    ctx.request_repaint();
}

/// After the print frame: the shapes painted since `from` on the Score's layer, the pages.
pub fn finish(app: &mut App, ctx: &egui::Context, layer: egui::LayerId, from: usize, pages: &[Rect], job: Job, paper_pt: (f32, f32)) {
    let shapes: Vec<ClippedShape> = ctx.graphics_mut(|g| g.entry(layer).all_entries().skip(from).cloned().collect());
    let pdf = match &job {
        Job::Camera(r, _) => {
            // the area at the page's scale
            let page_w = pages.first().map_or(r.width(), |p| p.width());
            let s = paper_pt.0 / page_w.max(1.0);
            write_pdf(&shapes, &[*r], (r.width() * s, r.height() * s))
        }
        _ => write_pdf(&shapes, pages, paper_pt),
    };
    let result = match job {
        Job::Pdf(path) | Job::Camera(_, path) => std::fs::write(&path, &pdf).map(|_| format!("Saved {}", path.display())),
        Job::Print(printer, copies) => {
            let tmp = std::env::temp_dir().join(format!("bramble-score-{}.pdf", std::process::id()));
            std::fs::write(&tmp, &pdf).and_then(|_| {
                let mut c = std::process::Command::new("lp");
                if !printer.is_empty() {
                    c.arg("-d").arg(&printer);
                }
                c.arg("-n").arg(copies.to_string()).arg(&tmp);
                let o = c.output()?;
                if o.status.success() { Ok(format!("Sent {} page(s) to {}", pages.len(), if printer.is_empty() { "the printer" } else { &printer })) } else { Err(std::io::Error::other(String::from_utf8_lossy(&o.stderr).to_string())) }
            })
        }
    };
    match result {
        Ok(m) => app.tell(m),
        Err(e) => app.tell(format!("Couldn't print: {e}")),
    }
}

/// Fonts by the egui family a text shape uses (outlines for the PDF).
fn font_for(family: &egui::FontFamily) -> Option<ab_glyph::FontRef<'static>> {
    let bytes: &'static [u8] = match family {
        egui::FontFamily::Name(n) if n.as_ref() == "bravura" => crate::look::BRAVURA,
        egui::FontFamily::Name(n) if n.as_ref() == "bold" || n.as_ref() == "semibold" => crate::look::BOLD,
        _ => crate::look::REGULAR,
    };
    ab_glyph::FontRef::try_from_slice(bytes).ok()
}

fn col(c: Color32) -> Option<String> {
    if c.a() == 0 {
        return None;
    }
    // (premultiplied on a white page: blend toward white)
    let a = c.a() as f32 / 255.0;
    let ch = |v: u8| ((v as f32 + (1.0 - a) * 255.0).min(255.0)) / 255.0;
    Some(format!("{:.3} {:.3} {:.3}", ch(c.r()), ch(c.g()), ch(c.b())))
}

/// A page's content stream.
struct Page {
    out: String,
    origin: Pos2,
    s: f32,
    h: f32,
}

impl Page {
    fn p(&self, q: Pos2) -> (f32, f32) {
        ((q.x - self.origin.x) * self.s, self.h - (q.y - self.origin.y) * self.s)
    }
    fn mv(&mut self, q: Pos2) {
        let (x, y) = self.p(q);
        let _ = write!(self.out, "{x:.2} {y:.2} m ");
    }
    fn ln(&mut self, q: Pos2) {
        let (x, y) = self.p(q);
        let _ = write!(self.out, "{x:.2} {y:.2} l ");
    }
    fn poly(&mut self, pts: &[Pos2], closed: bool, fill: Option<Color32>, stroke: Option<(f32, Color32)>) {
        if pts.len() < 2 {
            return;
        }
        let fill = fill.and_then(col);
        let stroke = stroke.filter(|s| s.0 > 0.0).and_then(|s| col(s.1).map(|c| (s.0, c)));
        if fill.is_none() && stroke.is_none() {
            return;
        }
        self.mv(pts[0]);
        for q in &pts[1..] {
            self.ln(*q);
        }
        if closed {
            self.out += "h ";
        }
        if let Some(f) = &fill {
            let _ = write!(self.out, "{f} rg ");
        }
        if let Some((w, c)) = &stroke {
            let _ = write!(self.out, "{c} RG {:.2} w ", w * self.s);
        }
        self.out += match (fill.is_some() && closed, stroke.is_some()) {
            (true, true) => "B\n",
            (true, false) => "f\n",
            _ => "S\n",
        };
    }
    fn shape(&mut self, sh: &Shape) {
        match sh {
            Shape::Vec(v) => v.iter().for_each(|s| self.shape(s)),
            Shape::LineSegment { points, stroke } => self.poly(points, false, None, Some((stroke.width, stroke.color))),
            Shape::Path(p) => {
                let stroke = match &p.stroke.color {
                    egui::epaint::ColorMode::Solid(c) => Some((p.stroke.width, *c)),
                    _ => None,
                };
                self.poly(&p.points, p.closed, p.closed.then_some(p.fill), stroke);
            }
            Shape::Circle(c) => {
                let pts: Vec<Pos2> = (0..24).map(|k| c.center + egui::Vec2::angled(k as f32 / 24.0 * std::f32::consts::TAU) * c.radius).collect();
                self.poly(&pts, true, Some(c.fill), Some((c.stroke.width, c.stroke.color)));
            }
            Shape::Ellipse(e) => {
                let pts: Vec<Pos2> = (0..24).map(|k| {
                    let a = k as f32 / 24.0 * std::f32::consts::TAU;
                    e.center + egui::vec2(a.cos() * e.radius.x, a.sin() * e.radius.y)
                }).collect();
                self.poly(&pts, true, Some(e.fill), Some((e.stroke.width, e.stroke.color)));
            }
            Shape::Rect(r) => {
                let q = r.rect;
                let pts = [q.left_top(), q.right_top(), q.right_bottom(), q.left_bottom()];
                self.poly(&pts, true, Some(r.fill), Some((r.stroke.width, r.stroke.color)));
            }
            Shape::Mesh(m) => {
                for t in m.indices.chunks(3) {
                    if t.len() < 3 {
                        continue;
                    }
                    let v: Vec<&egui::epaint::Vertex> = t.iter().filter_map(|&i| m.vertices.get(i as usize)).collect();
                    if v.len() == 3 {
                        self.poly(&[v[0].pos, v[1].pos, v[2].pos], true, Some(v[0].color), None);
                    }
                }
            }
            Shape::CubicBezier(b) => {
                let pts: Vec<Pos2> = (0..=16).map(|k| b.sample(k as f32 / 16.0)).collect();
                let stroke = match &b.stroke.color {
                    egui::epaint::ColorMode::Solid(c) => Some((b.stroke.width, *c)),
                    _ => None,
                };
                self.poly(&pts, b.closed, b.closed.then_some(b.fill), stroke);
            }
            Shape::QuadraticBezier(b) => {
                let pts: Vec<Pos2> = (0..=16).map(|k| b.sample(k as f32 / 16.0)).collect();
                let stroke = match &b.stroke.color {
                    egui::epaint::ColorMode::Solid(c) => Some((b.stroke.width, *c)),
                    _ => None,
                };
                self.poly(&pts, b.closed, b.closed.then_some(b.fill), stroke);
            }
            Shape::Text(t) => self.text(t),
            _ => {}
        }
    }
    /// Text as the fonts' outlines (filled), glyph by glyph.
    fn text(&mut self, t: &egui::epaint::TextShape) {
        use ab_glyph::{Font, OutlineCurve};
        let Some(sec) = t.galley.job.sections.first() else { return };
        let fid = &sec.format.font_id;
        let color = if t.override_text_color.is_some() { t.override_text_color.unwrap_or(sec.format.color) } else if sec.format.color == Color32::PLACEHOLDER { t.fallback_color } else { sec.format.color };
        let Some(c) = col(color) else { return };
        let fonts = [font_for(&fid.family), font_for(&egui::FontFamily::Proportional), font_for(&egui::FontFamily::Name("bravura".into()))];
        let mut body = String::new();
        for row in &t.galley.rows {
            for g in &row.row.glyphs {
                if g.chr.is_whitespace() {
                    continue;
                }
                let Some(font) = fonts.iter().flatten().find(|f| f.glyph_id(g.chr).0 != 0) else { continue };
                let Some(outline) = font.outline(font.glyph_id(g.chr)) else { continue };
                let units = font.units_per_em().unwrap_or(1000.0);
                let scale = fid.size / units;
                let base = t.pos + row.pos.to_vec2() + g.pos.to_vec2();
                let pt = |p: ab_glyph::Point| -> (f32, f32) { self.p(Pos2::new(base.x + p.x * scale, base.y - p.y * scale)) };
                let mut last: Option<ab_glyph::Point> = None;
                for curve in &outline.curves {
                    let (a, b) = match curve {
                        OutlineCurve::Line(a, b) => (*a, *b),
                        OutlineCurve::Quad(a, _, b) => (*a, *b),
                        OutlineCurve::Cubic(a, _, _, b) => (*a, *b),
                    };
                    if last.is_none_or(|l| (l.x - a.x).abs() > 0.01 || (l.y - a.y).abs() > 0.01) {
                        let (x, y) = pt(a);
                        let _ = write!(body, "{x:.2} {y:.2} m ");
                    }
                    match curve {
                        OutlineCurve::Line(_, b) => {
                            let (x, y) = pt(*b);
                            let _ = write!(body, "{x:.2} {y:.2} l ");
                        }
                        OutlineCurve::Quad(a, q, b) => {
                            // a quadratic as a cubic
                            let c1 = ab_glyph::point(a.x + 2.0 / 3.0 * (q.x - a.x), a.y + 2.0 / 3.0 * (q.y - a.y));
                            let c2 = ab_glyph::point(b.x + 2.0 / 3.0 * (q.x - b.x), b.y + 2.0 / 3.0 * (q.y - b.y));
                            let ((x1, y1), (x2, y2), (x3, y3)) = (pt(c1), pt(c2), pt(*b));
                            let _ = write!(body, "{x1:.2} {y1:.2} {x2:.2} {y2:.2} {x3:.2} {y3:.2} c ");
                        }
                        OutlineCurve::Cubic(_, c1, c2, b) => {
                            let ((x1, y1), (x2, y2), (x3, y3)) = (pt(*c1), pt(*c2), pt(*b));
                            let _ = write!(body, "{x1:.2} {y1:.2} {x2:.2} {y2:.2} {x3:.2} {y3:.2} c ");
                        }
                    }
                    last = Some(b);
                }
            }
        }
        if !body.is_empty() {
            let _ = writeln!(self.out, "{c} rg {body}f");
        }
    }
}

/// Is the shape screen furniture (page fill and shadow)?
fn furniture(sh: &Shape, pages: &[Rect]) -> bool {
    match sh {
        Shape::Rect(r) => pages.iter().any(|p| (r.rect.min - p.min).length() < 0.5 && (r.rect.max - p.max).length() < 0.5) || (r.fill.a() < 60 && r.fill.r() == 0 && pages.iter().any(|p| (r.rect.size() - p.size()).length() < 0.5)),
        _ => false,
    }
}

/// The PDF of some pages (screen rects) from the shapes, each page `paper` points.
pub fn write_pdf(shapes: &[ClippedShape], pages: &[Rect], paper: (f32, f32)) -> Vec<u8> {
    let mut streams = Vec::new();
    for pr in pages {
        let mut pg = Page { out: String::from("1 J 1 j\n"), origin: pr.min, s: paper.0 / pr.width().max(1.0), h: paper.1 };
        for cs in shapes {
            let b = cs.shape.visual_bounding_rect();
            if !b.is_positive() && !matches!(cs.shape, Shape::LineSegment { .. }) {
                continue;
            }
            if !pr.intersects(b.expand(0.5)) || furniture(&cs.shape, pages) {
                continue;
            }
            pg.shape(&cs.shape);
        }
        streams.push(pg.out);
    }
    // the file: catalog, pages, one content stream per page
    let n = streams.len();
    let mut objs: Vec<String> = Vec::new();
    objs.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    let kids: Vec<String> = (0..n).map(|k| format!("{} 0 R", 3 + 2 * k)).collect();
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")));
    for (k, s) in streams.iter().enumerate() {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.2} {:.2}] /Contents {} 0 R /Resources << >> >>", paper.0, paper.1, 4 + 2 * k));
        objs.push(format!("<< /Length {} >>\nstream\n{s}\nendstream", s.len() + 1));
    }
    let mut out = String::from("%PDF-1.4\n");
    let mut offs = Vec::new();
    for (k, o) in objs.iter().enumerate() {
        offs.push(out.len());
        let _ = write!(out, "{} 0 obj\n{o}\nendobj\n", k + 1);
    }
    let xref = out.len();
    let _ = write!(out, "xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1);
    for o in offs {
        let _ = writeln!(out, "{o:010} 00000 n ");
    }
    let _ = write!(out, "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1);
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_has_a_page_per_rect_and_skips_page_furniture() {
        let page = Rect::from_min_size(Pos2::new(10.0, 10.0), egui::vec2(100.0, 140.0));
        let pages = [page, page.translate(egui::vec2(0.0, 150.0))];
        let shapes = vec![
            ClippedShape::new(Rect::EVERYTHING, Shape::rect_filled(page, 0.0, Color32::WHITE)),
            ClippedShape::new(Rect::EVERYTHING, Shape::line_segment([Pos2::new(20.0, 20.0), Pos2::new(90.0, 20.0)], egui::Stroke::new(1.0, Color32::BLACK))),
            ClippedShape::new(Rect::EVERYTHING, Shape::circle_filled(Pos2::new(50.0, 200.0), 3.0, Color32::BLACK)),
        ];
        let pdf = String::from_utf8(write_pdf(&shapes, &pages, (595.28, 833.0))).unwrap();
        assert!(pdf.starts_with("%PDF-1.4"));
        assert!(pdf.contains("/Count 2"));
        // the line on page 1 (x 10 → 59.53 pt), the circle on page 2, no page fill
        assert!(pdf.contains("59.53 773.47 m"), "{pdf}");
        assert_eq!(pdf.matches(" f\n").count(), 1);
        assert!(!pdf.contains("1.000 1.000 1.000 rg"));
        assert!(pdf.trim_end().ends_with("%%EOF"));
    }
}
