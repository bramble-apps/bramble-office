//! View > Show Warnings: Keynote's Document Warnings (fonts that aren't installed, media that
//! can't be found), and File > Reduce File Size.

use egui::RichText;

use crate::app::App;
use crate::theme;

#[derive(Default)]
pub struct Warnings {
    pub open: bool,
    list: Vec<String>,
    missing_fonts: bool,
}

/// Look the document over and open the window.
pub fn show(app: &mut App) {
    let Some(doc) = app.doc.as_ref() else { return };
    let mut list = Vec::new();
    let mut missing_fonts = false;
    for (f, _) in doc.fonts_used() {
        if !app.renderer.fonts.is_installed(&f) {
            list.push(format!("The font \u{201c}{f}\u{201d} is missing and was replaced."));
            missing_fonts = true;
        }
    }
    if let Some(pm) = doc.file.package_metadata() {
        for d in pm.msgs("datas") {
            let Some(id) = d.u64("identifier") else { continue };
            if doc.file.data(id).is_none() {
                let name = d.str("preferred_file_name").or(d.str("file_name")).unwrap_or("media");
                list.push(format!("\u{201c}{name}\u{201d} couldn\u{2019}t be found and is shown as a placeholder."));
            }
        }
    }
    app.warnings = Warnings { open: true, list, missing_fonts };
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.warnings.open {
        return;
    }
    let mut open = true;
    let mut replace = false;
    egui::Window::new("Document Warnings").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(true).default_width(460.0).open(&mut open).show(ctx, |ui| {
        if app.warnings.list.is_empty() {
            ui.label(RichText::new("There are no warnings for this presentation.").color(theme::TEXT_DIM()));
        }
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for w in &app.warnings.list {
                ui.label(format!("\u{26a0}  {w}"));
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if app.warnings.missing_fonts && ui.button("Replace Fonts\u{2026}").clicked() {
                replace = true;
            }
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(app.warnings.list.join("\n"));
            }
        });
    });
    app.warnings.open = open;
    if replace {
        crate::replacefonts::open(app);
    }
}

/// File > Reduce File Size: pictures much larger than they are shown are scaled down and
/// re-saved (JPEG for photos, PNG when they have transparency). Returns bytes saved.
pub fn reduce_file_size(app: &mut App) {
    let Some(doc) = app.doc.as_mut() else { return };
    // the largest size each picture is drawn at (slide points; 2x for sharp HiDPI display)
    let mut shown: std::collections::HashMap<u64, (f32, f32)> = std::collections::HashMap::new();
    let slides = doc.slides();
    for i in 0..slides.len() {
        let Some(sc) = doc.scene(&slides, i) else { continue };
        for d in &sc.drawables {
            if let kn_model::DrawableKind::Image(img) = &d.kind {
                if let Some(id) = img.data_id {
                    let e = shown.entry(id).or_insert((0.0, 0.0));
                    e.0 = e.0.max(d.geom.w * 2.0);
                    e.1 = e.1.max(d.geom.h * 2.0);
                }
            }
        }
    }
    let mut saved = 0i64;
    let mut changed = Vec::new();
    for (id, (w, h)) in shown {
        let Some((name, bytes)) = doc.file.data(id).map(|(n, b)| (n.to_string(), b.into_owned())) else { continue };
        let Ok(img) = image::load_from_memory(&bytes) else { continue };
        if (img.width() as f32) < w * 1.5 || bytes.len() < 300_000 {
            continue;
        }
        let s = (w / img.width() as f32).max(h / img.height() as f32).min(1.0);
        let (nw, nh) = ((img.width() as f32 * s).round().max(1.0) as u32, (img.height() as f32 * s).round().max(1.0) as u32);
        let small = img.resize_exact(nw, nh, image::imageops::FilterType::Lanczos3);
        let mut out = std::io::Cursor::new(Vec::new());
        let alpha = small.color().has_alpha() && small.to_rgba8().pixels().any(|p| p[3] < 255);
        let ok = if alpha {
            small.write_to(&mut out, image::ImageFormat::Png).is_ok()
        } else {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88).encode_image(&small.to_rgb8()).is_ok()
        };
        let out = out.into_inner();
        if ok && out.len() < bytes.len() {
            saved += bytes.len() as i64 - out.len() as i64;
            changed.push((id, name, out, alpha));
        }
    }
    if changed.is_empty() {
        app.notify("The presentation can\u{2019}t be made any smaller");
        return;
    }
    doc.begin(&mut app.hist, "Reduce File Size");
    for (id, name, bytes, png) in changed {
        let stem = std::path::Path::new(&name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("image".into());
        let new_name = format!("{stem}-small.{}", if png { "png" } else { "jpeg" });
        doc.replace_data(&mut app.hist, id, bytes, &new_name);
    }
    doc.end(&mut app.hist);
    app.bump_all();
    app.notify(format!("Reduced the file by {:.1} MB", saved as f64 / 1_000_000.0));
}
