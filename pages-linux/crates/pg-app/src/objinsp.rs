//! The Format inspector for objects (text boxes, shapes, lines, pictures, charts): Keydeck's
//! Style / Text / Arrange / Image panels (kn-app inspector.rs, Keynote 15.4 = Numbers 15.4's
//! object inspectors), applied to the sheet's selected object.

#![allow(clippy::too_many_lines)]

use egui::{Color32, RichText};
use kn_ui::theme;
use pg_core::kn_format;
use pg_core::kn_model;
use pg_core::kn_model::format::{Dash, FillSpec, StrokeSpec, TextProp};
use pg_core::kn_model::{Align, Color, DrawableKind, Fill, Geometry, VAlign};

use crate::app::App;

/// Segmented control like Keynote's: one capsule, the selected tab a blue pill, thin dividers
/// between unselected neighbours.
pub(crate) fn tabs(ui: &mut egui::Ui, labels: &[&str], cur: &mut usize) {
    segmented(ui, labels, cur, 16.0);
}

/// Keynote's inner switches (Style | Layout, Standard | Dynamic): rounded rectangles.
pub(crate) fn seg_small(ui: &mut egui::Ui, labels: &[&str], cur: &mut usize) {
    segmented(ui, labels, cur, 7.0);
}

fn segmented(ui: &mut egui::Ui, labels: &[&str], cur: &mut usize, radius: f32) {
    if *cur >= labels.len() {
        *cur = 0;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::hover());
    let p = ui.painter().clone();
    if kn_ui::glass::enabled() && radius >= 12.0 {
        // the inspector's tabs on liquid glass (no lens: the panel under it is plain)
        for s in kn_ui::glass::shapes(ui.ctx(), rect, radius, false) {
            p.add(s);
        }
    } else {
        p.rect_filled(rect, radius, theme::gray(0x33));
    }
    let w = rect.width() / labels.len() as f32;
    for (i, l) in labels.iter().enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(rect.min.x + w * i as f32, rect.min.y), egui::vec2(w, rect.height()));
        // (the id names the whole switch: the tabs "Style | Text | Arrange" and the Text pane's
        // "Style | Layout" sit in one scope, and a shared ("Style", 0) sent clicks to the wrong one)
        let resp = ui.interact(r, ui.scope_id().with(("seg", labels.join("|"), i)), egui::Sense::click());
        let sel = *cur == i;
        if sel {
            p.rect_filled(r.shrink(1.0), radius - 1.0, theme::ACCENT());
        } else if resp.hovered() {
            p.rect_filled(r.shrink(2.0), radius - 2.0, theme::gray(0x40));
        }
        if i > 0 && !sel && *cur != i - 1 {
            p.line_segment([egui::pos2(r.min.x, r.min.y + 8.0), egui::pos2(r.min.x, r.max.y - 8.0)], egui::Stroke::new(1.0, theme::gray(0x55)));
        }
        theme::centered(&p, r.center(), *l, egui::FontId::proportional(14.0), if sel { Color32::WHITE } else { theme::TEXT() });
        if resp.clicked() {
            *cur = i;
        }
    }
    ui.add_space(10.0);
}

/// Format > Gallery (Keynote 15.4, R29): Title and Caption (one per image or the same for all),
/// the images (pick one to show it, + to add more, − to remove it) and the shown image's
/// description.
fn shown_file_name(stored: &str) -> String {
    let base = stored.rsplit('/').next().unwrap_or(stored);
    let (stem, ext) = base.rsplit_once('.').unwrap_or((base, ""));
    let stem = match stem.rsplit_once('-') {
        Some((s, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => s,
        _ => stem,
    };
    if ext.is_empty() { stem.to_string() } else { format!("{stem}.{ext}") }
}

/// A small picture of a gallery image for the inspector's list (made once).
pub(crate) fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.separator();
    ui.label(theme::bold(title).size(14.0));
    ui.add_space(2.0);
}

fn image_panel(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
    let DrawableKind::Image(i) = &d.kind else { return };
    let id = d.id;
    // Keynote 15.4's Image tab: File Info; Edit Mask | Auto Crop; Remove Background;
    // Adjustments (Exposure, Saturation; Enhance | adjust | Reset); Description
    section(ui, "File Info");
    let name = i.data_id.and_then(|did| app.wb.as_ref().map(|w| &w.doc).and_then(|doc| doc.data_name(did)).map(shown_file_name)).unwrap_or_else(|| "Image".into());
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 18.0), egui::Sense::hover());
        kn_ui::icons::draw(ui.painter(), r, kn_ui::icons::Icon::File, theme::TEXT_DIM());
        ui.add(egui::Label::new(name).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (r, resp) = ui.allocate_exact_size(egui::vec2(32.0, 24.0), egui::Sense::click());
            ui.painter().rect_filled(r, 6.0, if resp.hovered() { theme::gray(0x48) } else { theme::gray(0x38) });
            // a folder: replace the picture with another file
            let f = egui::Rect::from_center_size(r.center(), egui::vec2(14.0, 10.0));
            let st = egui::Stroke::new(1.2, theme::TEXT());
            ui.painter().rect_stroke(f, 1.5, st, egui::StrokeKind::Inside);
            ui.painter().line_segment([f.left_top() + egui::vec2(0.0, 2.0), f.left_top() + egui::vec2(5.0, 2.0)], st);
            if resp.on_hover_text("Replace…").clicked() {
                app.replace_target = Some(id);
                app.pick_image();
            }
        });
    });
    ui.separator();
    // two tiles, an icon over its label
    let tile = |ui: &mut egui::Ui, rect: egui::Rect, label: &str, on: bool, draw: &dyn Fn(&egui::Painter, egui::Pos2, egui::Stroke)| -> bool {
        let resp = ui.interact(rect, ui.scope_id().with(("img-tile", label)), egui::Sense::click());
        let fill = if on { theme::ACCENT() } else if resp.hovered() { theme::gray(0x48) } else { theme::gray(0x38) };
        ui.painter().rect_filled(rect, 6.0, fill);
        draw(ui.painter(), rect.center() - egui::vec2(0.0, 8.0), egui::Stroke::new(1.4, theme::TEXT()));
        ui.painter().text(rect.center() + egui::vec2(0.0, 11.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.5), theme::TEXT());
        resp.clicked()
    };
    let crop = |p: &egui::Painter, c: egui::Pos2, st: egui::Stroke| {
        p.line_segment([c + egui::vec2(-4.0, -8.0), c + egui::vec2(-4.0, 4.0)], st);
        p.line_segment([c + egui::vec2(-4.0, 4.0), c + egui::vec2(8.0, 4.0)], st);
        p.line_segment([c + egui::vec2(-8.0, -4.0), c + egui::vec2(4.0, -4.0)], st);
        p.line_segment([c + egui::vec2(4.0, -4.0), c + egui::vec2(4.0, 8.0)], st);
    };
    let auto = |p: &egui::Painter, c: egui::Pos2, st: egui::Stroke| {
        let r = egui::Rect::from_center_size(c, egui::vec2(14.0, 12.0));
        for (a, b) in [(r.left_top(), egui::vec2(4.0, 0.0)), (r.left_top(), egui::vec2(0.0, 4.0)), (r.right_top(), egui::vec2(-4.0, 0.0)), (r.right_top(), egui::vec2(0.0, 4.0)), (r.left_bottom(), egui::vec2(4.0, 0.0)), (r.left_bottom(), egui::vec2(0.0, -4.0)), (r.right_bottom(), egui::vec2(-4.0, 0.0)), (r.right_bottom(), egui::vec2(0.0, -4.0))] {
            p.line_segment([a, a + b], st);
        }
        p.rect_stroke(r.shrink(3.5), 1.0, st, egui::StrokeKind::Inside);
    };
    let editing = app.mask_edit == Some(id);
    let (row, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 46.0), egui::Sense::hover());
    let half = (row.width() - 8.0) / 2.0;
    let left = egui::Rect::from_min_size(row.min, egui::vec2(half, row.height()));
    let right = egui::Rect::from_min_size(row.min + egui::vec2(half + 8.0, 0.0), egui::vec2(half, row.height()));
    if tile(ui, left, if editing { "Done" } else { "Edit Mask" }, editing, &crop) {
        app.command(ui.ctx(), "Edit Mask");
    }
    if tile(ui, right, "Auto Crop", false, &auto) {
        app.command(ui.ctx(), "Auto Crop");
    }
    ui.add_space(6.0);
    let full = |ui: &mut egui::Ui, label: &str| ui.add_sized([ui.available_width(), 26.0], egui::Button::new(label)).clicked();
    if full(ui, "Remove Background") {
        app.command(ui.ctx(), "Remove Background");
    }
    if i.mask.is_some() && full(ui, "Reset Mask") {
        app.command(ui.ctx(), "Reset Mask");
    }
    section(ui, "Adjustments");
    let mut a = app.wb.as_ref().map(|w| &w.doc).map(|doc| doc.image_adjustments(id)).unwrap_or_default();
    let mut done = false;
    // a slider with its value in a field and a stepper beside it
    let adjust = |ui: &mut egui::Ui, label: &str, v: &mut f32| -> bool {
        ui.label(label);
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 96.0).max(60.0);
            let r = ui.add(egui::Slider::new(v, -1.0..=1.0).show_value(false));
            changed |= r.drag_stopped() || (r.changed() && !r.dragged());
            let mut pct = f64::from(*v * 100.0);
            if theme::stepper(ui, &mut pct, 1.0, -100.0..=100.0, "%", 0) {
                *v = pct as f32 / 100.0;
                changed = true;
            }
        });
        changed
    };
    done |= adjust(ui, "Exposure", &mut a.exposure);
    done |= adjust(ui, "Saturation", &mut a.saturation);
    let (row, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::hover());
    let w3 = (row.width() - 16.0 - 36.0) / 2.0;
    let enh = egui::Rect::from_min_size(row.min, egui::vec2(w3, 26.0));
    let adj = egui::Rect::from_min_size(row.min + egui::vec2(w3 + 8.0, 0.0), egui::vec2(36.0, 26.0));
    let rst = egui::Rect::from_min_size(row.min + egui::vec2(w3 + 52.0, 0.0), egui::vec2(w3, 26.0));
    if ui.put(enh, egui::Button::new("Enhance").selected(a.enhance)).clicked() {
        a.enhance = !a.enhance;
        done = true;
    }
    let ar = ui.put(adj, egui::Button::new(""));
    kn_ui::icons::draw(ui.painter(), egui::Rect::from_center_size(adj.center(), egui::vec2(16.0, 16.0)), kn_ui::icons::Icon::Sliders, theme::TEXT());
    if ar.on_hover_text("Adjust Image").clicked() {
        app.notify("Adjust Image isn't available yet.");
    }
    if ui.put(rst, egui::Button::new("Reset")).clicked() && !a.is_default() {
        a = Default::default();
        done = true;
    }
    if done {
        crate::imageadj::set(app, id, a);
    }
    if let Some(eq) = &i.equation {
        section(ui, "Equation");
        ui.code(eq);
    }
    section(ui, "Description");
    if app.desc_buf.as_ref().is_none_or(|(o, _)| *o != id) {
        app.desc_buf = Some((id, d.alt_text.clone().unwrap_or_default()));
    }
    let mut commit = false;
    if let Some((_, buf)) = app.desc_buf.as_mut() {
        let r = ui.add(egui::TextEdit::multiline(buf).hint_text("Describe this image for people who use assistive technology").desired_rows(5).desired_width(f32::INFINITY));
        commit = r.lost_focus() && buf.as_str() != d.alt_text.as_deref().unwrap_or("");
    }
    if commit {
        let text = app.desc_buf.as_ref().map(|(_, b)| b.clone()).unwrap_or_default();
        let slide = app.current_slide().map(|s| s.slide);
        if let Some(doc) = app.wb.as_mut().map(|w| &mut w.doc) {
            doc.begin(&mut app.hist, "Description");
            doc.set_description(&mut app.hist, id, &text);
            doc.end(&mut app.hist);
        }
        if let Some(s) = slide {
            app.bump(s);
        }
    }
}

pub(crate) fn c32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied((c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8, (c.a * 255.0).round() as u8)
}

pub(crate) fn from_c32(c: Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: a as f32 / 255.0 }
}

/// Colour well (Numbers': presets in a popover, and a wheel button for the Colors window;
/// kn-ui's, shared with Keydeck); returns the new colour when the user changed it.
pub(crate) fn color_button(ui: &mut egui::Ui, c: Color) -> Option<Color> {
    let mut v = c32(c);
    let id = ui.next_auto_id();
    kn_ui::colors::color_well(ui, id, &mut v, 64.0).then(|| from_c32(v))
}

/// Fill editor as Keynote's (No Fill / Color / Gradient / Image / Advanced Gradient Fill);
/// returns a new fill when changed. `doc` takes the picture for an Image Fill.
pub(crate) fn fill_editor(ui: &mut egui::Ui, salt: &str, fill: &Fill, doc: Option<&mut kn_model::Document>) -> Option<FillSpec> {
    use kn_model::ImageFillTechnique as T;
    let cur = crate::editing::fill_spec(fill);
    // Keynote's order: No Fill, Color, Gradient, Image, Advanced Image (tinted), Advanced Gradient
    let kind = match &cur {
        FillSpec::None => 0,
        FillSpec::Color(_) => 1,
        FillSpec::Gradient { .. } => 2,
        FillSpec::Image { tint: None, .. } => 3,
        FillSpec::Image { .. } => 4,
        FillSpec::Advanced { .. } => 5,
    };
    let names = ["No Fill", "Color Fill", "Gradient Fill", "Image Fill", "Advanced Image Fill", "Advanced Gradient Fill"];
    let mut k = kind;
    let mut out = None;
    theme::combo(salt).selected_text(names[kind]).width(ui.available_width()).show_ui(ui, |ui| {
        for (i, n) in names.iter().enumerate() {
            ui.selectable_value(&mut k, i, *n);
        }
    });
    let base = match &cur {
        FillSpec::Color(c) => *c,
        FillSpec::Gradient { from, .. } => *from,
        FillSpec::Advanced { stops, .. } => stops.first().map(|s| s.1).unwrap_or(Color::WHITE),
        _ => Color { r: 0.0, g: 0.63, b: 1.0, a: 1.0 },
    };
    // a picture from disk into the document: (data, pixel size)
    let mut doc = doc;
    let pick = |doc: &mut Option<&mut kn_model::Document>| -> Option<(u64, (f32, f32))> {
        let path = rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg"]).set_title("Choose Image").pick_file()?;
        let bytes = std::fs::read(&path).ok()?;
        let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok()?.into_dimensions().ok()?;
        let name = path.file_name()?.to_string_lossy().to_string();
        let id = doc.as_mut()?.file.add_image_data(bytes, &name).ok()?;
        Some((id, (w as f32, h as f32)))
    };
    if k != kind {
        out = match k {
            0 => Some(FillSpec::None),
            1 => Some(FillSpec::Color(base)),
            2 => Some(FillSpec::Gradient { from: base, to: Color::WHITE, angle: 270.0 }),
            3 | 4 => {
                // Advanced Image Fill: the picture with Keynote's default translucent blue tint
                let tint = (k == 4).then_some(Color { r: 0.0, g: 0.48, b: 1.0, a: 0.35 });
                match &cur {
                    FillSpec::Image { data_id, technique, size, reference, .. } => Some(FillSpec::Image { data_id: *data_id, technique: *technique, size: *size, reference: *reference, tint }),
                    _ => pick(&mut doc).map(|(data_id, size)| FillSpec::Image { data_id, technique: T::ScaleToFill, size: Some(size), reference: None, tint }),
                }
            }
            _ => Some(match &cur {
                FillSpec::Gradient { from, to, angle } => FillSpec::Advanced { stops: vec![(0.0, *from, 0.5), (1.0, *to, 0.5)], radial: false, angle: *angle },
                _ => FillSpec::Advanced { stops: vec![(0.0, base, 0.5), (1.0, Color::WHITE, 0.5)], radial: false, angle: 270.0 },
            }),
        };
    }
    match &cur {
        FillSpec::Color(c) if k == 1 => {
            ui.horizontal(|ui| {
                if let Some(n) = color_button(ui, *c) {
                    out = Some(FillSpec::Color(n));
                }
            });
        }
        FillSpec::Gradient { from, to, angle } if k == 2 => {
            ui.horizontal(|ui| {
                let (mut f, mut t, mut a) = (*from, *to, *angle);
                let mut changed = false;
                if let Some(n) = color_button(ui, f) {
                    f = n;
                    changed = true;
                }
                if let Some(n) = color_button(ui, t) {
                    t = n;
                    changed = true;
                }
                if ui.button("⇄").on_hover_text("Reverse colours").clicked() {
                    std::mem::swap(&mut f, &mut t);
                    changed = true;
                }
                changed |= theme::stepper32(ui, &mut a, 1.0, 0.0..=360.0, "°", 0);
                if changed {
                    out = Some(FillSpec::Gradient { from: f, to: t, angle: a });
                }
            });
        }
        FillSpec::Advanced { stops, radial, angle } if k == 5 => {
            if let Some(n) = advanced_gradient_editor(ui, salt, stops, *radial, *angle) {
                out = Some(n);
            }
        }
        FillSpec::Image { data_id, technique, size, reference, tint } if k == 3 || k == 4 => {
            let (data_id, technique, reference, tint) = (*data_id, *technique, *reference, *tint);
            // the picture's own size (Scale is the fill size against it)
            let key = ui.scope_id().with(("fill-image-size", data_id));
            let natural = ui.data(|m| m.get_temp::<Option<(f32, f32)>>(key)).unwrap_or_else(|| {
                let n = doc.as_ref().and_then(|d| d.data(data_id)).and_then(|b| image::ImageReader::new(std::io::Cursor::new(b.to_vec())).with_guessed_format().ok()?.into_dimensions().ok()).map(|(w, h)| (w as f32, h as f32));
                ui.data_mut(|m| m.insert_temp(key, n));
                n
            });
            let techniques = [(T::Natural, "Original Size"), (T::Stretch, "Stretch"), (T::Tile, "Tile"), (T::ScaleToFill, "Scale to Fill"), (T::ScaleToFit, "Scale to Fit")];
            ui.horizontal(|ui| {
                let mut tq = technique;
                let label = techniques.iter().find(|t| t.0 == technique).map(|t| t.1).unwrap_or("Scale to Fill");
                theme::combo(&format!("{salt}-technique")).selected_text(label).width(ui.available_width() - 110.0).show_ui(ui, |ui| {
                    for (t, n) in techniques {
                        ui.selectable_value(&mut tq, t, n);
                    }
                });
                if tq != technique {
                    out = Some(FillSpec::Image { data_id, technique: tq, size: *size, reference, tint });
                }
                if let Some(t) = tint {
                    if let Some(c) = color_button(ui, t) {
                        // the fallback colour: the picture's own average under the new tint
                        let base = reference.map(|r| {
                            let a = t.a.clamp(0.0, 0.99);
                            let un = |v: f32, tv: f32| ((v - tv * a) / (1.0 - a)).clamp(0.0, 1.0);
                            Color { r: un(r.r, t.r), g: un(r.g, t.g), b: un(r.b, t.b), a: 1.0 }
                        });
                        let reference = base.map(|b| Color { r: b.r + (c.r - b.r) * c.a, g: b.g + (c.g - b.g) * c.a, b: b.b + (c.b - b.b) * c.a, a: 1.0 });
                        out = Some(FillSpec::Image { data_id, technique, size: *size, reference, tint: Some(c) });
                    }
                }
                if ui.button("Choose…").clicked() {
                    if let Some((d, s)) = pick(&mut doc) {
                        out = Some(FillSpec::Image { data_id: d, technique, size: Some(s), reference: None, tint });
                    }
                }
            });
            if technique == T::Tile || technique == T::Natural {
                if let Some((nw, nh)) = natural {
                    let mut pct = size.map(|s| s.0 / nw.max(1.0) * 100.0).unwrap_or(100.0);
                    ui.horizontal(|ui| {
                        ui.label("Scale");
                        if ui.add(egui::Slider::new(&mut pct, 1.0..=400.0).suffix("%").integer()).changed() {
                            let f = pct / 100.0;
                            out = Some(FillSpec::Image { data_id, technique, size: Some(((nw * f).round(), (nh * f).round())), reference, tint });
                        }
                    });
                }
            }
        }
        _ => {}
    }
    out
}

/// Keynote's Advanced Gradient: a bar with colour stops under it (drag to move, click the bar
/// to add one, drag one away to remove it) and inflection diamonds over it; Linear | Radial,
/// reverse and angle.
fn advanced_gradient_editor(ui: &mut egui::Ui, salt: &str, stops: &[(f32, Color, f32)], radial: bool, angle: f32) -> Option<FillSpec> {
    let key = ui.scope_id().with((salt, "adv-grad"));
    let sel_key = key.with("sel");
    // while dragging, a working copy is drawn and only written when the drag ends
    let mut work: Vec<(f32, Color, f32)> = ui.data(|m| m.get_temp::<Vec<(f32, Color, f32)>>(key)).unwrap_or_else(|| stops.to_vec());
    let mut sel = ui.data(|m| m.get_temp::<usize>(sel_key)).unwrap_or(0).min(work.len().saturating_sub(1));
    let mut out = None;
    let mut commit = false;
    let mut dragging = false;
    let lerp = |a: Color, b: Color, t: f32| Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t };
    let color_at = |s: &[(f32, Color, f32)], x: f32| -> Color {
        if x <= s[0].0 {
            return s[0].1;
        }
        for w in s.windows(2) {
            if x <= w[1].0 {
                let t = ((x - w[0].0) / (w[1].0 - w[0].0).max(1e-4)).clamp(0.0, 1.0);
                // the inflection is where the colour is halfway
                let m = w[0].2.clamp(0.01, 0.99);
                let u = if t < m { t / m * 0.5 } else { 0.5 + (t - m) / (1.0 - m) * 0.5 };
                return lerp(w[0].1, w[1].1, u);
            }
        }
        s[s.len() - 1].1
    };
    let width = ui.available_width();
    let (outer, _) = ui.allocate_exact_size(egui::vec2(width, 44.0), egui::Sense::hover());
    let bar = egui::Rect::from_min_max(egui::pos2(outer.min.x + 8.0, outer.min.y + 10.0), egui::pos2(outer.max.x - 8.0, outer.min.y + 30.0));
    let xf = |f: f32| bar.min.x + f * bar.width();
    // the bar
    {
        let n = 64;
        let mut mesh = egui::Mesh::default();
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let c = c32(color_at(&work, t));
            let x = bar.min.x + t * bar.width();
            mesh.colored_vertex(egui::pos2(x, bar.min.y), c);
            mesh.colored_vertex(egui::pos2(x, bar.max.y), c);
            if i > 0 {
                let b = (i * 2) as u32;
                mesh.add_triangle(b - 2, b - 1, b);
                mesh.add_triangle(b - 1, b, b + 1);
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().rect_stroke(bar, 3.0, egui::Stroke::new(1.0, theme::gray(0x80)), egui::StrokeKind::Inside);
    }
    let bar_resp = ui.interact(bar, key.with("bar"), egui::Sense::click());
    if bar_resp.clicked() {
        if let Some(p) = bar_resp.interact_pointer_pos() {
            let f = ((p.x - bar.min.x) / bar.width()).clamp(0.0, 1.0);
            let c = color_at(&work, f);
            let at = work.iter().position(|s| s.0 > f).unwrap_or(work.len());
            work.insert(at, (f, c, 0.5));
            sel = at;
            commit = true;
        }
    }
    // inflection diamonds, between neighbouring stops
    for i in 0..work.len().saturating_sub(1) {
        let (a, b) = (work[i].0, work[i + 1].0);
        let x = xf(a + (b - a) * work[i].2);
        let r = egui::Rect::from_center_size(egui::pos2(x, bar.min.y - 4.0), egui::vec2(10.0, 10.0));
        let resp = ui.interact(r, key.with(("infl", i)), egui::Sense::drag());
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                let f = ((p.x - bar.min.x) / bar.width()).clamp(a, b);
                work[i].2 = ((f - a) / (b - a).max(1e-4)).clamp(0.01, 0.99);
                dragging = true;
            }
        }
        commit |= resp.drag_stopped();
        let c = r.center();
        let pts = vec![egui::pos2(c.x, c.y - 4.0), egui::pos2(c.x + 4.0, c.y), egui::pos2(c.x, c.y + 4.0), egui::pos2(c.x - 4.0, c.y)];
        ui.painter().add(egui::Shape::convex_polygon(pts, theme::gray(0x90), egui::Stroke::new(1.0, theme::gray(0x50))));
    }
    // colour stops
    let mut remove = None;
    for i in 0..work.len() {
        let x = xf(work[i].0);
        let r = egui::Rect::from_center_size(egui::pos2(x, bar.max.y + 4.0), egui::vec2(12.0, 14.0));
        let resp = ui.interact(r, key.with(("stop", i)), egui::Sense::click_and_drag());
        if resp.clicked() || resp.drag_started() {
            sel = i;
        }
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                let lo = if i > 0 { work[i - 1].0 } else { 0.0 };
                let hi = if i + 1 < work.len() { work[i + 1].0 } else { 1.0 };
                work[i].0 = ((p.x - bar.min.x) / bar.width()).clamp(lo, hi);
                dragging = true;
                // pulled well away from the bar: it goes (two stops always stay)
                if p.y > bar.max.y + 40.0 && work.len() > 2 {
                    remove = Some(i);
                }
            }
        }
        commit |= resp.drag_stopped();
        let fill = c32(work[i].1);
        ui.painter().rect_filled(r, 2.0, fill);
        let stroke = if i == sel { egui::Stroke::new(2.0, theme::ACCENT()) } else { egui::Stroke::new(1.0, theme::gray(0x60)) };
        ui.painter().rect_stroke(r, 2.0, stroke, egui::StrokeKind::Outside);
    }
    if let Some(i) = remove {
        work.remove(i);
        sel = sel.min(work.len() - 1);
        commit = true;
    }
    let (mut rad, mut ang) = (radial, angle);
    // the selected stop's colour, then Keynote's row: Linear | Radial, reverse, Angle
    ui.horizontal(|ui| {
        ui.label("Color");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(c) = color_button(ui, work[sel].1) {
                work[sel].1 = c;
                commit = true;
            }
        });
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let small = |t: &str| egui::RichText::new(t).size(11.0);
        if ui.add(egui::Button::new(small("Linear")).selected(!rad)).clicked() && rad {
            rad = false;
            commit = true;
        }
        if ui.add(egui::Button::new(small("Radial")).selected(rad)).clicked() && !rad {
            rad = true;
            commit = true;
        }
        if ui.button("⇄").on_hover_text("Reverse").clicked() {
            work = work.iter().rev().enumerate().map(|(i, s)| (1.0 - s.0, s.1, 1.0 - work.get(work.len().wrapping_sub(i + 2)).map(|p| p.2).unwrap_or(0.5))).collect();
            commit = true;
        }
        if !rad {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                commit |= theme::stepper32(ui, &mut ang, 1.0, 0.0..=360.0, "°", 0);
                ui.label("Angle");
            });
        }
    });
    ui.data_mut(|m| {
        m.insert_temp(sel_key, sel);
        if dragging && !commit {
            m.insert_temp(key, work.clone());
        } else {
            m.remove::<Vec<(f32, Color, f32)>>(key);
        }
    });
    if commit {
        out = Some(FillSpec::Advanced { stops: work, radial: rad, angle: ang });
    }
    out
}

/// Border editor; returns Some(new) when changed (Some(None) = no border).
pub(crate) fn stroke_editor(ui: &mut egui::Ui, salt: &str, stroke: &Option<kn_model::Stroke>, allow_none: bool) -> Option<Option<StrokeSpec>> {
    let cur = stroke.as_ref().map(|s| StrokeSpec {
        color: s.color,
        width: s.width,
        dash: if s.dash.is_empty() { Dash::Solid } else if s.dash[0] < 0.5 { Dash::Dotted } else { Dash::Dashed },
    });
    let mut out = None;
    let names = ["No Border", "Line"];
    let kind = usize::from(cur.is_some());
    let mut k = kind;
    if allow_none {
        theme::combo(salt).selected_text(names[kind]).width(ui.available_width()).show_ui(ui, |ui| {
            for (i, n) in names.iter().enumerate() {
                ui.selectable_value(&mut k, i, *n);
            }
        });
    }
    if k != kind {
        out = Some((k == 1).then(|| StrokeSpec { color: Color::BLACK, width: 1.0, dash: Dash::Solid }));
    }
    let Some(mut s) = cur.or((!allow_none).then(|| StrokeSpec { color: Color::BLACK, width: 1.0, dash: Dash::Solid })) else { return out };
    if k == 0 {
        return out;
    }
    let mut changed = false;
    ui.horizontal(|ui| {
        theme::combo((salt, "dash"))
            .selected_text(match s.dash {
                Dash::Solid => "Solid",
                Dash::Dashed => "Dashed",
                Dash::Dotted => "Dotted",
            })
            .width(90.0)
            .show_ui(ui, |ui| {
                for (d, n) in [(Dash::Solid, "Solid"), (Dash::Dashed, "Dashed"), (Dash::Dotted, "Dotted")] {
                    changed |= ui.selectable_value(&mut s.dash, d, n).changed();
                }
            });
        if let Some(c) = color_button(ui, s.color) {
            s.color = c;
            changed = true;
        }
        changed |= theme::stepper32(ui, &mut s.width, 0.5, 0.0..=100.0, " pt", 1);
    });
    if changed {
        out = Some(Some(s));
    }
    out
}

/// Format > Style, laid out like Keynote 15.4: collapsible Fill / Border / Shadow sections with
/// swatches, Reflection, Opacity (slider + stepper), Title / Caption.
fn style(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
    use theme::Swatch;
    let mut fill_change = None;
    let mut stroke_change = None;
    let mut ends_change = None;
    let fill_swatch = |f: &Fill| match f {
        Fill::Color(c) => Swatch::Color(c32(*c)),
        Fill::Gradient { stops, .. } => stops.first().map(|s| Swatch::Color(c32(s.1))).unwrap_or(Swatch::None),
        Fill::None => Swatch::None,
        #[allow(unreachable_patterns)]
        _ => Swatch::Color(theme::gray(128)),
    };
    // theme presets grid ("Shape Styles" / "Text Styles"): 6 per page, arrows + page dots
    let mut preset_pick: Option<kn_format::ObjId> = None;
    if let DrawableKind::Shape(s) = &d.kind {
        if !s.is_line {
            let textbox = s.is_text_box;
            let presets = app.wb.as_ref().map(|w| &w.doc).map(|doc| doc.shape_style_presets(textbox)).unwrap_or_default();
            if !presets.is_empty() {
                let pages = presets.len().div_ceil(6);
                let key = ui.scope_id().with("preset-page");
                let mut page = ui.data(|m| m.get_temp::<usize>(key)).unwrap_or(0).min(pages - 1);
                ui.horizontal(|ui| {
                    if ui.add_enabled(page > 0, egui::Button::new(RichText::new("‹").size(22.0)).frame(false)).clicked() {
                        page -= 1;
                    }
                    // leave room for the › arrow and spacing so the row fits the panel exactly
                    let w = ui.available_width() - 40.0;
                    let cell = egui::vec2((w / 3.0).min(96.0), 60.0);
                    ui.vertical(|ui| {
                        for row in 0..2 {
                            ui.horizontal(|ui| {
                                for col in 0..3 {
                                    let i = page * 6 + row * 3 + col;
                                    let (r, resp) = ui.allocate_exact_size(cell, egui::Sense::click());
                                    let Some(&pid) = presets.get(i) else { continue };
                                    let g = app.wb.as_ref().map(|w| &w.doc).map(|doc| kn_model::style::graphic(&doc.file, Some(pid)));
                                    let inner = r.shrink(7.0);
                                    match g.as_ref().map(|g| &g.fill) {
                                        Some(Fill::Color(c)) => {
                                            ui.painter().rect_filled(inner, 3.0, c32(*c));
                                        }
                                        Some(Fill::Gradient { stops, .. }) if !stops.is_empty() => {
                                            ui.painter().rect_filled(inner, 3.0, c32(stops[0].1));
                                        }
                                        _ => {
                                            ui.painter().rect_filled(inner, 3.0, theme::gray(0x2a));
                                            theme::centered(ui.painter(), inner.center(), "Aa", egui::FontId::proportional(16.0), theme::TEXT());
                                        }
                                    }
                                    if let Some(st) = g.as_ref().and_then(|g| g.stroke.as_ref()) {
                                        ui.painter().rect_stroke(inner, 3.0, egui::Stroke::new(2.0, c32(st.color)), egui::StrokeKind::Inside);
                                    }
                                    if resp.hovered() {
                                        ui.painter().rect_stroke(r.shrink(3.0), 5.0, egui::Stroke::new(1.5, theme::ACCENT()), egui::StrokeKind::Inside);
                                    }
                                    if resp.clicked() {
                                        preset_pick = Some(pid);
                                    }
                                }
                            });
                        }
                    });
                    if ui.add_enabled(page + 1 < pages, egui::Button::new(RichText::new("›").size(22.0)).frame(false)).clicked() {
                        page += 1;
                    }
                });
                ui.data_mut(|m| m.insert_temp(key, page));
                ui.vertical_centered(|ui| {
                    ui.label(theme::bold(if textbox { "Text Styles" } else { "Shape Styles" }));
                    let dots: String = (0..pages).map(|i| if i == page { "● " } else { "○ " }).collect();
                    ui.label(RichText::new(dots.trim_end()).size(8.0).color(theme::TEXT_DIM()));
                });
            }
        }
    }
    // pictures: the theme's "Image Styles", each shown as this picture in that style
    if let DrawableKind::Image(im) = &d.kind {
        let presets = app.wb.as_ref().map(|w| &w.doc).map(|doc| doc.image_style_presets()).unwrap_or_default();
        if !presets.is_empty() && im.gallery.len() < 2 {
            let thumb = im.data_id.and_then(|data| gallery_thumb(app, ui.ctx(), data));
            let pages = presets.len().div_ceil(6);
            let key = ui.scope_id().with("image-preset-page");
            let mut page = ui.data(|m| m.get_temp::<usize>(key)).unwrap_or(0).min(pages - 1);
            ui.horizontal(|ui| {
                if ui.add_enabled(page > 0, egui::Button::new(RichText::new("‹").size(22.0)).frame(false)).clicked() {
                    page -= 1;
                }
                let w = ui.available_width() - 40.0;
                let cell = egui::vec2((w / 3.0).min(96.0), 60.0);
                ui.vertical(|ui| {
                    for row in 0..2 {
                        ui.horizontal(|ui| {
                            for col in 0..3 {
                                let i = page * 6 + row * 3 + col;
                                let (r, resp) = ui.allocate_exact_size(cell, egui::Sense::click());
                                let Some(&pid) = presets.get(i) else { continue };
                                let g = app.wb.as_ref().map(|w| &w.doc).map(|doc| kn_model::style::graphic(&doc.file, Some(pid)));
                                let inner = r.shrink(9.0);
                                let p = ui.painter();
                                if g.as_ref().is_some_and(|g| g.shadow.is_some()) {
                                    p.rect_filled(inner.translate(egui::vec2(2.0, 2.0)), 1.0, Color32::from_black_alpha(110));
                                }
                                match &thumb {
                                    Some(t) => {
                                        p.image(t.id(), inner, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                                    }
                                    None => {
                                        p.rect_filled(inner, 1.0, theme::gray(0x60));
                                    }
                                }
                                if let Some(st) = g.as_ref().and_then(|g| g.stroke.as_ref()) {
                                    p.rect_stroke(inner, 0.0, egui::Stroke::new(st.width.clamp(1.0, 4.0), c32(st.color)), egui::StrokeKind::Outside);
                                }
                                if g.as_ref().is_some_and(|g| g.reflection.is_some()) {
                                    let refl = egui::Rect::from_min_size(egui::pos2(inner.min.x, inner.max.y + 1.0), egui::vec2(inner.width(), 6.0));
                                    p.rect_filled(refl, 0.0, Color32::from_white_alpha(30));
                                }
                                if resp.hovered() {
                                    p.rect_stroke(r.shrink(2.0), 5.0, egui::Stroke::new(1.5, theme::ACCENT()), egui::StrokeKind::Inside);
                                }
                                if resp.clicked() {
                                    preset_pick = Some(pid);
                                }
                            }
                        });
                    }
                });
                if ui.add_enabled(page + 1 < pages, egui::Button::new(RichText::new("›").size(22.0)).frame(false)).clicked() {
                    page += 1;
                }
            });
            ui.data_mut(|m| m.insert_temp(key, page));
            ui.vertical_centered(|ui| {
                ui.label(theme::bold("Image Styles"));
                let dots: String = (0..pages).map(|i| if i == page { "● " } else { "○ " }).collect();
                ui.label(RichText::new(dots.trim_end()).size(8.0).color(theme::TEXT_DIM()));
            });
        }
    }
    if let Some(pid) = preset_pick {
        crate::editing::apply(app, "Style", |doc, h, id| doc.apply_shape_style(h, id, pid));
    }
    match &d.kind {
        DrawableKind::Shape(s) => {
            if !s.is_line && theme::disclosure(ui, "fill", "Fill", Some(fill_swatch(&s.fill))) {
                fill_change = fill_editor(ui, "fill", &s.fill, app.wb.as_mut().map(|w| &mut w.doc));
            }
            let sw = s.stroke.as_ref().map(|st| Swatch::Color(c32(st.color))).unwrap_or(Swatch::None);
            if theme::disclosure(ui, "border", if s.is_line { "Stroke" } else { "Border" }, Some(sw)) {
                stroke_change = stroke_editor(ui, "stroke", &s.stroke, !s.is_line);
                if s.is_line {
                    let (mut tail, mut head) = (s.tail.is_some(), s.head.is_some());
                    ui.horizontal(|ui| {
                        let a = theme::checkbox(ui, &mut tail, "Start arrow").changed();
                        let b = theme::checkbox(ui, &mut head, "End arrow").changed();
                        if a || b {
                            ends_change = Some((tail, head));
                        }
                    });
                }
            }
        }
        DrawableKind::Image(i) => {
            let sw = i.stroke.as_ref().map(|st| Swatch::Color(c32(st.color))).unwrap_or(Swatch::None);
            if theme::disclosure(ui, "border", "Border", Some(sw)) {
                stroke_change = stroke_editor(ui, "stroke", &i.stroke, true);
            }
        }
        _ => {}
    }
    // Shadow (Keynote 15.4): No Shadow / Drop / Contact / Curved, each with its settings
    let mut shadow_spec: Option<Option<kn_model::Shadow>> = None;
    let sw = if d.shadow.is_some() { Swatch::Color(theme::gray(40)) } else { Swatch::None };
    if theme::disclosure(ui, "shadow", "Shadow", Some(sw)) {
        let kinds = ["No Shadow", "Drop Shadow", "Contact Shadow", "Curved Shadow"];
        let cur = d.shadow.as_ref().map(|s| usize::from(s.kind) + 1).unwrap_or(0);
        let mut k = cur;
        theme::combo("shadow-kind").selected_text(kinds[cur.min(3)]).width(ui.available_width()).show_ui(ui, |ui| {
            for (i, n) in kinds.iter().enumerate() {
                ui.selectable_value(&mut k, i, *n);
            }
        });
        let defaults = |kind: u8| kn_model::Shadow {
            color: Color::BLACK,
            angle: 315.0,
            offset: if kind == 0 { 5.0 } else { 0.0 },
            blur: if kind == 1 { 6.0 } else { 10.0 },
            opacity: if kind == 0 { 0.75 } else { 0.5 },
            kind,
            height: 0.2,
            curve: 0.6,
        };
        if k != cur {
            shadow_spec = Some((k > 0).then(|| {
                let mut s = d.shadow.clone().unwrap_or_else(|| defaults(k as u8 - 1));
                if s.kind != k as u8 - 1 {
                    s = defaults(k as u8 - 1);
                }
                s
            }));
        } else if let Some(sh) = d.shadow.clone() {
            let mut s = sh.clone();
            let mut done = false;
            let row = |ui: &mut egui::Ui, label: &str, v: &mut f32, step: f64, range: std::ops::RangeInclusive<f64>, suffix: &str, scale: f32| -> bool {
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut x = f64::from(*v * scale);
                        if theme::stepper(ui, &mut x, step, range, suffix, 0) {
                            *v = x as f32 / scale;
                            changed = true;
                        }
                    });
                });
                changed
            };
            if s.kind != 2 {
                done |= row(ui, "Blur", &mut s.blur, 1.0, 0.0..=100.0, " pt", 1.0);
            }
            done |= row(ui, "Offset", &mut s.offset, 1.0, 0.0..=100.0, " pt", 1.0);
            done |= row(ui, "Opacity", &mut s.opacity, 1.0, 0.0..=100.0, "%", 100.0);
            match s.kind {
                0 => done |= row(ui, "Angle", &mut s.angle, 1.0, 0.0..=359.0, "°", 1.0),
                // Keynote shows the contact shadow's height as a perspective angle (R38: 0.25 = 14.5°)
                1 => {
                    let mut deg = s.height.clamp(0.0, 1.0).asin().to_degrees();
                    if row(ui, "Perspective", &mut deg, 1.0, 1.0..=90.0, "°", 1.0) {
                        s.height = deg.to_radians().sin();
                        done = true;
                    }
                }
                _ => done |= row(ui, "Curvature", &mut s.curve, 1.0, 0.0..=100.0, "%", 100.0),
            }
            ui.horizontal(|ui| {
                ui.label("Color");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(c) = color_button(ui, s.color) {
                        s.color = c;
                        done = true;
                    }
                });
            });
            if done {
                shadow_spec = Some(Some(s));
            }
        }
    }
    ui.add_space(4.0);
    ui.separator();
    // Reflection, with its strength
    let mut refl = d.reflection.is_some();
    let mut refl_amount: Option<f32> = None;
    if theme::checkbox(ui, &mut refl, theme::bold("Reflection").size(15.0)).changed() {
        refl_amount = Some(if refl { 0.5 } else { 0.0 });
    }
    if let Some(cur) = d.reflection {
        let mut v = f64::from(cur * 100.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 96.0).max(60.0);
            let r = ui.add(egui::Slider::new(&mut v, 0.0..=100.0).show_value(false));
            let stepped = theme::stepper(ui, &mut v, 1.0, 0.0..=100.0, "%", 0);
            if r.drag_stopped() || (r.changed() && !r.dragged()) || stepped {
                refl_amount = Some((v as f32 / 100.0).max(0.01));
            }
        });
    }
    ui.add_space(4.0);
    ui.separator();
    ui.label(theme::bold("Opacity").size(15.0));
    let mut op = (d.opacity * 100.0) as f64;
    let mut op_changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().slider_width = (ui.available_width() - 96.0).max(60.0);
        op_changed |= ui.add(egui::Slider::new(&mut op, 0.0..=100.0).show_value(false)).changed();
        op_changed |= theme::stepper(ui, &mut op, 1.0, 0.0..=100.0, "%", 0);
    });
    let op = op as f32;
    ui.add_space(4.0);
    ui.separator();
    // Title / Caption of the object (shown above / below it)
    {
        let shown = |field: &str, hidden: &str| {
            app.wb.as_ref().map(|w| &w.doc).and_then(|doc| doc.file.obj(d.id)).and_then(|m| m.find_super("TSD.DrawableArchive")).is_some_and(|dm| {
                dm.bool(hidden) == Some(false) && dm.ref_id(field).is_some_and(|c| app.wb.as_ref().map(|w| &w.doc).is_some_and(|doc| doc.file.type_name(c) == "TSA.CaptionInfoArchive"))
            })
        };
        let (mut t, mut c) = (shown("title", "title_hidden"), shown("caption", "caption_hidden"));
        let mut change = None;
        if theme::checkbox(ui, &mut t, "Title").changed() {
            change = Some((true, t));
        }
        if theme::checkbox(ui, &mut c, "Caption").changed() {
            change = Some((false, c));
        }
        if let (Some((title, on)), Some(slide)) = (change, app.current_slide().map(|s| s.slide)) {
            let id = d.id;
            if let Some(doc) = app.wb.as_mut().map(|w| &mut w.doc) {
                doc.begin(&mut app.hist, if title { "Title" } else { "Caption" });
                doc.set_caption(&mut app.hist, slide, id, title, on);
                doc.end(&mut app.hist);
            }
            app.bump(slide);
        }
    }

    if let Some(f) = fill_change {
        crate::editing::apply(app, "Fill", |doc, h, id| doc.set_fill(h, id, &f));
    }
    if let Some(s) = stroke_change {
        crate::editing::apply(app, "Border", |doc, h, id| doc.set_stroke(h, id, s.as_ref()));
    }
    if let Some((tail, head)) = ends_change {
        let end = kn_model::insert::arrow_line_end();
        crate::editing::apply(app, "Arrows", |doc, h, id| doc.set_line_ends(h, id, tail.then_some(&end), head.then_some(&end)));
    }
    if let Some(spec) = shadow_spec {
        crate::editing::apply(app, "Shadow", |doc, h, id| doc.set_shadow_spec(h, id, spec.as_ref()));
    }
    if let Some(a) = refl_amount {
        crate::editing::apply(app, "Reflection", |doc, h, id| doc.set_reflection_amount(h, id, a));
    }
    if op_changed {
        crate::editing::apply(app, "Opacity", |doc, h, id| doc.set_opacity(h, id, op / 100.0));
    }
}


/// Segmented control of icons (Keynote's alignment rows): draws `n` cells, the selected one blue,
/// `draw(painter, cell rect, index, colour)` paints each icon. Returns the clicked cell.
pub(crate) fn icon_segments(ui: &mut egui::Ui, id: &str, n: usize, selected: Option<usize>, draw: impl Fn(&egui::Painter, egui::Rect, usize, Color32)) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, 7.0, theme::gray(0x33));
    let w = rect.width() / n as f32;
    let mut clicked = None;
    for i in 0..n {
        let r = egui::Rect::from_min_size(egui::pos2(rect.min.x + w * i as f32, rect.min.y), egui::vec2(w, rect.height()));
        let resp = ui.interact(r, ui.scope_id().with((id, i)), egui::Sense::click());
        let sel = selected == Some(i);
        if sel {
            p.rect_filled(r.shrink(1.0), 6.0, theme::ACCENT());
        } else if resp.hovered() {
            p.rect_filled(r.shrink(2.0), 5.0, theme::gray(0x40));
        }
        if i > 0 && !sel && selected != Some(i - 1) {
            p.line_segment([egui::pos2(r.min.x, r.min.y + 8.0), egui::pos2(r.min.x, r.max.y - 8.0)], egui::Stroke::new(1.0, theme::gray(0x55)));
        }
        draw(&p, r, i, if sel { Color32::WHITE } else { theme::TEXT() });
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// Format > Text, laid out like Keynote 15.4: paragraph style button, Style | Layout, Font
/// (family, typeface + size, B I U S + ⚙, character styles), Text Color, alignment / indent /
/// vertical alignment segments, Spacing and Bullets & Lists sections.
pub(crate) fn text(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
    let DrawableKind::Shape(s) = &d.kind else { return };
    let Some(tb) = &s.text else {
        ui.label("No text");
        return;
    };
    let Some(run) = tb.paragraphs.first().and_then(|p| p.runs.first()).map(|r| r.style.clone()) else { return };
    let para = tb.paragraphs.first().cloned();
    let mut changes: Vec<(&str, TextProp)> = Vec::new();
    let mut command: Option<&str> = None;
    // paragraph style button
    let pstyle = app.wb.as_ref().map(|w| &w.doc).and_then(|doc| doc.paragraph_style_name(tb.storage)).unwrap_or_else(|| "Body".into());
    let fr = egui::Frame::new().fill(crate::look::style_button_colors().0).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(&pstyle).size(24.0).color(crate::look::style_button_colors().1));
    });
    let r = fr.response.rect;
    kn_ui::icons::draw(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.max.x - 22.0, r.center().y), egui::vec2(16.0, 16.0)), kn_ui::icons::Icon::ChevronDown, theme::gray(0x30));
    // the paragraph styles, each in its own look (Keynote's paragraph style pop-up)
    let pr = ui.interact(r, ui.scope_id().with("para-style"), egui::Sense::click());
    let mut pick_style = None;
    egui::Popup::menu(&pr).width(r.width()).show(|ui| {
        let styles = app.wb.as_ref().map(|w| &w.doc).map(|d| d.paragraph_styles()).unwrap_or_default();
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            for (id, name) in styles {
                if ui.selectable_label(name == pstyle, RichText::new(&name).size(15.0)).clicked() {
                    pick_style = Some(id);
                    ui.close();
                }
            }
        });
    });
    if let Some(sid) = pick_style {
        let storage = tb.storage;
        crate::editing::apply(app, "Paragraph Style", |doc, h, oid| oid == d.id && doc.set_paragraph_style(h, storage, sid));
        return;
    }
    ui.add_space(8.0);
    let key = ui.scope_id().with("text-subtab");
    let mut sub = ui.data(|m| m.get_temp::<usize>(key)).unwrap_or(0);
    seg_small(ui, &["Style", "Layout"], &mut sub);
    ui.data_mut(|m| m.insert_temp(key, sub));
    if sub == 1 {
        // Keynote's Text > Layout: Columns, Text Inset, Shrink text to fit
        use kn_model::format::TextLayoutChange;
        let mut change: Option<TextLayoutChange> = None;
        section(ui, "Columns");
        ui.horizontal(|ui| {
            ui.label("Columns");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut n = tb.columns as f64;
                if theme::stepper(ui, &mut n, 1.0, 1.0..=10.0, "", 0) {
                    change = Some(TextLayoutChange::Columns(n as u32, tb.column_gap.max(if n > 1.0 { 12.0 } else { 0.0 })));
                }
            });
        });
        ui.add_enabled_ui(tb.columns > 1, |ui| {
            ui.horizontal(|ui| {
                ui.label("Gutter");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut g = tb.column_gap as f64;
                    if theme::stepper(ui, &mut g, 1.0, 0.0..=200.0, " pt", 0) {
                        change = Some(TextLayoutChange::Columns(tb.columns, g as f32));
                    }
                });
            });
        });
        section(ui, "Margins");
        ui.horizontal(|ui| {
            ui.label("Text Inset");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut v = tb.padding[0] as f64;
                if theme::stepper(ui, &mut v, 1.0, 0.0..=200.0, " pt", 0) {
                    change = Some(TextLayoutChange::Inset(v as f32));
                }
            });
        });
        section(ui, "Text Fitting");
        let mut shrink = tb.shrink_to_fit;
        if theme::checkbox(ui, &mut shrink, "Shrink text to fit").changed() {
            change = Some(TextLayoutChange::ShrinkToFit(shrink));
        }
        if let Some(c) = change {
            let id = d.id;
            crate::editing::apply(app, "Text Layout", |doc, h, oid| oid == id && doc.set_text_layout(h, oid, c));
        }
        return;
    }
    ui.separator();
    ui.label(theme::bold("Font").size(14.0));
    let family = kn_render::fonts::parse_ps_name(&run.font_name).0;
    if app.font_names.is_none() {
        app.font_names = Some(crate::editing::font_names(app));
    }
    let fonts = app.font_names.clone().unwrap_or_default();
    theme::combo("font").selected_text(&family).width(ui.available_width()).height(400.0).show_ui(ui, |ui| {
        for f in &fonts {
            if ui.selectable_label(*f == family, f).clicked() && *f != family {
                changes.push(("Font", TextProp::Font(f.replace(' ', ""))));
            }
        }
    });
    ui.horizontal(|ui| {
        let typeface = match (run.bold, run.italic) {
            (true, true) => "Bold Italic",
            (true, false) => "Bold",
            (false, true) => "Italic",
            _ => "Regular",
        };
        theme::combo("typeface").selected_text(typeface).width(ui.available_width() - 100.0).show_ui(ui, |ui| {
            for (n, b, i) in [("Regular", false, false), ("Italic", false, true), ("Bold", true, false), ("Bold Italic", true, true)] {
                if ui.selectable_label(n == typeface, n).clicked() && n != typeface {
                    if b != run.bold {
                        changes.push(("Bold", TextProp::Bold(b)));
                    }
                    if i != run.italic {
                        changes.push(("Italic", TextProp::Italic(i)));
                    }
                }
            }
        });
        let mut size = run.size as f64;
        if theme::stepper(ui, &mut size, 1.0, 1.0..=1000.0, " pt", 0) {
            changes.push(("Font Size", TextProp::Size(size.round() as f32)));
        }
    });
    ui.horizontal(|ui| {
        let w = ui.available_width();
        ui.allocate_ui(egui::vec2(w - 110.0, 30.0), |ui| {
            let states = [run.bold, run.italic, run.underline, run.strike];
            // several can be on at once: draw each "on" cell blue
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(rect, 7.0, theme::gray(0x33));
            let cw = rect.width() / 4.0;
            for (i, (lab, on)) in ["B", "I", "U", "S"].iter().zip(states).enumerate() {
                let r = egui::Rect::from_min_size(egui::pos2(rect.min.x + cw * i as f32, rect.min.y), egui::vec2(cw, rect.height()));
                let resp = ui.interact(r, ui.scope_id().with(("bius", i)), egui::Sense::click());
                if on {
                    p.rect_filled(r.shrink(1.0), 6.0, theme::ACCENT());
                } else if resp.hovered() {
                    p.rect_filled(r.shrink(2.0), 5.0, theme::gray(0x40));
                }
                let ink = if on { Color32::WHITE } else { theme::gray(0xee) };
                let font = if i == 0 { egui::FontId::new(15.0, egui::FontFamily::Name("bold".into())) } else { egui::FontId::proportional(15.0) };
                let g = p.layout_no_wrap(lab.to_string(), font, ink);
                let pos = r.center() - g.size() / 2.0;
                p.galley(pos, g.clone(), ink);
                if i == 2 {
                    p.line_segment([egui::pos2(pos.x, pos.y + g.size().y - 2.0), egui::pos2(pos.x + g.size().x, pos.y + g.size().y - 2.0)], egui::Stroke::new(1.2, ink));
                }
                if i == 3 {
                    p.line_segment([egui::pos2(pos.x - 1.0, r.center().y), egui::pos2(pos.x + g.size().x + 1.0, r.center().y)], egui::Stroke::new(1.2, ink));
                }
                if resp.clicked() {
                    changes.push(match i {
                        0 => ("Bold", TextProp::Bold(!on)),
                        1 => ("Italic", TextProp::Italic(!on)),
                        2 => ("Underline", TextProp::Underline(!on)),
                        _ => ("Strikethrough", TextProp::Strike(!on)),
                    });
                }
            }
        });
        // ⚙ advanced options (Keynote's gear menu)
        let gear = ui.add_sized([96.0, 30.0], egui::Button::new("⚙ ⌄"));
        egui::Popup::menu(&gear).show(|ui| {
            ui.set_min_width(220.0);
            let mut tracking = run.tracking * 100.0;
            ui.horizontal(|ui| {
                ui.label("Character Spacing");
                if theme::stepper32(ui, &mut tracking, 1.0, -50.0..=200.0, " %", 0) {
                    changes.push(("Character Spacing", TextProp::Tracking(tracking.round() / 100.0)));
                }
            });
            ui.separator();
            ui.label(RichText::new("Baseline").color(theme::TEXT_DIM()));
            for (v, n) in [(0i8, "Default"), (1, "Superscript"), (-1, "Subscript")] {
                if ui.button(n).clicked() {
                    changes.push(("Baseline", TextProp::Script(v)));
                }
            }
            ui.separator();
            ui.label(RichText::new("Capitalization").color(theme::TEXT_DIM()));
            for (c, n) in [(kn_model::Caps::None, "None"), (kn_model::Caps::All, "All Caps"), (kn_model::Caps::Small, "Small Caps"), (kn_model::Caps::Title, "Title Case"), (kn_model::Caps::Start, "Start Case")] {
                if ui.button(n).clicked() {
                    changes.push(("Capitalization", TextProp::Caps(c)));
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label(theme::bold("Character Styles"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let styles = app.wb.as_ref().map(|w| &w.doc).map(|d| d.character_styles()).unwrap_or_default();
            let mut pick: Option<Option<kn_format::ObjId>> = None;
            theme::combo("charstyles").selected_text("None").width(150.0).show_ui(ui, |ui| {
                if ui.selectable_label(false, "None").clicked() {
                    pick = Some(None);
                }
                for (id, name) in styles.iter().filter(|(_, n)| n != "None") {
                    if ui.selectable_label(false, name).clicked() {
                        pick = Some(Some(*id));
                    }
                }
            });
            if let Some(style) = pick {
                // the selected text while editing, else all of it
                let range = match app.cursor {
                    Some(c) if !c.is_empty() => Some((c.storage, c.range().0, c.range().1)),
                    _ => Some((tb.storage, 0, tb.text.len())),
                };
                if let (Some((st, a, b)), Some(doc)) = (range, app.wb.as_mut().map(|w| &mut w.doc)) {
                    doc.begin(&mut app.hist, "Character Style");
                    doc.set_character_style(&mut app.hist, st, a, b, style);
                    doc.end(&mut app.hist);
                }
                if let Some(s) = app.current_slide().map(|s| s.slide) {
                    app.bump(s);
                }
            }
        });
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Text Color");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(c) = color_button(ui, run.color) {
                changes.push(("Text Color", TextProp::Color(c)));
            }
        });
    });
    ui.separator();
    // horizontal alignment: left / centre / right / justify (lines icons)
    if let Some(p) = &para {
        let cur = match p.align {
            Align::Left | Align::Natural => 0,
            Align::Center => 1,
            Align::Right => 2,
            Align::Justify => 3,
        };
        let pick = icon_segments(ui, "align", 4, Some(cur), |pt, r, i, col| {
            let st = egui::Stroke::new(1.5, col);
            let c = r.center();
            for (k, len) in [(0, 16.0), (1, 10.0), (2, 16.0), (3, 10.0)] {
                let y = c.y - 6.0 + k as f32 * 4.0;
                let len = if i == 3 { 16.0 } else { len };
                let (x0, x1) = match i {
                    0 => (c.x - 8.0, c.x - 8.0 + len),
                    1 => (c.x - len / 2.0, c.x + len / 2.0),
                    2 => (c.x + 8.0 - len, c.x + 8.0),
                    _ => (c.x - 8.0, c.x + 8.0),
                };
                pt.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], st);
            }
        });
        if let Some(i) = pick.filter(|i| *i != cur) {
            changes.push(("Alignment", TextProp::Align([Align::Left, Align::Center, Align::Right, Align::Justify][i])));
        }
        ui.add_space(4.0);
        // indent: decrease / increase list level
        if let Some(i) = icon_segments(ui, "indent", 2, None, |pt, r, i, col| {
            let st = egui::Stroke::new(1.3, col);
            let c = r.center();
            for k in 0..3 {
                let y = c.y - 4.0 + k as f32 * 4.0;
                pt.line_segment([egui::pos2(c.x - 3.0, y), egui::pos2(c.x + 8.0, y)], st);
            }
            let ax = c.x - 7.0;
            let dir = if i == 0 { -1.0 } else { 1.0 };
            pt.line(vec![egui::pos2(ax - 2.0 * dir, c.y - 3.0), egui::pos2(ax + 2.0 * dir, c.y), egui::pos2(ax - 2.0 * dir, c.y + 3.0)], st);
        }) {
            command = Some(if i == 0 { "Decrease Indent Level" } else { "Increase Indent Level" });
        }
        ui.add_space(4.0);
    }
    // vertical alignment: top / middle / bottom
    let vcur = match tb.valign {
        VAlign::Top => 0,
        VAlign::Middle => 1,
        VAlign::Bottom => 2,
    };
    if let Some(i) = icon_segments(ui, "valign", 3, Some(vcur), |pt, r, i, col| {
        let st = egui::Stroke::new(1.5, col);
        let c = r.center();
        let bar_y = [c.y - 7.0, c.y, c.y + 7.0][i];
        pt.line_segment([egui::pos2(c.x - 7.0, bar_y), egui::pos2(c.x + 7.0, bar_y)], st);
        match i {
            0 => pt.line(vec![egui::pos2(c.x, c.y + 7.0), egui::pos2(c.x, c.y - 5.0), egui::pos2(c.x - 3.5, c.y - 1.5), egui::pos2(c.x, c.y - 5.0), egui::pos2(c.x + 3.5, c.y - 1.5)], st),
            1 => {
                pt.line(vec![egui::pos2(c.x, c.y - 7.0), egui::pos2(c.x, c.y - 2.0)], st);
                pt.line(vec![egui::pos2(c.x, c.y + 7.0), egui::pos2(c.x, c.y + 2.0)], st)
            }
            _ => pt.line(vec![egui::pos2(c.x, c.y - 7.0), egui::pos2(c.x, c.y + 5.0), egui::pos2(c.x - 3.5, c.y + 1.5), egui::pos2(c.x, c.y + 5.0), egui::pos2(c.x + 3.5, c.y + 1.5)], st),
        };
    }) {
        if i != vcur {
            changes.push(("Vertical Alignment", TextProp::VAlign([VAlign::Top, VAlign::Middle, VAlign::Bottom][i])));
        }
    }
    if let Some(p) = &para {
        if theme::disclosure(ui, "spacing", "Spacing", None) {
            let (amount, relative) = match p.line_spacing {
                kn_model::LineSpacing::Relative(a) => (a, true),
                _ => (1.0, false),
            };
            ui.horizontal(|ui| {
                theme::combo("linespacing-mode").selected_text(if relative { "Lines" } else { "Exactly" }).width(120.0).show_ui(ui, |ui| {
                    let _ = ui.selectable_label(relative, "Lines");
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut v = amount as f64;
                    if theme::stepper(ui, &mut v, 0.1, 0.1..=10.0, "", 2) {
                        changes.push(("Line Spacing", TextProp::LineSpacing(v as f32)));
                    }
                });
            });
            for (label, cur, before) in [("Before Paragraph", p.space_before, true), ("After Paragraph", p.space_after, false)] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut v = cur as f64;
                        if theme::stepper(ui, &mut v, 1.0, 0.0..=500.0, " pt", 0) {
                            let v = v.round() as f32;
                            changes.push(if before { ("Space Before", TextProp::SpaceBefore(v)) } else { ("Space After", TextProp::SpaceAfter(v)) });
                        }
                    });
                });
            }
        }
    }
    if theme::disclosure(ui, "bullets", "Bullets & Lists", None) {
        let has_bullets = tb.paragraphs.iter().any(|p| p.label.is_some());
        let cur = if has_bullets { "Bullet" } else { "No Bullets" };
        theme::combo("bullets").selected_text(cur).width(ui.available_width()).show_ui(ui, |ui| {
            if ui.selectable_label(!has_bullets, "No Bullets").clicked() && has_bullets {
                changes.push(("Bullets", TextProp::Bullets(false)));
            }
            if ui.selectable_label(has_bullets, "Bullet").clicked() && !has_bullets {
                changes.push(("Bullets", TextProp::Bullets(true)));
            }
        });
    }
    if let Some(c) = command {
        app.command(ui.ctx(), c);
    }
    for (label, p) in changes {
        crate::editing::set_text(app, label, p);
    }
}

/// Format > Arrange, laid out like Keynote 15.4: layer buttons (Back / Front, Backward / Forward,
/// icons with labels), Align / Distribute pop-ups, Size / Position / Rotate fields with steppers
/// and captions, rotation dial + Flip, and the Lock / Unlock / Group / Ungroup grid.
pub(crate) fn arrange(app: &mut App, ui: &mut egui::Ui, selected: &[kn_model::Drawable]) {
    let d = &selected[0];
    ui.set_max_width(ui.available_width());
    let mut command: Option<&str> = None;
    // layer buttons: stacked-squares icons, label below
    let layer_icon = |p: &egui::Painter, c: egui::Pos2, top: bool, col: Color32| {
        // three stacked diamonds; the highlighted one (front/back) drawn solid
        for k in 0..3 {
            let y = c.y - 5.0 + k as f32 * 4.0;
            let pts = vec![egui::pos2(c.x - 8.0, y), egui::pos2(c.x, y - 3.5), egui::pos2(c.x + 8.0, y), egui::pos2(c.x, y + 3.5)];
            let solid = (top && k == 0) || (!top && k == 2);
            if solid {
                p.add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
            } else {
                p.add(egui::Shape::closed_line(pts, egui::Stroke::new(1.1, col)));
            }
        }
    };
    ui.horizontal(|ui| {
        let half = (ui.available_width() - 16.0) / 2.0;
        for pair in [[("Back", "Send to Back", false), ("Front", "Bring to Front", true)], [("Backward", "Send Backward", false), ("Forward", "Bring Forward", true)]] {
            // top-down inside the row: the button pair, then room for the captions below it
            ui.allocate_ui_with_layout(egui::vec2(half, 52.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(half, 30.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 7.0, theme::gray(0x33));
                let w = rect.width() / 2.0;
                for (i, (label, cmd, top)) in pair.iter().enumerate() {
                    let r = egui::Rect::from_min_size(egui::pos2(rect.min.x + w * i as f32, rect.min.y), egui::vec2(w, rect.height()));
                    let resp = ui.interact(r, ui.scope_id().with(("layer", *label)), egui::Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r.shrink(2.0), 5.0, theme::gray(0x44));
                    }
                    layer_icon(ui.painter(), r.center(), *top, theme::TEXT());
                    theme::centered(ui.painter(), egui::pos2(r.center().x, rect.max.y + 10.0), *label, egui::FontId::new(12.0, egui::FontFamily::Name("bold".into())), theme::TEXT());
                    if resp.clicked() {
                        command = Some(cmd);
                    }
                }
                ui.add_space(18.0);
            });
        }
    });
    ui.horizontal(|ui| {
        let half = (ui.available_width() - 12.0) / 2.0;
        let multi = selected.len() >= 2;
        ui.add_enabled_ui(multi, |ui| {
            theme::combo("align").selected_text("Align").width(half - 52.0).show_ui(ui, |ui| {
                for (label, cmd) in [("Left", "Align Objects > Left"), ("Center", "Align Objects > Center"), ("Right", "Align Objects > Right"), ("Top", "Align Objects > Top"), ("Middle", "Align Objects > Middle"), ("Bottom", "Align Objects > Bottom")] {
                    if ui.selectable_label(false, label).clicked() {
                        command = Some(cmd);
                    }
                }
            });
        });
        ui.add_enabled_ui(selected.len() >= 3, |ui| {
            theme::combo("distribute").selected_text("Distribute").width(half - 52.0).show_ui(ui, |ui| {
                for (label, cmd) in [("Evenly", "Distribute Objects > Evenly"), ("Horizontally", "Distribute Objects > Horizontally"), ("Vertically", "Distribute Objects > Vertically")] {
                    if ui.selectable_label(false, label).clicked() {
                        command = Some(cmd);
                    }
                }
            });
        });
    });
    let g0 = app.canvas.tiles.effective(d);
    let mut g = g0;
    let locked = d.locked;
    // two stepper fields side by side with captions under them
    let pair = |ui: &mut egui::Ui, title: &str, a: (&str, &mut f32, f64, std::ops::RangeInclusive<f64>, &str), b: (&str, &mut f32, f64, std::ops::RangeInclusive<f64>, &str)| {
        ui.add_space(4.0);
        ui.separator();
        ui.label(theme::bold(title).size(14.0));
        ui.columns(2, |c| {
            for (col, (cap, v, step, range, suffix)) in c.iter_mut().zip([a, b]) {
                col.vertical_centered(|ui| {
                    let mut x = *v as f64;
                    if theme::stepper(ui, &mut x, step, range, suffix, if step < 1.0 { 1 } else { 0 }) {
                        *v = x as f32;
                    }
                    ui.label(theme::bold(cap).size(12.0));
                });
            }
        });
    };
    let aspect_key = ui.scope_id().with(("constrain", d.id));
    let mut constrain = ui.data(|m| m.get_temp::<bool>(aspect_key)).unwrap_or(false);
    ui.add_enabled_ui(!locked, |ui| {
        pair(ui, "Size", ("Width", &mut g.w, 1.0, 1.0..=100000.0, " pt"), ("Height", &mut g.h, 1.0, 1.0..=100000.0, " pt"));
        ui.horizontal(|ui| {
            ui.add_space(ui.available_width() * 0.2);
            if theme::checkbox(ui, &mut constrain, "Constrain proportions").changed() {
                ui.data_mut(|m| m.insert_temp(aspect_key, constrain));
            }
        });
        pair(ui, "Position", ("X", &mut g.x, 1.0, -100000.0..=100000.0, " pt"), ("Y", &mut g.y, 1.0, -100000.0..=100000.0, " pt"));
        ui.add_space(4.0);
        ui.separator();
        ui.label(theme::bold("Rotate").size(14.0));
        ui.horizontal(|ui| {
            // rotation dial: drag around it to rotate
            let (r, resp) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::drag());
            let c = r.center();
            ui.painter().circle_filled(c, 16.0, theme::gray(0x3a));
            let a = -g.angle.to_radians();
            ui.painter().line_segment([c, c + egui::vec2(a.cos(), a.sin()) * 11.0], egui::Stroke::new(2.0, theme::TEXT()));
            if resp.dragged() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let v = p - c;
                    g.angle = (-v.y.atan2(v.x)).to_degrees().rem_euclid(360.0).round();
                }
            }
            ui.vertical(|ui| {
                let mut ang = g.angle as f64;
                if theme::stepper(ui, &mut ang, 1.0, 0.0..=359.0, "°", 0) {
                    g.angle = ang as f32;
                }
                ui.label(theme::bold("Angle").size(12.0));
            });
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if ui.add_sized([34.0, 26.0], egui::Button::new("⇅")).on_hover_text("Flip Vertically").clicked() {
                        g.vflip = !g.vflip;
                    }
                    if ui.add_sized([34.0, 26.0], egui::Button::new("⇆")).on_hover_text("Flip Horizontally").clicked() {
                        g.hflip = !g.hflip;
                    }
                });
                ui.label(theme::bold("Flip").size(12.0));
            });
        });
    });
    if constrain && g0.w > 0.0 && g0.h > 0.0 {
        if g.w != g0.w {
            g.h = g0.h * g.w / g0.w;
        } else if g.h != g0.h {
            g.w = g0.w * g.h / g0.h;
        }
    }
    if g != g0 {
        let slide = app.current_slide().map(|s| s.slide);
        let id = d.id;
        let is_table = matches!(d.kind, DrawableKind::Table(_));
        let merge = app.pointer_down;
        if let Some(doc) = app.wb.as_mut().map(|w| &mut w.doc) {
            if merge {
                doc.begin_merge(&mut app.hist, "Change Geometry");
            } else {
                doc.begin(&mut app.hist, "Change Geometry");
            }
            if is_table && (g.w != g0.w || g.h != g0.h) {
                doc.scale_table_to(&mut app.hist, id, g.w, g.h);
            }
            doc.set_frame(&mut app.hist, id, Geometry { auto_width: g0.auto_width && g.w == g0.w, auto_height: g0.auto_height && g.h == g0.h, ..g });
            doc.end(&mut app.hist);
        }
        if let Some(s) = slide {
            app.bump(s);
        }
    }
    ui.add_space(6.0);
    ui.separator();
    let is_group = selected.len() == 1 && matches!(d.kind, DrawableKind::Group(_));
    ui.columns(2, |c| {
        let w = c[0].available_width();
        if c[0].add_enabled_ui(!locked, |ui| ui.add_sized([w, 28.0], egui::Button::new("Lock"))).inner.clicked() {
            command = Some("Lock");
        }
        if c[1].add_enabled_ui(locked, |ui| ui.add_sized([w, 28.0], egui::Button::new("Unlock"))).inner.clicked() {
            command = Some("Unlock");
        }
        if c[0].add_enabled_ui(selected.len() >= 2, |ui| ui.add_sized([w, 28.0], egui::Button::new("Group"))).inner.clicked() {
            command = Some("Group");
        }
        if c[1].add_enabled_ui(is_group, |ui| ui.add_sized([w, 28.0], egui::Button::new("Ungroup"))).inner.clicked() {
            command = Some("Ungroup");
        }
    });
    if let Some(cmd) = command {
        app.command(ui.ctx(), cmd);
    }
}

// --------------------------------------------------------------------------- animate


/// A small picture of image data `data` (cached per data id).
fn gallery_thumb(app: &mut App, ctx: &egui::Context, data: u64) -> Option<egui::TextureHandle> {
    thread_local!(static THUMBS: std::cell::RefCell<std::collections::HashMap<u64, egui::TextureHandle>> = Default::default());
    if let Some(t) = THUMBS.with(|m| m.borrow().get(&data).cloned()) {
        return Some(t);
    }
    let bytes = app.wb.as_ref()?.doc.data(data)?;
    let img = image::load_from_memory(&bytes).ok()?.thumbnail(96, 72).to_rgba8();
    let tex = ctx.load_texture(format!("gallery-thumb-{data}"), egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), egui::TextureOptions::LINEAR);
    THUMBS.with(|m| m.borrow_mut().insert(data, tex.clone()));
    Some(tex)
}

/// One of the object inspector's panels (Style, Image, Chart, Axis, Series) for `d`.
pub fn ui_tab(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable, tab: &str) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match tab {
        "Style" => style(app, ui, d),
        "Image" => image_panel(app, ui, d),
        "Chart" | "Axis" | "Series" => chart_panes(app, ui, d, tab),
        _ => {}
    });
}

thread_local! {
    /// The Axis / Series panes' picked axis and series.
    static CHART_PANES: std::cell::Cell<kn_ui::chartpanes::ChartPanes> = std::cell::Cell::new(Default::default());
}

/// The Chart / Axis / Series tabs: the suite's shared panes (kn_ui::chartpanes).
fn chart_panes(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable, tab: &str) {
    use kn_ui::chartpanes::{self, PaneAction};
    let DrawableKind::Chart(c) = &d.kind else { return };
    let id = d.id;
    let mut st = CHART_PANES.with(|s| s.get());
    let Some(doc) = app.wb.as_mut().map(|w| &mut w.doc) else { return };
    let act = match tab {
        "Chart" => chartpanes::chart_tab(ui, doc, &mut app.hist, id, c),
        "Axis" => chartpanes::axis_tab(ui, doc, &mut app.hist, id, c, &mut st),
        _ => chartpanes::series_tab(ui, doc, &mut app.hist, id, c, &mut st),
    };
    CHART_PANES.with(|s| s.set(st));
    match act {
        PaneAction::EditData => crate::chartedit::open(app, id),
        PaneAction::Changed => app.changed(),
        PaneAction::None => {}
    }
}

/// The Format inspector for the selected object: the tabs Numbers shows for its kind.
#[allow(dead_code)]
pub fn ui(app: &mut App, ui: &mut egui::Ui, id: kn_format::ObjId) {
    let Some(d) = app.drawables.iter().find(|d| d.id == id).cloned() else { return };
    let labels: Vec<&str> = match &d.kind {
        DrawableKind::Shape(s) if s.is_line => vec!["Style", "Arrange"],
        DrawableKind::Shape(_) => vec!["Style", "Text", "Arrange"],
        DrawableKind::Image(_) => vec!["Style", "Image", "Arrange"],
        DrawableKind::Chart(_) => vec!["Chart", "Axis", "Series", "Arrange"],
        _ => vec!["Arrange"],
    };
    let mut tab = app.obj_tab.min(labels.len() - 1);
    tabs(ui, &labels, &mut tab);
    app.obj_tab = tab;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match labels[tab] {
        "Style" => style(app, ui, &d),
        "Text" => text(app, ui, &d),
        "Image" => image_panel(app, ui, &d),
        "Chart" => chart_panel(app, ui, &d),
        "Axis" | "Series" => chart_axis_series(app, ui, &d, labels[tab]),
        _ => {
            // every selected object (Group needs two or more)
            let all: Vec<kn_model::Drawable> = app.selected_objs().iter().filter_map(|id| app.drawables.iter().find(|x| x.id == *id).cloned()).collect();
            arrange(app, ui, if all.is_empty() { std::slice::from_ref(&d) } else { &all })
        }
    });
}

/// Chart: type, legend and title (Keydeck's Chart tab).
fn chart_panel(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
    let DrawableKind::Chart(c) = &d.kind else { return };
    let (id, cur_type, legend) = (d.id, c.chart_type.clone(), c.show_legend);
    section(ui, "Chart");
    if ui.add(egui::Button::new("Edit Chart Data").min_size(egui::vec2(ui.available_width(), 26.0))).clicked() {
        crate::chartedit::open(app, id);
    }
    ui.label(RichText::new(format!("{} series × {} categories", c.values.len(), c.col_names.len())).color(theme::TEXT_DIM()));
    section(ui, "Chart Type");
    let types = kn_model::chartdata::CHART_TYPES;
    let shown = types.iter().find(|t| t.0 == cur_type).map(|t| t.1.to_string()).unwrap_or_else(|| cur_type.clone());
    let mut pick = None;
    theme::combo("chart-type").selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
        for (k, name) in types {
            if ui.selectable_label(*k == cur_type, *name).clicked() {
                pick = Some(*k);
            }
        }
    });
    section(ui, "Chart Options");
    let mut show = legend;
    let legend_changed = theme::checkbox(ui, &mut show, "Legend").changed();
    let mut has_title = c.title.is_some();
    let mut title_text = c.title.clone().unwrap_or_else(|| "Title".into());
    let mut title_change: Option<Option<String>> = None;
    if theme::checkbox(ui, &mut has_title, "Title").changed() {
        title_change = Some(has_title.then(|| title_text.clone()));
    }
    if has_title {
        let r = ui.add(egui::TextEdit::singleline(&mut title_text).desired_width(f32::INFINITY));
        if r.lost_focus() && Some(title_text.as_str()) != c.title.as_deref() {
            title_change = Some(Some(title_text.clone()));
        }
    }
    if let Some(t) = title_change {
        app.edit("Chart Title", |doc, h| doc.set_chart_title(h, id, t.as_deref()));
    }
    if let Some(p) = pick.filter(|p| *p != cur_type) {
        app.edit("Chart Type", |doc, h| doc.set_chart_type(h, id, p));
    }
    if legend_changed {
        app.edit("Legend", |doc, h| doc.set_chart_legend(h, id, show));
    }
}

/// Chart Axis tab (Value Y / Category X: Axis Name, Labels, Major Gridlines, Axis Line) and
/// Series tab (Value Labels) — Keydeck's.
fn chart_axis_series(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable, tab: &str) {
    use kn_model::chartdata::ChartFlag;
    let DrawableKind::Chart(c) = &d.kind else { return };
    let id = d.id;
    let mut flag: Option<ChartFlag> = None;
    if tab == "Axis" {
        let key = egui::Id::unique("chart-axis-tab");
        let mut which: usize = ui.data(|m| m.get_temp(key)).unwrap_or(0);
        seg_small(ui, &["Value (Y)", "Category (X)"], &mut which);
        ui.data_mut(|m| m.insert_temp(key, which));
        let (axis, idx) = if which == 0 { (c.value_axes.first().cloned().unwrap_or_default(), Some(0)) } else { (c.category_axis.clone(), None) };
        section(ui, "Axis Options");
        for (label, prop, on) in [("Axis Name", "showtitle", axis.title.is_some()), ("Labels", "showlabels", axis.show_labels), ("Major Gridlines", "showmajorgridlines", axis.show_gridlines), ("Axis Line", "showaxis", axis.show_line)] {
            let mut v = on;
            if theme::checkbox(ui, &mut v, label).changed() {
                flag = Some(ChartFlag::Axis(idx, prop, v));
            }
        }
        if c.value_axes.len() > 1 && which == 0 {
            section(ui, "Second Value Axis");
            let a2 = &c.value_axes[1];
            for (label, prop, on) in [("Axis Name", "showtitle", a2.title.is_some()), ("Labels", "showlabels", a2.show_labels)] {
                let mut v = on;
                if theme::checkbox(ui, &mut v, label).changed() {
                    flag = Some(ChartFlag::Axis(Some(1), prop, v));
                }
            }
        }
    } else {
        section(ui, "Value Labels");
        let mut v = c.series.first().is_some_and(|s| s.show_value_labels);
        if theme::checkbox(ui, &mut v, "Show value labels").changed() {
            flag = Some(ChartFlag::ValueLabels(v));
        }
        section(ui, "Series");
        for s in &c.series {
            ui.label(&s.name);
        }
    }
    if let Some(f) = flag {
        app.edit("Chart", |doc, h| doc.set_chart_flag(h, id, f));
    }
}
