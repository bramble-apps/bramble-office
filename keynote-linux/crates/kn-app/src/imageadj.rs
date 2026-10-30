//! Image adjustments applied to the picture's pixels (Keynote stores the adjusted picture next
//! to the original, so other readers show it too), and the Adjust Image window.

use egui::RichText;
use kn_format::ObjId;
use kn_model::imageedit::Adjustments;

use crate::app::App;
use crate::theme;

/// Apply `a` to an encoded picture; returns the new file (PNG when it has transparency, else
/// JPEG) and its extension.
pub fn apply(bytes: &[u8], a: &Adjustments) -> Option<(Vec<u8>, &'static str)> {
    let img = image::load_from_memory(bytes).ok()?;
    let mut rgba = img.to_rgba8();
    // Enhance: stretch the levels to the picture's own darkest/brightest values
    let (mut lo, mut hi) = (a.bottom_level, a.top_level);
    if a.enhance {
        let mut hist = [0u32; 256];
        for p in rgba.pixels() {
            hist[((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as usize] += 1;
        }
        let total: u32 = hist.iter().sum();
        let cut = total / 200;
        let (mut acc, mut l, mut h) = (0, 0usize, 255usize);
        for (i, n) in hist.iter().enumerate() {
            acc += n;
            if acc > cut {
                l = i;
                break;
            }
        }
        acc = 0;
        for (i, n) in hist.iter().enumerate().rev() {
            acc += n;
            if acc > cut {
                h = i;
                break;
            }
        }
        lo = lo.max(l as f32 / 255.0);
        hi = hi.min(h as f32 / 255.0).max(lo + 0.05);
    }
    let exposure = 2f32.powf(a.exposure * 1.5);
    let sat = 1.0 + a.saturation;
    let contrast = 1.0 + a.contrast;
    let gamma = a.gamma.clamp(0.1, 10.0);
    let (rt, bt) = (1.0 + a.temperature * 0.15, 1.0 - a.temperature * 0.15);
    let gt = 1.0 - a.tint * 0.1;
    for p in rgba.pixels_mut() {
        let mut c = [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0];
        c[0] *= rt;
        c[1] *= gt;
        c[2] *= bt;
        for v in c.iter_mut() {
            *v = ((*v - lo) / (hi - lo).max(0.01)).clamp(0.0, 1.0).powf(1.0 / gamma);
            *v *= exposure;
            *v = (*v - 0.5) * contrast + 0.5;
        }
        let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
        // shadows lift the darks, highlights pull down the brights
        let tone = a.shadows * (1.0 - l).powi(2) * 0.4 - a.highlights * l.powi(2) * 0.4;
        for v in c.iter_mut() {
            *v = l + (*v - l) * sat + tone;
            *v = v.clamp(0.0, 1.0);
        }
        p[0] = (c[0] * 255.0).round() as u8;
        p[1] = (c[1] * 255.0).round() as u8;
        p[2] = (c[2] * 255.0).round() as u8;
    }
    let mut out = image::DynamicImage::ImageRgba8(rgba);
    if a.denoise > 0.0 {
        out = out.blur(a.denoise * 1.5);
    }
    if a.sharpness > 0.0 {
        out = out.unsharpen(1.2, (20.0 * (1.0 - a.sharpness)) as i32);
    }
    let alpha = out.as_rgba8().is_some_and(|i| i.pixels().any(|p| p[3] < 255));
    let mut buf = std::io::Cursor::new(Vec::new());
    if alpha {
        out.write_to(&mut buf, image::ImageFormat::Png).ok()?;
        Some((buf.into_inner(), "png"))
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 92).encode_image(&out.to_rgb8()).ok()?;
        Some((buf.into_inner(), "jpeg"))
    }
}

/// Write adjustments for `image` (rendering the adjusted picture).
pub fn set(app: &mut App, image: ObjId, a: Adjustments) {
    let Some(doc) = app.doc.as_ref() else { return };
    let adjusted = if a.is_default() {
        None
    } else {
        doc.image_original_data(image).and_then(|d| doc.file.data(d)).and_then(|(name, b)| {
            let stem = std::path::Path::new(name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("image".into());
            apply(&b, &a).map(|(bytes, ext)| (bytes, format!("{stem}-adjusted.{ext}")))
        })
    };
    let slide = app.current_slide().map(|s| s.slide);
    if let Some(doc) = app.doc.as_mut() {
        doc.begin_merge(&mut app.hist, "Adjust Image");
        doc.set_image_adjustments(&mut app.hist, image, &a, adjusted);
        doc.end(&mut app.hist);
    }
    if let Some(s) = slide {
        app.bump(s);
    }
}

/// Keynote's slider row: label, slider, value. Returns true when let go (changes are applied
/// on release; the picture is re-rendered then).
pub fn slider(ui: &mut egui::Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.label(theme::bold(label));
    let mut done = false;
    ui.horizontal(|ui| {
        let r = ui.add_sized([ui.available_width() - 60.0, 20.0], egui::Slider::new(v, range).show_value(false));
        ui.label(RichText::new(format!("{:.0}%", *v * 100.0)).color(theme::TEXT_DIM()));
        done = r.drag_stopped() || (r.changed() && !r.dragged());
    });
    done
}

/// View > Show Adjust Image: the full set of adjustments for the selected picture.
pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.show_adjust_image {
        return;
    }
    let image = app.sel.first().copied().filter(|id| app.doc.as_ref().is_some_and(|d| d.file.type_name(*id) == "TSD.ImageArchive"));
    let mut open = true;
    let mut apply_now: Option<Adjustments> = None;
    egui::Window::new("Adjust Image").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).default_width(300.0).open(&mut open).show(ctx, |ui| {
        let Some(id) = image else {
            ui.label(RichText::new("Select a picture to adjust it.").color(theme::TEXT_DIM()));
            return;
        };
        let mut a = app.doc.as_ref().map(|d| d.image_adjustments(id)).unwrap_or_default();
        let mut done = false;
        let mut levels = (a.bottom_level, a.top_level);
        done |= slider(ui, "Exposure", &mut a.exposure, -1.0..=1.0);
        done |= slider(ui, "Contrast", &mut a.contrast, -1.0..=1.0);
        done |= slider(ui, "Saturation", &mut a.saturation, -1.0..=1.0);
        done |= slider(ui, "Highlights", &mut a.highlights, -1.0..=1.0);
        done |= slider(ui, "Shadows", &mut a.shadows, -1.0..=1.0);
        done |= slider(ui, "Sharpness", &mut a.sharpness, 0.0..=1.0);
        done |= slider(ui, "De-noise", &mut a.denoise, 0.0..=1.0);
        done |= slider(ui, "Temperature", &mut a.temperature, -1.0..=1.0);
        done |= slider(ui, "Tint", &mut a.tint, -1.0..=1.0);
        ui.label(theme::bold("Levels"));
        ui.horizontal(|ui| {
            ui.label("Black");
            done |= theme::stepper32(ui, &mut levels.0, 0.01, 0.0..=0.95, "", 2);
            ui.label("White");
            done |= theme::stepper32(ui, &mut levels.1, 0.01, 0.05..=1.0, "", 2);
        });
        a.bottom_level = levels.0;
        a.top_level = levels.1.max(levels.0 + 0.05);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new("Enhance").selected(a.enhance)).clicked() {
                a.enhance = !a.enhance;
                done = true;
            }
            if ui.button("Reset Image").clicked() {
                a = Adjustments::default();
                done = true;
            }
        });
        if done {
            apply_now = Some(a);
        }
    });
    app.show_adjust_image = open;
    if let (Some(a), Some(id)) = (apply_now, image) {
        set(app, id, a);
    }
}

/// Remove Background: everything connected to the picture's edges that is close in colour to
/// the edge colours becomes transparent (with a soft rim). PNG out.
pub fn remove_background(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w < 2 || h < 2 {
        return None;
    }
    let px = |x: u32, y: u32| img.get_pixel(x, y).0;
    let dist = |a: [u8; 4], b: [u8; 4]| ((a[0] as f32 - b[0] as f32).powi(2) + (a[1] as f32 - b[1] as f32).powi(2) + (a[2] as f32 - b[2] as f32).powi(2)).sqrt();
    // the most common edge colours are the background
    let mut seeds: Vec<[u8; 4]> = Vec::new();
    let mut edge = Vec::new();
    for x in 0..w {
        edge.push((x, 0));
        edge.push((x, h - 1));
    }
    for y in 0..h {
        edge.push((0, y));
        edge.push((w - 1, y));
    }
    for &(x, y) in edge.iter().step_by(7) {
        let c = px(x, y);
        if !seeds.iter().any(|s| dist(*s, c) < 24.0) {
            seeds.push(c);
        }
        if seeds.len() > 12 {
            break;
        }
    }
    let tol = 42.0;
    let mut bg = vec![false; (w * h) as usize];
    let mut stack: Vec<(u32, u32)> = edge.into_iter().filter(|&(x, y)| seeds.iter().any(|s| dist(*s, px(x, y)) < tol)).collect();
    while let Some((x, y)) = stack.pop() {
        let i = (y * w + x) as usize;
        if bg[i] {
            continue;
        }
        let c = px(x, y);
        if !seeds.iter().any(|s| dist(*s, c) < tol) {
            continue;
        }
        bg[i] = true;
        if x > 0 {
            stack.push((x - 1, y));
        }
        if x + 1 < w {
            stack.push((x + 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if y + 1 < h {
            stack.push((x, y + 1));
        }
    }
    if bg.iter().filter(|b| **b).count() as f32 > 0.97 * (w * h) as f32 {
        return None; // nothing would be left
    }
    let mut out = img.clone();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            if bg[i] {
                out.get_pixel_mut(x, y).0[3] = 0;
            } else {
                // soften the rim: half transparent next to removed pixels
                let near = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)].iter().any(|&(a, b)| a < w && b < h && bg[(b * w + a) as usize]);
                if near {
                    let p = out.get_pixel_mut(x, y);
                    p.0[3] = p.0[3] / 2;
                }
            }
        }
    }
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(out).write_to(&mut buf, image::ImageFormat::Png).ok()?;
    Some(buf.into_inner())
}
