//! The Movie / Audio pane (Volume, Repeat, Trim, Poster Frame) and the Image Gallery pane
//! (Caption, the images, each one's description), as Keydeck's, without Keynote's builds; and a
//! selected gallery's ‹ › arrows on the canvas.

use std::collections::HashMap;

use egui::{pos2, vec2, Color32, Rect, RichText};
use kn_ui::media;
use pg_core::kn_format::ObjId;
use pg_core::kn_model::imageedit::MovieProp;
use pg_core::kn_model::{Drawable, DrawableKind};

use crate::app::App;
use crate::objinsp::section;
use kn_ui::theme;

#[derive(Default)]
struct State {
    /// The movie whose length is known: (movie, seconds, when asked).
    movie_len: Option<(ObjId, f32, std::time::Instant)>,
    /// The image each gallery shows (picked with its arrows or in the pane).
    gallery_pos: HashMap<ObjId, usize>,
    gallery_desc: Option<(ObjId, String)>,
    thumbs: HashMap<u64, egui::TextureHandle>,
}

thread_local! {
    static STATE: std::cell::RefCell<State> = std::cell::RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// After the drawables are rebuilt: galleries show the image picked (the file's is the first).
pub fn apply_gallery_pos(drawables: &mut [Drawable]) {
    with(|s| {
        if s.gallery_pos.is_empty() {
            return;
        }
        for d in drawables.iter_mut() {
            let Some(&k) = s.gallery_pos.get(&d.id) else { continue };
            if let DrawableKind::Image(im) = &mut d.kind {
                if let Some(it) = im.gallery.get(k.min(im.gallery.len().saturating_sub(1))) {
                    im.gallery_shown = k.min(im.gallery.len() - 1);
                    im.data_id = it.data_id;
                    im.thumb_id = None;
                }
            }
        }
    });
}

fn show_image(app: &mut App, id: ObjId, k: usize) {
    with(|s| s.gallery_pos.insert(id, k));
    // relayout without marking the document edited
    app.version += 1;
}

pub fn movie_panel(app: &mut App, ui: &mut egui::Ui, d: &Drawable) {
    let DrawableKind::Movie(m) = &d.kind else { return };
    let id = d.id;
    let mut change: Option<MovieProp> = None;
    // the length (ffprobe, once per movie; asked again every 2 s while the file is still
    // being copied to the cache)
    let known = with(|s| match s.movie_len {
        Some((mid, l, at)) if mid == id && (l > 0.0 || at.elapsed().as_secs_f32() < 2.0) => Some(l),
        _ => None,
    });
    let len = known.unwrap_or_else(|| {
        let l = m.movie_data.and_then(|data| app.doc().and_then(|doc| media::cache_file(doc, data))).and_then(|(p, _)| media::duration(&p)).unwrap_or(0.0) as f32;
        with(|s| s.movie_len = Some((id, l, std::time::Instant::now())));
        l
    });
    section(ui, if m.audio_only { "Audio" } else { "Movie" });
    ui.label(theme::bold("Volume"));
    let mut vol = m.volume;
    ui.horizontal(|ui| {
        let r = ui.add_sized([ui.available_width() - 60.0, 20.0], egui::Slider::new(&mut vol, 0.0..=1.0).show_value(false));
        ui.label(RichText::new(format!("{:.0}%", vol * 100.0)).color(theme::TEXT_DIM()));
        if r.drag_stopped() || (r.changed() && !r.dragged()) {
            change = Some(MovieProp::Volume(vol));
        }
    });
    ui.label(theme::bold("Repeat"));
    let names = ["None", "Loop", "Loop Back and Forth"];
    let cur = (m.loop_mode as usize).min(2);
    theme::combo("movie-repeat").selected_text(names[cur]).width(ui.available_width()).show_ui(ui, |ui| {
        for (i, n) in names.iter().enumerate() {
            if ui.selectable_label(i == cur, *n).clicked() && i != cur {
                change = Some(MovieProp::Loop(i as u32));
            }
        }
    });
    if len > 0.0 {
        ui.label(theme::bold("Trim"));
        let (mut a, mut b) = (m.start, if m.end > 0.0 { m.end } else { len });
        let r1 = ui.add(egui::Slider::new(&mut a, 0.0..=len).text("Start").suffix(" s"));
        let r2 = ui.add(egui::Slider::new(&mut b, 0.0..=len).text("End").suffix(" s"));
        if r1.drag_stopped() || r2.drag_stopped() {
            change = Some(MovieProp::Trim(a.min(b - 0.1).max(0.0), b));
        }
        if !m.audio_only {
            ui.label(theme::bold("Poster Frame"));
            let mut t = m.poster_time;
            if ui.add(egui::Slider::new(&mut t, 0.0..=len).suffix(" s")).drag_stopped() {
                let frame = m.movie_data.and_then(|data| app.doc().and_then(|doc| media::cache_file(doc, data))).and_then(|(p, _)| media::frame_jpeg(&p, t as f64));
                match frame {
                    Some(jpeg) => {
                        if let Some(Err(e)) = app.edit("Poster Frame", |doc, h| doc.set_movie_poster(h, id, t, jpeg)) {
                            app.notify(format!("Couldn't set the poster frame: {e}"));
                        }
                    }
                    None => app.notify("Couldn't read that frame (is ffmpeg installed?)"),
                }
            }
        }
    } else {
        ui.label(RichText::new("Trim and Poster Frame need ffmpeg to read the movie.").small().color(theme::TEXT_DIM()));
    }
    if let Some(c) = change {
        app.edit(if m.audio_only { "Audio" } else { "Movie" }, |doc, h| doc.set_movie_prop(h, id, &c));
    }
}

/// A package data file's name as Pages shows it: "Data/gallery-1-452.png" -> "gallery-1.png".
fn shown_file_name(stored: &str) -> String {
    let base = stored.rsplit('/').next().unwrap_or(stored);
    let (stem, ext) = base.rsplit_once('.').unwrap_or((base, ""));
    let stem = match stem.rsplit_once('-') {
        Some((s, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => s,
        _ => stem,
    };
    if ext.is_empty() { stem.to_string() } else { format!("{stem}.{ext}") }
}

/// A small picture of a gallery image for the pane's list (made once).
fn thumb(app: &App, ctx: &egui::Context, data: u64) -> Option<egui::TextureHandle> {
    if let Some(t) = with(|s| s.thumbs.get(&data).cloned()) {
        return Some(t);
    }
    let bytes = app.doc()?.data(data)?;
    let img = image::load_from_memory(&bytes).ok()?.thumbnail(96, 72).to_rgba8();
    let tex = ctx.load_texture(format!("gallery-thumb-{data}"), egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), egui::TextureOptions::LINEAR);
    with(|s| s.thumbs.insert(data, tex.clone()));
    Some(tex)
}

pub fn gallery_panel(app: &mut App, ui: &mut egui::Ui, d: &Drawable) {
    let DrawableKind::Image(im) = &d.kind else { return };
    let (id, items, shown) = (d.id, im.gallery.clone(), im.gallery_shown.min(im.gallery.len().saturating_sub(1)));
    let caption_on = app.doc().and_then(|doc| doc.file.obj(id)).and_then(|m| m.find_super("TSD.DrawableArchive")).is_some_and(|dm| dm.bool("caption_hidden") == Some(false));
    let mut caption = caption_on;
    let mode = if im.gallery_caption_mode == 2 { 2u8 } else { 1 };
    type Change = Box<dyn FnOnce(&mut pg_core::kn_model::Document, &mut pg_core::kn_model::edit::History)>;
    let mut change: Option<Change> = None;
    section(ui, "Appearance");
    if theme::checkbox(ui, &mut caption, "Caption").changed() {
        change = Some(Box::new(move |doc, h| doc.set_gallery_caption(h, id, caption, mode)));
    }
    if caption {
        ui.indent("gallery-caption-mode", |ui| {
            for (m, label) in [(1u8, "Individual caption per image"), (2, "Same caption for all images")] {
                if ui.radio(mode == m, label).clicked() && mode != m {
                    change = Some(Box::new(move |doc, h| doc.set_gallery_caption(h, id, true, m)));
                }
            }
        });
    }
    section(ui, "Images");
    let mut pick: Option<usize> = None;
    egui::Frame::new().fill(theme::gray(0x1e)).corner_radius(6).inner_margin(4).show(ui, |ui| {
        ui.set_min_height(180.0);
        for (k, it) in items.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), egui::Sense::click());
            if k == shown {
                ui.painter().rect_filled(r, 4.0, theme::gray(0x3a));
            }
            let t = Rect::from_min_size(r.min + vec2(8.0, 4.0), vec2(44.0, 32.0));
            match it.data_id.and_then(|data| thumb(app, ui.ctx(), data)) {
                Some(tex) => {
                    ui.painter().image(tex.id(), t, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    ui.painter().rect_filled(t, 2.0, theme::gray(0x50));
                }
            }
            let name = it.data_id.and_then(|data| app.doc().and_then(|doc| doc.data_name(data)).map(shown_file_name)).unwrap_or_else(|| format!("Image {}", k + 1));
            ui.painter().text(pos2(t.max.x + 10.0, r.center().y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(13.0), theme::TEXT());
            if resp.clicked() {
                pick = Some(k);
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button(" + ").on_hover_text("Add images to the gallery").clicked() {
            let files = crate::app::file_dialog(|d| d.add_filter("Pictures", media::IMAGE_EXT).set_title("Choose Pictures for the Gallery").pick_files()).unwrap_or_default();
            let pics: Vec<(Vec<u8>, String)> = files.iter().filter_map(|p| Some((std::fs::read(p).ok()?, p.file_name()?.to_string_lossy().to_string()))).collect();
            if !pics.is_empty() {
                if let Some(Err(e)) = app.edit("Add Images", |doc, h| doc.make_gallery(h, id, pics)) {
                    app.notify(format!("Couldn't add the pictures: {e}"));
                }
            }
        }
        if ui.add_enabled(items.len() > 1, egui::Button::new(" − ")).on_hover_text("Remove the selected image").clicked() {
            change = Some(Box::new(move |doc, h| {
                doc.remove_gallery_image(h, id, shown);
            }));
            with(|s| s.gallery_pos.insert(id, shown.min(items.len().saturating_sub(2))));
        }
    });
    if let Some(k) = pick {
        show_image(app, id, k);
    }
    section(ui, "Image Description");
    if let Some(it) = items.get(shown) {
        let mut buf = with(|s| match &s.gallery_desc {
            Some((iid, b)) if *iid == it.id => b.clone(),
            _ => it.description.clone(),
        });
        let r = ui.add(egui::TextEdit::multiline(&mut buf).hint_text("Describe this image for people who use assistive technology").desired_rows(4).desired_width(f32::INFINITY));
        with(|s| s.gallery_desc = Some((it.id, buf.clone())));
        if r.lost_focus() && buf != it.description {
            let item = it.id;
            change = Some(Box::new(move |doc, h| doc.set_gallery_description(h, item, &buf)));
        }
    }
    if let Some(f) = change {
        app.edit("Gallery", |doc, h| f(doc, h));
    }
}

/// Format > Web Video: its title and author, the link, Replace.
pub fn web_video_panel(ui: &mut egui::Ui, d: &Drawable) {
    let DrawableKind::Image(im) = &d.kind else { return };
    let Some(w) = im.web_video.clone() else { return };
    ui.add_space(6.0);
    ui.label(theme::bold(if w.title.is_empty() { "Web Video" } else { w.title.as_str() }).size(15.0));
    if !w.author.is_empty() {
        ui.label(RichText::new(&w.author).color(theme::TEXT_DIM()));
    }
    ui.hyperlink_to(RichText::new(&w.url).small(), &w.url);
    ui.add_space(4.0);
    if ui.add_sized([ui.available_width(), 26.0], egui::Button::new("Replace")).clicked() {
        crate::media::replace_web_video(d.id, &w.url);
    }
    ui.label(RichText::new("Third party content and services may be subject to additional terms and conditions.").small().color(theme::TEXT_DIM()));
}

/// A selected gallery's arrows (and "2 of 5") on the canvas. Returns true when the pointer is
/// over them.
pub fn gallery_controls(app: &mut App, ui: &mut egui::Ui) -> bool {
    let Some(l) = app.layout.as_ref() else { return false };
    let Some(id) = app.canvas.obj else { return false };
    let Some(d) = app.drawables.iter().find(|d| d.id == id) else { return false };
    let DrawableKind::Image(im) = &d.kind else { return false };
    let n = im.gallery.len();
    if n < 2 {
        return false;
    }
    let Some(&pi) = app.obj_page.get(&id) else { return false };
    let g = d.geom;
    let r = Rect::from_min_max(app.canvas.page_to_screen(l, pi, pos2(g.x, g.y)), app.canvas.page_to_screen(l, pi, pos2(g.x + g.w, g.y + g.h)));
    let shown = im.gallery_shown.min(n - 1);
    let painter = ui.painter_at(app.canvas.view_rect());
    let mut hot = false;
    let mut go: Option<usize> = None;
    for (left, x) in [(true, r.min.x + 22.0), (false, r.max.x - 22.0)] {
        let b = Rect::from_center_size(pos2(x, r.center().y), vec2(30.0, 30.0));
        let resp = ui.interact(b, ui.scope_id().with(("gallery-arrow", id, left)), egui::Sense::click());
        hot |= resp.hovered();
        painter.circle_filled(b.center(), 15.0, Color32::from_black_alpha(if resp.hovered() { 170 } else { 120 }));
        painter.text(b.center(), egui::Align2::CENTER_CENTER, if left { "‹" } else { "›" }, egui::FontId::proportional(22.0), Color32::WHITE);
        if resp.clicked() {
            go = Some(if left { (shown + n - 1) % n } else { (shown + 1) % n });
        }
    }
    let label = format!("{} of {n}", shown + 1);
    let lr = Rect::from_center_size(pos2(r.center().x, r.max.y - 18.0), vec2(label.len() as f32 * 7.5 + 20.0, 22.0));
    painter.rect_filled(lr, 11.0, Color32::from_black_alpha(130));
    painter.text(lr.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), Color32::WHITE);
    if let Some(k) = go {
        show_image(app, id, k);
    }
    hot
}
