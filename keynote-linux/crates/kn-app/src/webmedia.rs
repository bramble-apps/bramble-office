//! Insert > Web Video… (a link to a YouTube / Vimeo page, shown as its poster) and
//! Insert > Image Gallery (several pictures in one frame).

use std::sync::mpsc::{channel, Receiver};

use crate::app::App;
use kn_ui::media::web::{fetch, video_link, Fetched};

#[derive(Default)]
pub struct WebMedia {
    /// The Insert > Web Video sheet: the address typed so far.
    pub url_sheet: Option<String>,
    /// Format > Web Video > Replace: the web video the sheet's link replaces.
    pub replacing: Option<kn_format::ObjId>,
    fetching: Option<Receiver<Fetched>>,
    picking: Option<Receiver<Vec<(String, Vec<u8>)>>>,
    /// The gallery the pictures being picked are added to (Format > Gallery > +).
    adding_to: Option<kn_format::ObjId>,
}


pub fn show_url_sheet(app: &mut App) {
    app.webmedia.url_sheet = Some(String::new());
}

/// Format > Gallery > +: more pictures for gallery `id`.
pub fn pick_more(app: &mut App, id: kn_format::ObjId) {
    pick_gallery(app);
    app.webmedia.adding_to = Some(id);
}

pub fn pick_gallery(app: &mut App) {
    app.webmedia.adding_to = None;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let files = rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg"]).set_title("Choose Pictures for the Gallery").pick_files().unwrap_or_default();
        let pics: Vec<(String, Vec<u8>)> = files
            .iter()
            .filter_map(|p| Some((p.file_name()?.to_string_lossy().to_string(), std::fs::read(p).ok()?)))
            .collect();
        let _ = tx.send(pics);
    });
    app.webmedia.picking = Some(rx);
}

/// The URL sheet, plus finishing fetches / picks that came back.
pub fn window(app: &mut App, ctx: &egui::Context) {
    if let Some(mut url) = app.webmedia.url_sheet.take() {
        let mut keep = true;
        let mut go = false;
        let valid = video_link(url.trim());
        // Keynote 15.4's "Add Web Video" sheet (R37)
        egui::Window::new("Add Web Video").title_bar(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                let bad = !url.trim().is_empty() && !valid;
                // a cloud with a play mark, or a warning triangle for a link that isn't a video
                let (r, _) = ui.allocate_exact_size(egui::vec2(56.0, 40.0), egui::Sense::hover());
                let p = ui.painter();
                let c = r.center();
                if bad {
                    let tri = vec![c + egui::vec2(0.0, -17.0), c + egui::vec2(19.0, 16.0), c + egui::vec2(-19.0, 16.0)];
                    p.add(egui::Shape::convex_polygon(tri, egui::Color32::from_rgb(0xf5, 0xc2, 0x11), egui::Stroke::NONE));
                    p.text(c + egui::vec2(0.0, 3.0), egui::Align2::CENTER_CENTER, "!", egui::FontId::proportional(20.0), egui::Color32::BLACK);
                } else {
                    let col = crate::theme::TEXT_DIM();
                    p.circle_filled(c + egui::vec2(-10.0, 4.0), 11.0, col);
                    p.circle_filled(c + egui::vec2(6.0, -4.0), 14.0, col);
                    p.circle_filled(c + egui::vec2(17.0, 6.0), 9.0, col);
                    p.rect_filled(egui::Rect::from_min_max(c + egui::vec2(-10.0, 4.0), c + egui::vec2(17.0, 15.0)), 0.0, col);
                    let tri = vec![c + egui::vec2(-2.0, -4.0), c + egui::vec2(8.0, 3.0), c + egui::vec2(-2.0, 10.0)];
                    p.add(egui::Shape::convex_polygon(tri, crate::theme::PANEL_BG(), egui::Stroke::NONE));
                }
                ui.add_space(6.0);
                if bad {
                    ui.label(egui::RichText::new("You haven't entered a valid YouTube or Vimeo video URL.").strong());
                } else {
                    ui.label(egui::RichText::new("Add videos from YouTube and Vimeo.").strong());
                }
                ui.label(egui::RichText::new("Third party content and services may be subject to additional terms and conditions.").small().color(crate::theme::TEXT_DIM()));
                ui.add_space(8.0);
                let r = ui.add(egui::TextEdit::singleline(&mut url).hint_text("Enter URL").desired_width(f32::INFINITY));
                r.request_focus();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(valid, egui::Button::new(if app.webmedia.replacing.is_some() { "Replace" } else { "Insert" }).fill(crate::theme::ACCENT())).clicked() || (enter && valid) {
                            go = true;
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            keep = false;
                        }
                    });
                });
            });
        });
        if go {
            let (tx, rx) = channel();
            let u = url.trim().to_string();
            std::thread::spawn(move || {
                let _ = tx.send(fetch(u));
            });
            app.webmedia.fetching = Some(rx);
            app.notify("Getting the video's details…");
        } else if keep {
            app.webmedia.url_sheet = Some(url);
        } else {
            app.webmedia.replacing = None;
        }
    }
    if let Some(rx) = app.webmedia.fetching.take() {
        match rx.try_recv() {
            Ok(Ok((url, title, author, poster))) => match app.webmedia.replacing.take() {
                // Replace: the same object gets the new video
                Some(id) => {
                    if let (Some(s), Some(doc)) = (app.current_slide().map(|s| s.slide), app.doc.as_mut()) {
                        doc.begin(&mut app.hist, "Replace Web Video");
                        let _ = doc.replace_image(&mut app.hist, id, poster, "hqdefault.jpeg");
                        doc.make_web_video(&mut app.hist, id, &url, &title, &author);
                        doc.end(&mut app.hist);
                        app.bump(s);
                    }
                }
                None => {
                    // the video's own shape (Keynote's 480 x 360 suits only YouTube's 4:3 still)
                    let aspect = kn_format::objects::image_size(&poster).map(|(w, h)| h as f32 / w.max(1) as f32).filter(|a| (0.2..5.0).contains(a)).unwrap_or(0.75);
                    crate::editing::insert_object(app, kn_model::insert::NewObject::Image(poster, "hqdefault.jpeg".into()));
                    if let (Some(&id), Some(s)) = (app.sel.first(), app.current_slide().map(|s| s.slide)) {
                        let size = app.doc.as_ref().map(|d| d.slide_size());
                        if let Some(doc) = app.doc.as_mut() {
                            doc.begin_merge(&mut app.hist, "Insert Image");
                            // Keynote: 480 wide in the middle of the slide, loading on click
                            if let (Some(sz), Some(mut g)) = (size, doc.scene(&app.slides, app.cur).and_then(|sc| sc.drawables.iter().find(|d| d.id == id).map(|d| d.geom))) {
                                g.w = 480.0;
                                g.h = (480.0 * aspect).round();
                                g.x = (sz.w - g.w) / 2.0;
                                g.y = (sz.h - g.h) / 2.0;
                                doc.set_frame(&mut app.hist, id, g);
                            }
                            doc.make_web_video(&mut app.hist, id, &url, &title, &author);
                            let _ = doc.add_web_video_build(&mut app.hist, s, id);
                            doc.end(&mut app.hist);
                        }
                        app.bump(s);
                    }
                }
            },
            Ok(Err(e)) => app.notify(e),
            Err(std::sync::mpsc::TryRecvError::Empty) => app.webmedia.fetching = Some(rx),
            Err(_) => {}
        }
    }
    if let Some(rx) = app.webmedia.picking.take() {
        match rx.try_recv() {
            Ok(pics) if !pics.is_empty() && app.webmedia.adding_to.is_some() => {
                let id = app.webmedia.adding_to.take().unwrap();
                if let (Some(s), Some(doc)) = (app.current_slide().map(|s| s.slide), app.doc.as_mut()) {
                    let have = doc.scene(&app.slides, app.cur).and_then(|sc| sc.drawables.iter().find_map(|d| match &d.kind {
                        kn_model::DrawableKind::Image(im) if d.id == id => Some(im.gallery.len()),
                        _ => None,
                    })).unwrap_or(1);
                    let n = have + pics.len();
                    doc.begin(&mut app.hist, "Add Images");
                    let r = doc.make_gallery(&mut app.hist, id, pics.into_iter().map(|(n, b)| (b, n)).collect()).and_then(|_| doc.sync_gallery_build(&mut app.hist, s, id, n));
                    doc.end(&mut app.hist);
                    if let Err(e) = r {
                        app.notify(format!("Couldn't add the pictures: {e}"));
                    }
                    app.bump(s);
                }
            }
            Ok(mut pics) if !pics.is_empty() => {
                let (name, first) = pics.remove(0);
                crate::editing::insert_object(app, kn_model::insert::NewObject::Image(first, name));
                if let (Some(&id), Some(s)) = (app.sel.first(), app.current_slide().map(|s| s.slide)) {
                    if let Some(doc) = app.doc.as_mut() {
                        doc.begin_merge(&mut app.hist, "Insert Image");
                        let n = pics.len() + 1;
                        // Keynote gives every new gallery an "Appear" build that steps through its images
                        let r = doc.make_gallery(&mut app.hist, id, pics.into_iter().map(|(n, b)| (b, n)).collect()).and_then(|_| doc.sync_gallery_build(&mut app.hist, s, id, n));
                        doc.end(&mut app.hist);
                        if let Err(e) = r {
                            app.notify(format!("Couldn't make the gallery: {e}"));
                        }
                    }
                    app.bump(s);
                }
            }
            Ok(_) => {}
            Err(std::sync::mpsc::TryRecvError::Empty) => app.webmedia.picking = Some(rx),
            Err(_) => {}
        }
    }
    if app.webmedia.fetching.is_some() || app.webmedia.picking.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}
