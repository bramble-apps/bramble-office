//! File menu extras: Revert To > Browse All Versions (the old file is kept each time it is
//! saved over, as Keydeck does), Change File Type (Single File / Package), Move To…,
//! Reduce File Size… and Print Without Preview.

use std::path::{Path, PathBuf};

use crate::app::App;

fn versions_dir(path: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).hash(&mut h);
    Some(base.join("numdeck").join("versions").join(format!("{:016x}", h.finish())))
}

/// Before saving over `path`: keep the old file as a version (the latest 30).
pub fn keep_version(path: &Path) {
    if !path.is_file() {
        return;
    }
    let Some(dir) = versions_dir(path) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let _ = std::fs::copy(path, dir.join(format!("{secs}.numbers")));
    let mut all = list(path);
    while all.len() > 30 {
        if let Some((p, _)) = all.pop() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Kept versions, newest first: (file, seconds since 1970).
pub fn list(path: &Path) -> Vec<(PathBuf, u64)> {
    let Some(dir) = versions_dir(path) else { return Vec::new() };
    let mut v: Vec<(PathBuf, u64)> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| Some((e.path(), e.path().file_stem()?.to_str()?.parse().ok()?))).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

fn when(secs: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    match now.saturating_sub(secs) {
        a @ 0..=59 => if a < 10 { "Just now".into() } else { format!("{a} seconds ago") },
        a @ 60..=3599 => format!("{} minutes ago", a / 60),
        a @ 3600..=86_399 => format!("{} hours ago", a / 3600),
        a => format!("{} days ago", a / 86_400),
    }
}

/// Revert To > Browse All Versions: the kept versions, newest first; Restore puts one back
/// (the file as it is now is kept as a version first).
pub fn versions_window(app: &mut App, ctx: &egui::Context) {
    if !app.show_versions {
        return;
    }
    let Some(path) = app.doc().and_then(|d| d.path.clone()) else {
        app.show_versions = false;
        return;
    };
    let mut open = true;
    let mut restore: Option<PathBuf> = None;
    egui::Window::new("All Versions").collapsible(false).resizable(false).default_pos(ctx.content_rect().center()).open(&mut open).show(ctx, |ui| {
        ui.set_width(340.0);
        let all = list(&path);
        if all.is_empty() {
            ui.label("No earlier versions yet: a version is kept each time the spreadsheet is saved over.");
        }
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (p, secs) in all {
                ui.horizontal(|ui| {
                    ui.label(when(secs));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Restore").clicked() {
                            restore = Some(p.clone());
                        }
                    });
                });
            }
        });
    });
    app.show_versions = open;
    if let Some(v) = restore {
        keep_version(&path);
        match std::fs::copy(&v, &path) {
            Ok(_) => {
                app.open(&path);
                app.show_versions = false;
                app.notify("Restored the earlier version");
            }
            Err(e) => app.notify(format!("Couldn't restore: {e}")),
        }
    }
}

/// File > Advanced > Change File Type: Single File / Package (saved at once when the
/// spreadsheet has a file).
pub fn set_package(app: &mut App, package: bool) {
    let Some(w) = app.wb.as_mut() else { return };
    if w.doc.package == package {
        return;
    }
    w.doc.package = package;
    let Some(p) = w.doc.path.clone().filter(|p| p.exists()) else { return };
    // the other kind replaces it: a package is a folder, a single file a zip
    let res = (|| -> Result<(), String> {
        let tmp = p.with_extension("numdeck-tmp");
        w.doc.save_as(&tmp).map_err(|e| e.to_string())?;
        if p.is_dir() {
            std::fs::remove_dir_all(&p).map_err(|e| e.to_string())?;
        } else {
            std::fs::remove_file(&p).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
        w.doc.path = Some(p.clone());
        Ok(())
    })();
    match res {
        Ok(()) => app.notify(if package { "The spreadsheet is now a package" } else { "The spreadsheet is now a single file" }),
        Err(e) => app.notify(format!("Couldn't change the file type: {e}")),
    }
}

/// File > Move To…: the spreadsheet saved in the new place, the old file gone.
pub fn move_to(app: &mut App) {
    let Some(old) = app.doc().and_then(|d| d.path.clone()) else {
        app.notify("Save the spreadsheet first.");
        return;
    };
    let name = old.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled.numbers".into());
    let Some(new) = crate::app::file_dialog(|d| d.add_filter("Numbers spreadsheet", &["numbers"]).set_file_name(&name).save_file()) else { return };
    relocate(app, new);
}

/// The spreadsheet saved as `new` and the file it was in removed (rename or move); a
/// spreadsheet never saved is just saved there.
pub fn relocate(app: &mut App, new: std::path::PathBuf) {
    // an Excel workbook is renamed / moved as itself
    if let Some(x) = app.excel.clone() {
        let new = new.with_extension(x.path.extension().unwrap_or_default());
        if new == x.path {
            return;
        }
        if new.exists() {
            app.notify(format!("“{}” is already there; choose another name.", new.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
            return;
        }
        app.save_excel(&new, x.compat);
        if app.excel.as_ref().is_some_and(|e| e.path == new) && x.path.exists() {
            let _ = std::fs::remove_file(&x.path);
        }
        return;
    }
    let old = app.doc().and_then(|d| d.path.clone());
    if old.as_ref() == Some(&new) {
        return;
    }
    if new.exists() {
        app.notify(format!("“{}” is already there; choose another name.", new.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        return;
    }
    crate::canvas::commit(app);
    let Some(w) = app.wb.as_mut() else { return };
    match w.doc.save_as(&new) {
        Ok(()) => {
            if let Some(old) = old.filter(|o| o.exists()) {
                let _ = if old.is_dir() { std::fs::remove_dir_all(&old) } else { std::fs::remove_file(&old) };
            }
            app.remember(&new);
        }
        Err(e) => app.notify(format!("Couldn't save there: {e}")),
    }
}

/// File > Reduce File Size…: pictures far bigger than they are ever shown are scaled down
/// (to twice their largest size on any sheet; JPEG, or PNG when transparent). Keydeck's.
pub fn reduce_file_size(app: &mut App) {
    crate::canvas::commit(app);
    let Some(w) = app.wb.as_ref() else { return };
    let mut shown: std::collections::HashMap<u64, (f32, f32)> = std::collections::HashMap::new();
    fn walk(ds: &[nm_core::kn_model::Drawable], shown: &mut std::collections::HashMap<u64, (f32, f32)>) {
        for d in ds {
            match &d.kind {
                nm_core::kn_model::DrawableKind::Image(img) => {
                    if let Some(id) = img.data_id {
                        let e = shown.entry(id).or_insert((0.0, 0.0));
                        e.0 = e.0.max(d.geom.w * 2.0);
                        e.1 = e.1.max(d.geom.h * 2.0);
                    }
                }
                nm_core::kn_model::DrawableKind::Group(kids) => walk(kids, shown),
                _ => {}
            }
        }
    }
    for s in w.sheets() {
        walk(&w.drawables(&s), &mut shown);
    }
    let mut saved = 0i64;
    let mut changed = Vec::new();
    for (id, (sw, sh)) in shown {
        let Some((name, bytes)) = w.doc.file.data(id).map(|(n, b)| (n.to_string(), b.into_owned())) else { continue };
        let Ok(img) = image::load_from_memory(&bytes) else { continue };
        if (img.width() as f32) < sw * 1.5 || bytes.len() < 300_000 {
            continue;
        }
        let k = (sw / img.width() as f32).max(sh / img.height() as f32).min(1.0);
        let (nw, nh) = ((img.width() as f32 * k).round().max(1.0) as u32, (img.height() as f32 * k).round().max(1.0) as u32);
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
        app.notify("The spreadsheet can\u{2019}t be made any smaller");
        return;
    }
    app.edit("Reduce File Size", |d, h| {
        for (id, name, bytes, png) in changed {
            let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("image".into());
            d.replace_data(h, id, bytes, &format!("{stem}-small.{}", if png { "png" } else { "jpeg" }));
        }
    });
    app.notify(format!("Reduced the file by {:.1} MB", saved as f64 / 1_000_000.0));
}

/// File > Print Without Preview: the spreadsheet as a PDF (one page per sheet) to the default
/// printer, in the background.
pub fn print_now(app: &mut App) {
    crate::canvas::commit(app);
    let Some(w) = app.wb.as_ref() else { return };
    let out = std::env::temp_dir().join(format!("numdeck-print-{}.pdf", std::process::id()));
    match crate::pdf::export_with(w, &out, app.canvas.tiles.extras.clone()) {
        Ok(_) => {
            std::thread::spawn(move || {
                let _ = std::process::Command::new("lp").arg(&out).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
            });
            app.notify("Sent to the printer");
        }
        Err(e) => app.notify(format!("Couldn't print: {e}")),
    }
}

/// File > Print…: Numbers' Print Setup (the current sheet's paper, orientation, scale or Fit,
/// margins, page order, page numbers, backgrounds, comments) with its page count, then Print
/// or Export PDF.
pub fn print_setup(app: &mut App, ctx: &egui::Context) {
    if !app.print_setup_open {
        return;
    }
    let Some(sheet) = app.sheets().get(app.sheet).cloned() else { return };
    let Some(before) = app.doc().map(|d| nm_core::sheets::page_setup(d, sheet.id)) else { return };
    let mut p = before.clone();
    let pages = app.wb.as_ref().map(|w| crate::pdf::page_count(w, &sheet)).unwrap_or(1);
    let mut open = true;
    let (mut print, mut export) = (false, false);
    const PAPERS: [(&str, (f32, f32)); 5] = [("US Letter", (612.0, 792.0)), ("US Legal", (612.0, 1008.0)), ("A4", (595.0, 842.0)), ("A3", (842.0, 1191.0)), ("Tabloid", (792.0, 1224.0))];
    egui::Window::new("Print Setup").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).default_width(340.0).open(&mut open).show(ctx, |ui| {
        ui.label(egui::RichText::new(format!("\u{201c}{}\u{201d}: {pages} page{}", sheet.name, if pages == 1 { "" } else { "s" })).strong());
        ui.add_space(6.0);
        egui::Grid::new("print-setup").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Paper");
            let cur = PAPERS.iter().find(|x| (x.1 .0 - p.paper.0.min(p.paper.1)).abs() < 1.0 && (x.1 .1 - p.paper.0.max(p.paper.1)).abs() < 1.0).map(|x| x.0).unwrap_or("Custom");
            kn_ui::theme::combo("paper").selected_text(cur).width(180.0).show_ui(ui, |ui| {
                for (n, sz) in PAPERS {
                    if ui.selectable_label(n == cur, n).clicked() {
                        p.paper = sz;
                    }
                }
            });
            ui.end_row();
            ui.label("Orientation");
            ui.allocate_ui(egui::vec2(220.0, 36.0), |ui| {
                // (the suite's inner switch, as Letters' Portrait | Landscape)
                let mut k = usize::from(!p.portrait);
                crate::objinsp::seg_small(ui, &["Portrait", "Landscape"], &mut k);
                p.portrait = k == 0;
            });
            ui.end_row();
            ui.label("Content Scale");
            ui.horizontal(|ui| {
                kn_ui::theme::checkbox(ui, &mut p.autofit, "Fit");
                ui.add_enabled_ui(!p.autofit, |ui| {
                    let mut pc = (p.scale * 100.0) as f64;
                    if kn_ui::theme::stepper(ui, &mut pc, 5.0, 10.0..=400.0, " %", 0) {
                        p.scale = pc as f32 / 100.0;
                    }
                });
            });
            ui.end_row();
            ui.label("Page Order");
            ui.allocate_ui(egui::vec2(220.0, 36.0), |ui| {
                let mut k = usize::from(!p.down_first);
                crate::objinsp::seg_small(ui, &["Down, Over", "Over, Down"], &mut k);
                p.down_first = k == 0;
            });
            ui.end_row();
            ui.label("Margins");
            ui.vertical(|ui| {
                for (i, n) in ["Top", "Left", "Bottom", "Right"].iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(*n);
                        let mut v = p.margins[i] as f64;
                        if kn_ui::theme::stepper(ui, &mut v, 6.0, 0.0..=144.0, " pt", 0) {
                            p.margins[i] = v as f32;
                        }
                    });
                }
            });
            ui.end_row();
            ui.label("Page Numbers");
            ui.vertical(|ui| {
                kn_ui::theme::checkbox(ui, &mut p.page_numbers, "Show page numbers");
                let mut custom = p.start_page.is_some();
                if kn_ui::theme::checkbox(ui, &mut custom, "Start at").changed() {
                    p.start_page = custom.then_some(1);
                }
                if let Some(n) = p.start_page.as_mut() {
                    let mut v = *n as f64;
                    if kn_ui::theme::stepper(ui, &mut v, 1.0, 1.0..=9999.0, "", 0) {
                        *n = v as i32;
                    }
                }
            });
            ui.end_row();
            ui.label("Options");
            ui.vertical(|ui| {
                kn_ui::theme::checkbox(ui, &mut p.repeat_headers, "Repeat table headers");
                kn_ui::theme::checkbox(ui, &mut p.backgrounds, "Print backgrounds");
                kn_ui::theme::checkbox(ui, &mut p.comments, "Print comments");
            });
            ui.end_row();
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            export = ui.button("Export PDF…").clicked();
            print = ui.add(egui::Button::new(egui::RichText::new("Print").color(egui::Color32::WHITE)).fill(kn_ui::theme::ACCENT())).clicked();
        });
    });
    if p != before {
        app.merge_next_edit = true;
        app.edit("Print Setup", |d, h| nm_core::sheets::set_page_setup(d, h, sheet.id, &p));
        app.merge_next_edit = false;
    }
    if print {
        print_now(app);
    }
    if export {
        app.export_pdf_dialog();
    }
    app.print_setup_open = open && !print;
}
