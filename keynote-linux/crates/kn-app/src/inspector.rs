//! Right-hand inspector: Format (Slide / Style / Text / Arrange), Animate, Document.

use egui::{Color32, RichText};
use kn_model::format::{Dash, FillSpec, StrokeSpec, TextProp};
use kn_model::{Align, Color, DrawableKind, Fill, Geometry, VAlign};

use crate::app::{App, InspectorKind};
use crate::theme;

#[derive(Default)]
pub struct InspectorState {
    pub format_tab: usize,
    /// Format > Gallery: image list thumbnails, and the Image Description being typed.
    pub gallery_thumbs: std::collections::HashMap<u64, egui::TextureHandle>,
    pub gallery_desc: Option<(u64, String)>,
    pub animate_tab: usize,
    pub doc_tab: usize,
    /// The floating Build Order window (Animate > Build Order button, View > Show Build Order).
    pub show_build_order: bool,
    /// Step selected in the Build Order window.
    pub build_order_sel: usize,
    /// Document > Slide Size > Custom Slide Size… sheet: width and height being typed.
    pub custom_size: Option<(f32, f32)>,
    /// Background: Standard (0) / Dynamic (1) shown, dynamic preset page, preset thumbnails.
    pub bg_tab: Option<usize>,
    pub dyn_page: usize,
    /// The slide the preset page was chosen for (a new slide opens on its preset's page).
    pub dyn_page_slide: Option<kn_format::ObjId>,
    /// Chart Axis tab: Value (0) / Category (1) / Value 2; Series tab: the series shown.
    pub charts: kn_ui::chartpanes::ChartPanes,
    /// The selected movie's length in seconds (looked up once).
    pub movie_len: Option<(kn_format::ObjId, f32, std::time::Instant)>,
    pub dyn_thumbs: std::collections::HashMap<(kn_format::ObjId, u64), egui::TextureHandle>,
}

/// Segmented control like Keynote's: one capsule, the selected tab a blue pill, thin dividers
/// between unselected neighbours.
fn tabs(ui: &mut egui::Ui, labels: &[&str], cur: &mut usize) {
    segmented(ui, labels, cur, 16.0);
}

/// Keynote's inner switches (Style | Layout, Standard | Dynamic): rounded rectangles.
fn seg_small(ui: &mut egui::Ui, labels: &[&str], cur: &mut usize) {
    segmented(ui, labels, cur, 7.0);
}

fn segmented(ui: &mut egui::Ui, labels: &[&str], cur: &mut usize, radius: f32) {
    if *cur >= labels.len() {
        *cur = 0;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::hover());
    let p = ui.painter().clone();
    if crate::glass::enabled() && radius >= 12.0 {
        // the inspector's tabs on liquid glass (no lens: the panel under it is plain)
        for s in crate::glass::shapes(ui.ctx(), rect, radius, false) {
            p.add(s);
        }
    } else {
        p.rect_filled(rect, radius, crate::theme::gray(0x33));
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
            p.rect_filled(r.shrink(2.0), radius - 2.0, crate::theme::gray(0x40));
        }
        if i > 0 && !sel && *cur != i - 1 {
            p.line_segment([egui::pos2(r.min.x, r.min.y + 8.0), egui::pos2(r.min.x, r.max.y - 8.0)], egui::Stroke::new(1.0, crate::theme::gray(0x55)));
        }
        crate::theme::centered(&p, r.center(), *l, egui::FontId::proportional(14.0), if sel { Color32::WHITE } else { theme::TEXT() });
        if resp.clicked() {
            *cur = i;
        }
    }
    ui.add_space(10.0);
}

/// Format > Gallery (Keynote 15.4, R29): Title and Caption (one per image or the same for all),
/// the images (pick one to show it, + to add more, − to remove it) and the shown image's
/// description.
fn gallery_panel(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
    let DrawableKind::Image(im) = &d.kind else { return };
    let (id, items, shown) = (d.id, im.gallery.clone(), im.gallery_shown.min(im.gallery.len().saturating_sub(1)));
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    let flags = app.doc.as_ref().and_then(|doc| doc.file.obj(id)).and_then(|m| m.find_super("TSD.DrawableArchive")).map(|dm| (dm.bool("title_hidden") == Some(false) && dm.ref_id("title").is_some_and(|c| app.doc.as_ref().is_some_and(|doc| doc.file.type_name(c) == "TSA.CaptionInfoArchive")), dm.bool("caption_hidden") == Some(false)));
    let (mut title, mut caption) = flags.unwrap_or((false, false));
    let mode = if im.gallery_caption_mode == 2 { 2u8 } else { 1 };
    section(ui, "Appearance");
    let mut change: Option<Box<dyn FnOnce(&mut kn_model::Document, &mut kn_model::edit::History)>> = None;
    if theme::checkbox(ui, &mut title, "Title").changed() {
        change = Some(Box::new(move |doc, h| {
            doc.set_caption(h, slide, id, true, title);
        }));
    }
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
            let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::click());
            if k == shown {
                ui.painter().rect_filled(r, 4.0, theme::gray(0x3a));
            }
            let thumb = egui::Rect::from_min_size(r.min + egui::vec2(8.0, 4.0), egui::vec2(44.0, 32.0));
            if let Some(tex) = it.data_id.and_then(|data| gallery_thumb(app, ui.ctx(), data)) {
                ui.painter().image(tex.id(), thumb, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            } else {
                ui.painter().rect_filled(thumb, 2.0, theme::gray(0x50));
            }
            let name = it.data_id.and_then(|data| app.doc.as_ref().and_then(|doc| doc.data_name(data)).map(shown_file_name)).unwrap_or_else(|| format!("Image {}", k + 1));
            ui.painter().text(egui::pos2(thumb.max.x + 10.0, r.center().y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(13.0), theme::TEXT());
            if resp.clicked() {
                pick = Some(k);
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button(" + ").on_hover_text("Add images to the gallery").clicked() {
            crate::webmedia::pick_more(app, id);
        }
        if ui.add_enabled(items.len() > 1, egui::Button::new(" − ")).on_hover_text("Remove the selected image").clicked() {
            let n = items.len() - 1;
            change = Some(Box::new(move |doc, h| {
                if doc.remove_gallery_image(h, id, shown) {
                    let _ = doc.sync_gallery_build(h, slide, id, n);
                }
            }));
            app.gallery_pos.insert(id, shown.min(n - 1));
        }
    });
    if let Some(k) = pick {
        app.gallery_pos.insert(id, k);
        app.bump(slide);
    }
    section(ui, "Image Description");
    if let Some(it) = items.get(shown) {
        let buf = match &mut app.insp.gallery_desc {
            Some((iid, b)) if *iid == it.id => b,
            other => {
                *other = Some((it.id, it.description.clone()));
                &mut other.as_mut().unwrap().1
            }
        };
        let r = ui.add(egui::TextEdit::multiline(buf).desired_rows(4).desired_width(f32::INFINITY));
        if r.lost_focus() && *buf != it.description {
            let (item, text) = (it.id, buf.clone());
            change = Some(Box::new(move |doc, h| doc.set_gallery_description(h, item, &text)));
        }
    }
    if let Some(f) = change {
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Gallery");
            f(doc, &mut app.hist);
            doc.end(&mut app.hist);
        }
        app.bump(slide);
    }
}

/// A package data file's name as Keynote shows it: "Data/gallery-1-452.png" -> "gallery-1.png".
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
fn gallery_thumb(app: &mut App, ctx: &egui::Context, data: u64) -> Option<egui::TextureHandle> {
    if let Some(t) = app.insp.gallery_thumbs.get(&data) {
        return Some(t.clone());
    }
    let bytes = app.doc.as_ref()?.data(data)?;
    let img = image::load_from_memory(&bytes).ok()?.thumbnail(96, 72).to_rgba8();
    let tex = ctx.load_texture(format!("gallery-thumb-{data}"), egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), egui::TextureOptions::LINEAR);
    app.insp.gallery_thumbs.insert(data, tex.clone());
    Some(tex)
}

/// Format > Web Video (Keynote 15.4, R37): the video's title and author, its link, Replace,
/// Title / Caption and "Load video on click" (the video's start build).
fn web_video_panel(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
    let DrawableKind::Image(im) = &d.kind else { return };
    let Some(w) = im.web_video.clone() else { return };
    let id = d.id;
    ui.add_space(6.0);
    ui.label(theme::bold(if w.title.is_empty() { "Web Video" } else { w.title.as_str() }).size(15.0));
    if !w.author.is_empty() {
        ui.label(RichText::new(&w.author).color(theme::TEXT_DIM()));
    }
    ui.hyperlink_to(RichText::new(&w.url).small(), &w.url);
    ui.add_space(4.0);
    if ui.add_sized([ui.available_width(), 26.0], egui::Button::new("Replace")).clicked() {
        app.webmedia.replacing = Some(id);
        app.webmedia.url_sheet = Some(w.url.clone());
    }
    ui.label(RichText::new("Third party content and services may be subject to additional terms and conditions.").small().color(theme::TEXT_DIM()));
    ui.separator();
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    // Title / Caption, as for pictures
    let shown = |app: &App, field: &str, hidden: &str| {
        app.doc.as_ref().and_then(|doc| doc.file.obj(id)).and_then(|m| m.find_super("TSD.DrawableArchive")).is_some_and(|dm| {
            dm.bool(hidden) == Some(false) && dm.ref_id(field).is_some_and(|c| app.doc.as_ref().is_some_and(|doc| doc.file.type_name(c) == "TSA.CaptionInfoArchive"))
        })
    };
    let (mut t, mut c) = (shown(app, "title", "title_hidden"), shown(app, "caption", "caption_hidden"));
    let mut change = None;
    if theme::checkbox(ui, &mut t, "Title").changed() {
        change = Some((true, t));
    }
    if theme::checkbox(ui, &mut c, "Caption").changed() {
        change = Some((false, c));
    }
    if let Some((title, on)) = change {
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, if title { "Title" } else { "Caption" });
            doc.set_caption(&mut app.hist, slide, id, title, on);
            doc.end(&mut app.hist);
        }
        app.bump(slide);
    }
    ui.separator();
    // "Load video on click": the build that starts it at a click in the show
    let build = app.doc.as_ref().and_then(|doc| doc.web_video_build(slide, id));
    let mut on_click = build.is_some();
    if theme::checkbox(ui, &mut on_click, "Load video on click").changed() {
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Load video on click");
            match (on_click, build) {
                (true, None) => {
                    let _ = doc.add_web_video_build(&mut app.hist, slide, id);
                }
                (false, Some(b)) => doc.remove_build(&mut app.hist, slide, b),
                _ => {}
            }
            doc.end(&mut app.hist);
        }
        app.bump(slide);
    }
}

/// Format > Live Video: which camera it shows, and the preview on the slide while editing.
fn live_video_panel(app: &mut App, ui: &mut egui::Ui) {
    section(ui, "Live Video");
    ui.label(theme::bold("Source"));
    let cams = crate::media::cameras();
    let cur = crate::media::camera_device();
    let name = cams.iter().find(|(d, _)| *d == cur).map(|(_, n)| n.clone()).unwrap_or_else(|| if cams.is_empty() { "No Camera".into() } else { cur.clone() });
    egui::ComboBox::from_id_salt("live-source").width(ui.available_width()).selected_text(name).show_ui(ui, |ui| {
        for (dev, n) in &cams {
            if ui.selectable_label(*dev == cur, n).clicked() {
                app.settings.camera = dev.clone();
                app.settings.save();
                crate::media::set_camera_device(dev);
                // restart the preview on the new camera
                app.live_cam = None;
            }
        }
    });
    if cams.is_empty() {
        ui.label(egui::RichText::new("No camera found. Live video shows its placeholder.").small().weak());
    }
    ui.add_space(8.0);
    let mut on = app.live_preview;
    if ui.checkbox(&mut on, "Show Live Video Preview").changed() {
        app.live_preview = on;
        if !on {
            app.live_cam = None;
        }
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.separator();
    ui.label(theme::bold(title).size(14.0));
    ui.add_space(2.0);
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| match app.inspector {
        InspectorKind::Format => format(app, ui),
        InspectorKind::Animate => animate(app, ui),
        InspectorKind::Document => document(app, ui),
    });
}

fn format(app: &mut App, ui: &mut egui::Ui) {
    let Some(scene) = app.scene(app.cur) else { return };
    let selected: Vec<_> = scene.drawables.iter().filter(|d| app.sel.contains(&d.id)).cloned().collect();
    if selected.is_empty() {
        slide_format(app, ui, &scene);
        return;
    }
    let d = &selected[0];
    let labels: Vec<&str> = match &d.kind {
        DrawableKind::Shape(s) if s.is_line => vec!["Style", "Arrange"],
        DrawableKind::Shape(_) => vec!["Style", "Text", "Arrange"],
        DrawableKind::Image(im) if !im.gallery.is_empty() => vec!["Gallery", "Arrange"],
        DrawableKind::Image(im) if im.web_video.is_some() => vec!["Web Video", "Arrange"],
        DrawableKind::Image(_) => vec!["Style", "Image", "Arrange"],
        DrawableKind::Movie(m) if m.live => vec!["Style", "Live Video", "Arrange"],
        DrawableKind::Movie(_) => vec!["Style", "Movie", "Arrange"],
        DrawableKind::Table(_) => vec!["Table", "Cell", "Text", "Arrange"],
        DrawableKind::Chart(_) => vec!["Chart", "Axis", "Series", "Arrange"],
        _ => vec!["Arrange"],
    };
    let mut tab = app.insp.format_tab;
    tabs(ui, &labels, &mut tab);
    app.insp.format_tab = tab;
    match labels[tab] {
        "Arrange" => arrange(app, ui, &selected),
        "Style" => style(app, ui, d),
        "Text" => text(app, ui, d),
        "Image" => image_panel(app, ui, d),
        "Gallery" => gallery_panel(app, ui, d),
        "Web Video" => web_video_panel(app, ui, d),
        "Movie" => movie_panel(app, ui, d),
        "Live Video" => live_video_panel(app, ui),
        "Table" => table_panel(app, ui, d),
        "Chart" | "Axis" | "Series" => chart_panes(app, ui, d, labels[tab]),
        "Cell" => cell_panel(app, ui, d),
        _ => {}
    }
}

/// Format > Chart / Axis / Series: the panes Keydeck shares with Values and Letters.
fn chart_panes(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable, tab: &str) {
    use kn_ui::chartpanes::{self, PaneAction};
    let DrawableKind::Chart(c) = &d.kind else { return };
    let id = d.id;
    let mut st = app.insp.charts;
    let Some(doc) = app.doc.as_mut() else { return };
    let act = match tab {
        "Chart" => chartpanes::chart_tab(ui, doc, &mut app.hist, id, c),
        "Axis" => chartpanes::axis_tab(ui, doc, &mut app.hist, id, c, &mut st),
        _ => chartpanes::series_tab(ui, doc, &mut app.hist, id, c, &mut st),
    };
    app.insp.charts = st;
    match act {
        PaneAction::EditData => crate::chartedit::open(app, id),
        PaneAction::Changed => {
            if let Some(s) = app.current_slide().map(|s| s.slide) {
                app.bump(s);
            }
        }
        PaneAction::None => {}
    }
}

/// Table Cell tab: the selected cell's content (edited in place) and its data format.
fn cell_panel(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
    use kn_model::tablefmt::{BorderEdges, CellFormat};
    let DrawableKind::Table(t) = &d.kind else { return };
    let id = d.id;
    // the selected cells, or the whole table when none are (Keynote formats every cell then)
    let block = match crate::tableops::range(app).filter(|r| r.0 == id) {
        Some((_, r0, c0, r1, c1)) => (r0, c0, r1, c1),
        None => match crate::canvas::table_cursor(app).filter(|c| c.0 == id) {
            Some((_, r, c)) => (r, c, r, c),
            None => (0, 0, t.rows.saturating_sub(1), t.cols.saturating_sub(1)),
        },
    };
    let (r, c) = (block.0, block.1);
    let mut cmd: Option<(&'static str, Box<dyn FnOnce(&mut kn_model::Document, &mut kn_model::edit::History) -> Result<(), String>>)> = None;

    // ------------------------------------------------------------------ Data Format
    section(ui, "Data Format");
    let cur = app.doc.as_ref().map(|doc| doc.cell_format(id, r, c)).unwrap_or(CellFormat::Automatic);
    let kinds = ["Automatic", "Number", "Currency", "Percentage", "Fraction", "Numeral System", "Scientific", "Text", "Date & Time", "Duration"];
    let kind = match &cur {
        CellFormat::Automatic => 0,
        CellFormat::Number { .. } => 1,
        CellFormat::Currency { .. } => 2,
        CellFormat::Percent { .. } => 3,
        CellFormat::Fraction { .. } => 4,
        CellFormat::Base { .. } => 5,
        CellFormat::Scientific { .. } => 6,
        CellFormat::Text => 7,
        CellFormat::DateTime { .. } => 8,
        CellFormat::Duration { .. } => 9,
        // a custom format (made in Numbers) is none of these: picking any one converts it
        CellFormat::Custom { .. } => usize::MAX,
    };
    let mut k = kind;
    let shown = kinds.get(kind).copied().unwrap_or("Custom");
    theme::combo(("cell-format", id)).selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
        for (i, n) in kinds.iter().enumerate() {
            if i == 7 || i == 8 {
                ui.separator();
            }
            ui.selectable_value(&mut k, i, *n);
        }
    });
    let mut new: Option<CellFormat> = None;
    if k != kind {
        new = Some(match k {
            1 => CellFormat::Number { decimals: None, thousands: false, negative: 0 },
            2 => CellFormat::Currency { code: "USD".into(), decimals: Some(2), thousands: false, negative: 0, accounting: false },
            3 => CellFormat::Percent { decimals: None, thousands: false, negative: 0 },
            4 => CellFormat::Fraction { accuracy: u32::MAX - 2 },
            5 => CellFormat::Base { base: 2, places: 0, minus: true },
            6 => CellFormat::Scientific { decimals: Some(2) },
            7 => CellFormat::Text,
            8 => CellFormat::DateTime { pattern: "M/d/yy h:mm a".into() },
            9 => CellFormat::Duration { style: 1, largest: 1, smallest: 32, automatic: true },
            _ => CellFormat::Automatic,
        });
    }
    // Decimals (blank = Auto) and the negative number style, as one row
    let decimals_row = |ui: &mut egui::Ui, salt: &str, dec: &mut Option<u32>, neg: Option<&mut u32>| -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Decimals");
            let mut v = dec.map(f64::from).unwrap_or(0.0);
            if dec.is_none() {
                if ui.add(egui::Button::new("Auto").min_size(egui::vec2(60.0, 22.0))).on_hover_text("Click to choose the number of decimal places").clicked() {
                    *dec = Some(2);
                    changed = true;
                }
            } else {
                if theme::stepper(ui, &mut v, 1.0, 0.0..=10.0, "", 0) {
                    *dec = Some(v as u32);
                    changed = true;
                }
                if ui.small_button("Auto").on_hover_text("Decimal places as typed").clicked() {
                    *dec = None;
                    changed = true;
                }
            }
            if let Some(neg) = neg {
                let styles = [("-100", false), ("100", true), ("(100)", false), ("(100)", true)];
                let show = |i: usize| {
                    let (t, red) = styles[i.min(3)];
                    RichText::new(t).color(if red { Color32::from_rgb(0xff, 0x45, 0x3a) } else { theme::TEXT() })
                };
                theme::combo((salt, "neg", id)).selected_text(show(*neg as usize)).width(ui.available_width().min(80.0)).show_ui(ui, |ui| {
                    for i in 0..4 {
                        if ui.selectable_label(*neg as usize == i, show(i)).clicked() {
                            *neg = i as u32;
                            changed = true;
                        }
                    }
                });
            }
        });
        changed
    };
    match cur.clone() {
        CellFormat::Number { mut decimals, mut thousands, mut negative } => {
            let mut ch = decimals_row(ui, "num", &mut decimals, Some(&mut negative));
            ch |= theme::checkbox(ui, &mut thousands, "Thousands Separator").changed();
            if ch {
                new = Some(CellFormat::Number { decimals, thousands, negative });
            }
        }
        CellFormat::Currency { mut code, mut decimals, mut thousands, mut negative, mut accounting } => {
            let mut ch = decimals_row(ui, "cur", &mut decimals, Some(&mut negative));
            ch |= theme::checkbox(ui, &mut thousands, "Thousands Separator").changed();
            ch |= theme::checkbox(ui, &mut accounting, "Accounting Style").changed();
            ui.horizontal(|ui| {
                ui.label("Currency");
                theme::combo(("currency", id)).selected_text(code.clone()).width(ui.available_width()).show_ui(ui, |ui| {
                    for cc in kn_model::tablefmt::CURRENCIES {
                        if ui.selectable_label(code == cc, cc).clicked() {
                            code = cc.to_string();
                            ch = true;
                        }
                    }
                });
            });
            if ch {
                new = Some(CellFormat::Currency { code, decimals, thousands, negative, accounting });
            }
        }
        CellFormat::Percent { mut decimals, mut thousands, mut negative } => {
            let mut ch = decimals_row(ui, "pct", &mut decimals, Some(&mut negative));
            ch |= theme::checkbox(ui, &mut thousands, "Thousands Separator").changed();
            if ch {
                new = Some(CellFormat::Percent { decimals, thousands, negative });
            }
        }
        CellFormat::Scientific { mut decimals } => {
            if decimals_row(ui, "sci", &mut decimals, None) {
                new = Some(CellFormat::Scientific { decimals });
            }
        }
        CellFormat::Fraction { accuracy } => {
            let name = kn_model::tablefmt::FRACTION_ACCURACIES.iter().find(|a| a.0 == accuracy).map(|a| a.1).unwrap_or("Up to 3 digits (445/553)");
            ui.horizontal(|ui| {
                ui.label("Accuracy");
                theme::combo(("frac", id)).selected_text(name).width(ui.available_width()).show_ui(ui, |ui| {
                    for (i, (v, n)) in kn_model::tablefmt::FRACTION_ACCURACIES.iter().enumerate() {
                        if i == 3 {
                            ui.separator();
                        }
                        if ui.selectable_label(*v == accuracy, *n).clicked() {
                            new = Some(CellFormat::Fraction { accuracy: *v });
                        }
                    }
                });
            });
        }
        CellFormat::Base { base, places, minus } => {
            let (mut b, mut pl, mut mi) = (f64::from(base), f64::from(places), minus);
            let mut ch = false;
            ui.horizontal(|ui| {
                ui.label("Base");
                ch |= theme::stepper(ui, &mut b, 1.0, 2.0..=36.0, "", 0);
                ui.label("Places");
                ch |= theme::stepper(ui, &mut pl, 1.0, 0.0..=32.0, "", 0);
            });
            ch |= theme::checkbox(ui, &mut mi, "Negative numbers with a minus sign").changed();
            if ch {
                new = Some(CellFormat::Base { base: b as u32, places: pl as u32, minus: mi });
            }
        }
        CellFormat::DateTime { pattern } => {
            // the pattern is a date part and a time part (either may be None)
            let sample = 789_332_889.0; // Monday, January 5, 2026, 7:08:09 PM
            let (date, time) = {
                let d = kn_model::tablefmt::DATE_PATTERNS.iter().filter(|p| pattern.starts_with(*p)).max_by_key(|p| p.len()).copied();
                let rest = d.map(|d| pattern[d.len()..].trim()).unwrap_or(pattern.as_str());
                let t = kn_model::tablefmt::TIME_PATTERNS.iter().find(|p| **p == rest || rest.replace('\u{202f}', " ") == **p).copied();
                (d, t)
            };
            let mut nd = date;
            let mut nt = time;
            let label = |p: Option<&str>| p.map(|p| kn_model::table::format_date(sample, p)).unwrap_or_else(|| "None".into());
            ui.horizontal(|ui| {
                ui.label("Date");
                theme::combo(("date", id)).selected_text(label(date)).width(ui.available_width()).show_ui(ui, |ui| {
                    ui.selectable_value(&mut nd, None, "None");
                    for p in kn_model::tablefmt::DATE_PATTERNS {
                        ui.selectable_value(&mut nd, Some(p), label(Some(p)));
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Time");
                theme::combo(("time", id)).selected_text(label(time)).width(ui.available_width()).show_ui(ui, |ui| {
                    ui.selectable_value(&mut nt, None, "None");
                    for p in kn_model::tablefmt::TIME_PATTERNS {
                        ui.selectable_value(&mut nt, Some(p), label(Some(p)));
                    }
                });
            });
            if (nd, nt) != (date, time) {
                let pattern = match (nd, nt) {
                    (Some(d), Some(t)) => format!("{d} {t}"),
                    (Some(d), None) => d.to_string(),
                    (None, Some(t)) => t.to_string(),
                    (None, None) => "M/d/yy".to_string(),
                };
                new = Some(CellFormat::DateTime { pattern });
            }
        }
        CellFormat::Duration { style, largest, smallest, automatic } => {
            let styles = ["0:00:00", "0h 0m 0s", "0 hours 0 minutes 0 seconds"];
            let units = [(1, "Weeks"), (2, "Days"), (4, "Hours"), (8, "Minutes"), (16, "Seconds"), (32, "Milliseconds")];
            let (mut st, mut lg, mut sm, mut au) = (style, largest, smallest, automatic);
            ui.horizontal(|ui| {
                ui.label("Format");
                theme::combo(("dur-style", id)).selected_text(styles[(st as usize).min(2)]).width(ui.available_width()).show_ui(ui, |ui| {
                    for (i, n) in styles.iter().enumerate() {
                        ui.selectable_value(&mut st, i as u32, *n);
                    }
                });
            });
            theme::checkbox(ui, &mut au, "Automatic Units");
            if !au {
                ui.horizontal(|ui| {
                    let name = |u: u32| units.iter().find(|x| x.0 == u).map(|x| x.1).unwrap_or("Days");
                    theme::combo(("dur-lg", id)).selected_text(name(lg)).width(110.0).show_ui(ui, |ui| {
                        for (u, n) in units {
                            ui.selectable_value(&mut lg, u, n);
                        }
                    });
                    ui.label("to");
                    theme::combo(("dur-sm", id)).selected_text(name(sm)).width(110.0).show_ui(ui, |ui| {
                        for (u, n) in units {
                            ui.selectable_value(&mut sm, u, n);
                        }
                    });
                });
            }
            if sm < lg {
                sm = lg;
            }
            if (st, lg, sm, au) != (style, largest, smallest, automatic) {
                new = Some(CellFormat::Duration { style: st, largest: lg, smallest: sm, automatic: au });
            }
        }
        CellFormat::Automatic | CellFormat::Text | CellFormat::Custom { .. } => {}
    }
    if let Some(f) = new {
        cmd = Some(("Data Format", Box::new(move |doc, h| doc.set_cells_format(h, id, block, &f).map(|_| ()).map_err(|e| e.to_string()))));
    }

    // ------------------------------------------------------------------ Fill
    section(ui, "Fill");
    let fill = t.cell_styles.get((r * t.cols + c) as usize).map(|s| s.fill.clone()).unwrap_or(kn_model::Fill::None);
    if let Some(spec) = fill_editor(ui, &format!("cell-fill-{id}"), &fill, app.doc.as_mut()) {
        cmd = Some(("Fill", Box::new(move |doc, h| doc.set_cells_fill(h, id, block, Some(&spec)).map(|_| ()).map_err(|e| e.to_string()))));
    }

    // ------------------------------------------------------------------ Border
    section(ui, "Border");
    // the edges chosen in the grid and the line to give them (kept while the panel is open)
    let mem = ui.make_persistent_id(("cell-border", id));
    let (mut edges, mut line): (Option<BorderEdges>, Option<StrokeSpec>) = ui.data(|m| m.get_temp(mem)).unwrap_or((None, Some(StrokeSpec { color: Color::BLACK, width: 1.0, dash: Dash::Solid })));
    let grid = [
        [BorderEdges::All, BorderEdges::Inside, BorderEdges::Outside],
        [BorderEdges::Left, BorderEdges::Center, BorderEdges::Right],
        [BorderEdges::Top, BorderEdges::Middle, BorderEdges::Bottom],
    ];
    let mut picked = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            for row in grid {
                ui.horizontal(|ui| {
                    for e in row {
                        let on = edges == Some(e);
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(30.0, 26.0), egui::Sense::click());
                        let p = ui.painter();
                        p.rect_filled(rect, 4.0, if on { theme::ACCENT() } else if resp.hovered() { theme::gray(0x48) } else { theme::gray(0x38) });
                        // a 2x2 grid with the edges it sets drawn bold
                        let b = egui::Rect::from_center_size(rect.center(), egui::vec2(14.0, 14.0));
                        let thin = egui::Stroke::new(1.0, theme::gray(0x80));
                        let bold = egui::Stroke::new(2.0, if on { Color32::WHITE } else { theme::gray(0xee) });
                        let (l, rr, tp, bt, cx, cy) = (b.min.x, b.max.x, b.min.y, b.max.y, b.center().x, b.center().y);
                        let lines = [
                            ([egui::pos2(l, tp), egui::pos2(rr, tp)], matches!(e, BorderEdges::All | BorderEdges::Outside | BorderEdges::Top)),
                            ([egui::pos2(l, bt), egui::pos2(rr, bt)], matches!(e, BorderEdges::All | BorderEdges::Outside | BorderEdges::Bottom)),
                            ([egui::pos2(l, tp), egui::pos2(l, bt)], matches!(e, BorderEdges::All | BorderEdges::Outside | BorderEdges::Left)),
                            ([egui::pos2(rr, tp), egui::pos2(rr, bt)], matches!(e, BorderEdges::All | BorderEdges::Outside | BorderEdges::Right)),
                            ([egui::pos2(l, cy), egui::pos2(rr, cy)], matches!(e, BorderEdges::All | BorderEdges::Inside | BorderEdges::Middle)),
                            ([egui::pos2(cx, tp), egui::pos2(cx, bt)], matches!(e, BorderEdges::All | BorderEdges::Inside | BorderEdges::Center)),
                        ];
                        for (seg, _) in &lines {
                            p.line_segment(*seg, thin);
                        }
                        for (seg, strong) in &lines {
                            if *strong {
                                p.line_segment(*seg, bold);
                            }
                        }
                        if resp.clicked() {
                            edges = Some(e);
                            picked = true;
                        }
                    }
                });
            }
        });
        ui.vertical(|ui| {
            let names = ["Solid", "Dashed", "Dotted", "No Border"];
            let cur = match &line {
                None => 3,
                Some(s) => match s.dash {
                    Dash::Solid => 0,
                    Dash::Dashed => 1,
                    Dash::Dotted => 2,
                },
            };
            let mut k = cur;
            theme::combo(("cell-line", id)).selected_text(names[cur]).width(ui.available_width()).show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    if i == 3 {
                        ui.separator();
                    }
                    ui.selectable_value(&mut k, i, *n);
                }
            });
            if k != cur {
                let base = line.clone().unwrap_or(StrokeSpec { color: Color::BLACK, width: 1.0, dash: Dash::Solid });
                line = match k {
                    3 => None,
                    i => Some(StrokeSpec { dash: [Dash::Solid, Dash::Dashed, Dash::Dotted][i], ..base }),
                };
                picked |= edges.is_some();
            }
            if let Some(s) = line.as_mut() {
                ui.horizontal(|ui| {
                    if let Some(col) = color_button(ui, s.color) {
                        s.color = col;
                        picked |= edges.is_some();
                    }
                    picked |= theme::stepper32(ui, &mut s.width, 0.5, 0.5..=20.0, " pt", 1) && edges.is_some();
                });
            }
        });
    });
    ui.label(RichText::new("Pick which borders, then their line.").small().color(theme::TEXT_DIM()));
    ui.data_mut(|m| m.insert_temp(mem, (edges, line.clone())));
    if let (true, Some(e)) = (picked, edges) {
        let l = line.clone();
        cmd = Some(("Border", Box::new(move |doc, h| doc.set_cells_border(h, id, block, e, l.as_ref()).map_err(|e| e.to_string()))));
    }

    // ------------------------------------------------------------------ Conditional Highlighting
    ui.add_space(8.0);
    let can = crate::menus::enabled(app, "Conditional Highlighting...");
    if ui.add_enabled(can, egui::Button::new("Conditional Highlighting…").min_size(egui::vec2(ui.available_width(), 26.0))).clicked() {
        let ctx = ui.ctx().clone();
        crate::menus::run(app, &ctx, "Conditional Highlighting...");
    }

    if let Some((name, f)) = cmd {
        let slide = app.current_slide().map(|s| s.slide);
        let mut res = Ok(());
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, name);
            res = f(doc, &mut app.hist);
            doc.end(&mut app.hist);
        }
        if let Err(e) = res {
            app.notify(e);
        }
        if let Some(s) = slide {
            app.bump(s);
        }
    }
}

/// Format inspector with nothing selected (Keynote 15.4): "Slide" heading, layout card,
/// Appearance (Title / Body / Slide Number), Background (Standard | Dynamic, Current Fill, fill
/// pop-up), and Edit Slide Layout at the bottom.
fn slide_format(app: &mut App, ui: &mut egui::Ui, scene: &kn_model::SlideScene) {
    ui.vertical_centered(|ui| ui.label(RichText::new("Slide").color(theme::TEXT_DIM())));
    ui.separator();
    let Some(info) = app.current_slide().cloned() else { return };
    let layout = app.doc.as_ref().and_then(|d| d.layout_name(info.slide)).unwrap_or_default();
    // layout card: the layout's thumbnail and name; click for the layouts
    let layout_id = app.doc.as_ref().and_then(|d| d.file.obj(info.slide)).and_then(|m| m.ref_id("template_slide"));
    let card = egui::Frame::new().fill(crate::theme::gray(0x2c)).stroke(egui::Stroke::new(1.0, crate::theme::gray(0x3c))).corner_radius(10).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(96.0, 54.0), egui::Sense::hover());
            let ctx = ui.ctx().clone();
            let mut budget = crate::textures::Budget::new(10);
            let tex = layout_id.and_then(|lid| {
                let doc = app.doc.as_ref()?;
                app.tex.thumb(&ctx, &mut app.renderer, doc, || doc.layout_scene(lid), lid, 0, &mut budget)
            });
            match tex {
                Some(t) => {
                    ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    ui.painter().rect_filled(r, 2.0, Color32::WHITE);
                    crate::theme::centered(ui.painter(), r.center(), &layout, egui::FontId::proportional(8.0), crate::theme::gray(60));
                }
            }
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new("Slide Layout").color(theme::TEXT_DIM()));
                ui.label(theme::bold(&layout));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // a drawn chevron: no font needed (macOS has no glyph for U+2304)
                let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                crate::icons::draw(ui.painter(), r, crate::icons::Icon::ChevronDown, theme::TEXT_DIM());
            });
        });
    });
    let card_resp = ui.interact(card.response.rect, ui.scope_id().with("layout-card"), egui::Sense::click());
    egui::Popup::menu(&card_resp).width(560.0).show(|ui| {
        let Some(doc) = app.doc.as_ref() else { return };
        let layouts = doc.layouts();
        let aspect = doc.slide_size().h / doc.slide_size().w;
        let mut budget = crate::textures::Budget::new(30);
        let mut pick = None;
        egui::Grid::new("change-layout").spacing([10.0, 10.0]).show(ui, |ui| {
            for (i, (id, name)) in layouts.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(120.0, 120.0 * aspect + 18.0), egui::Sense::click());
                let img = egui::Rect::from_min_size(rect.min, egui::vec2(120.0, 120.0 * aspect));
                let ctx = ui.ctx().clone();
                let doc = app.doc.as_ref().unwrap();
                match app.tex.thumb(&ctx, &mut app.renderer, doc, || doc.layout_scene(*id), *id, 0, &mut budget) {
                    Some(t) => {
                        ui.painter().image(t.id(), img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        ui.painter().rect_filled(img, 2.0, crate::theme::gray(80));
                        ctx.request_repaint();
                    }
                }
                let on = Some(*id) == layout_id;
                let stroke = if resp.hovered() || on { egui::Stroke::new(2.0, theme::ACCENT()) } else { egui::Stroke::new(1.0, crate::theme::gray(90)) };
                ui.painter().rect_stroke(img, 2.0, stroke, egui::StrokeKind::Outside);
                crate::theme::centered(ui.painter(), img.center_bottom() + egui::vec2(0.0, 10.0), name, egui::FontId::proportional(11.0), theme::TEXT_DIM());
                if resp.clicked() {
                    pick = Some(*id);
                }
                if i % 4 == 3 {
                    ui.end_row();
                }
            }
        });
        if let Some(l) = pick {
            ui.close();
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Change Layout");
                doc.change_slide_layout(&mut app.hist, info.slide, l);
                doc.end(&mut app.hist);
            }
            app.bump_all();
        }
    });
    section(ui, "Appearance");
    let shown = |k| scene.drawables.iter().any(|d| d.placeholder == Some(k) && !d.from_template);
    let has_field = |f: &str| app.doc.as_ref().and_then(|d| d.file.obj(info.slide)).and_then(|m| m.ref_id(f)).is_some();
    let mut change: Option<(&str, bool)> = None;
    for (label, kind, field) in [("Title", kn_model::PlaceholderKind::Title, "titlePlaceholder"), ("Body", kn_model::PlaceholderKind::Body, "bodyPlaceholder")] {
        let mut on = shown(kind);
        if ui.add_enabled_ui(has_field(field), |ui| theme::checkbox(ui, &mut on, label)).inner.changed() {
            change = Some((field, on));
        }
    }
    let mut num = shown(kn_model::PlaceholderKind::SlideNumber);
    if ui.add_enabled_ui(has_field("slideNumberPlaceholder"), |ui| theme::checkbox(ui, &mut num, "Slide Number")).inner.changed() {
        change = Some(("slideNumberPlaceholder", num));
    }
    if let Some((field, on)) = change {
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, if on { "Show Placeholder" } else { "Hide Placeholder" });
            doc.set_placeholder_visible(&mut app.hist, info.slide, field, on);
            doc.end(&mut app.hist);
        }
        app.bump(info.slide);
    }
    section(ui, "Background");
    let mut seg = if scene.motion.is_some() { 1 } else { app.insp.bg_tab.unwrap_or(0) };
    let before = seg;
    seg_small(ui, &["Standard", "Dynamic"], &mut seg);
    if seg != before {
        app.insp.bg_tab = Some(seg);
        if seg == 0 && scene.motion.is_some() {
            // back to a plain fill
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Background");
                doc.set_motion_background(&mut app.hist, info.slide, None, None);
                doc.end(&mut app.hist);
            }
            app.bump(info.slide);
            return;
        }
    }
    if seg == 1 {
        dynamic_background(app, ui, scene, info.slide);
        bottom_layout_button(app, ui);
        return;
    }
    ui.horizontal(|ui| {
        ui.label("Current Fill");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(100.0, 26.0), egui::Sense::hover());
            let c = match &scene.background {
                Fill::Color(c) => c32(*c),
                Fill::Gradient { stops, .. } => stops.first().map(|s| c32(s.1)).unwrap_or(Color32::WHITE),
                _ => crate::theme::gray(128),
            };
            ui.painter().rect_filled(r, 6.0, c);
        });
    });
    if let Some(f) = fill_editor(ui, "background", &scene.background, app.doc.as_mut()) {
        if let Some(doc) = app.doc.as_mut() {
            doc.begin_merge(&mut app.hist, "Background");
            doc.set_slide_background(&mut app.hist, info.slide, &f);
            doc.end(&mut app.hist);
        }
        app.bump(info.slide);
    }
    bottom_layout_button(app, ui);
}

fn bottom_layout_button(app: &mut App, ui: &mut egui::Ui) {
    let room = ui.available_height() - 40.0;
    if room > 0.0 {
        ui.add_space(room);
    }
    ui.separator();
    ui.vertical_centered(|ui| {
        if ui.add(egui::Button::new("Edit Slide Layout").min_size(egui::vec2(220.0, 28.0))).clicked() {
            app.command(ui.ctx(), "Edit Slide Layouts");
        }
    });
}

/// A small still of a dynamic background (preset picker), cached by style and look.
fn motion_thumb(app: &mut App, ctx: &egui::Context, mb: &kn_model::motionbg::MotionBg) -> egui::TextureHandle {
    use std::hash::{Hash, Hasher};
    let mut hsh = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", (&mb.nums, &mb.colors, &mb.family)).hash(&mut hsh);
    let key = (mb.style, hsh.finish());
    if let Some(t) = app.insp.dyn_thumbs.get(&key) {
        return t.clone();
    }
    let (w, h) = (96, 54);
    let px = kn_render::motion::render(mb, 0.0, w, h);
    let t = ctx.load_texture("dyn-thumb", egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px), egui::TextureOptions::LINEAR);
    app.insp.dyn_thumbs.insert(key, t.clone());
    t
}

/// Keynote's controls for each renderer: (label, property, min, max, decimals, suffix).
fn motion_controls(family: &str) -> &'static [(&'static str, &'static str, f64, f64, usize, &'static str)] {
    match family {
        "KNGradientTerrainBase" => &[("Speed", "noise_speed", 0.0, 100.0, 2, ""), ("Height", "noise_amplitude", 0.0, 10.0, 2, ""), ("Peaks", "noise_scale_horizontal", 0.01, 10.0, 2, "")],
        "KNGradientTerrainFlyover" => &[("Movement", "noise_movement", -100.0, 100.0, 0, ""), ("Height", "noise_amplitude", 0.0, 10.0, 2, ""), ("Peaks", "noise_scale_horizontal", 0.01, 10.0, 2, "")],
        "KNGradientDiffusionBase" => &[("Speed", "noise_speed", 0.0, 5.0, 2, ""), ("Complexity", "noise_octaves_iterations", 1.0, 8.0, 0, ""), ("Frequency", "scale", 0.1, 5.0, 2, "")],
        "KNWavesBase" | "KNWavesSymmetrical" => &[("Speed", "speed", 0.0, 100.0, 0, ""), ("Amplitude", "amplitude", 0.0, 10.0, 2, ""), ("Wavelength", "scale", 0.1, 10.0, 2, ""), ("Focus", "blur", 0.0, 10.0, 2, "")],
        "KNGraphicSpheresBase" => &[("Speed", "speed", 0.0, 100.0, 0, ""), ("Complexity", "noise_scale", 0.0, 10.0, 2, ""), ("Pattern", "offset", 0.0, 10.0, 2, "")],
        _ => &[("Angle", "angle", 0.0, 360.0, 1, "\u{b0}"), ("Scale", "scale", 0.1, 5.0, 2, ""), ("Speed", "speed", 0.0, 100.0, 2, "")],
    }
}

/// Format > Slide > Background > Dynamic: preset pages (6 per page), Preview Motion, Colors and
/// the renderer's options. Changes write a variation of the preset plus a new poster still.
fn dynamic_background(app: &mut App, ui: &mut egui::Ui, scene: &kn_model::SlideScene, slide: kn_format::ObjId) {
    let ctx = ui.ctx().clone();
    let presets: Vec<(kn_format::ObjId, kn_model::motionbg::MotionBg)> = match app.doc.as_ref() {
        Some(d) => d.motion_presets().into_iter().filter_map(|(id, _)| kn_model::motionbg::read(&d.file, id).map(|m| (id, m))).collect(),
        None => return,
    };
    if presets.is_empty() {
        ui.label(RichText::new("This theme has no dynamic backgrounds.").color(theme::TEXT_DIM()));
        return;
    }
    let cur = scene.motion.clone();
    // the preset the slide uses (a variation's parent)
    let cur_preset = cur.as_ref().map(|m| {
        let parent = app.doc.as_ref().and_then(|d| d.file.obj(m.style)).and_then(|s| s.find_super("TSS.StyleArchive")).and_then(|s| s.ref_id("parent"));
        if presets.iter().any(|p| p.0 == m.style) { m.style } else { parent.unwrap_or(m.style) }
    });
    let pages = presets.len().div_ceil(6);
    if app.insp.dyn_page_slide != Some(slide) {
        app.insp.dyn_page_slide = Some(slide);
        app.insp.dyn_page = cur_preset.and_then(|c| presets.iter().position(|p| p.0 == c)).map(|i| i / 6).unwrap_or(0);
    }
    let mut page = app.insp.dyn_page.min(pages - 1);
    let mut pick: Option<kn_format::ObjId> = None;
    ui.horizontal(|ui| {
        if ui.add_enabled(page > 0, egui::Button::new(RichText::new("\u{2039}").size(20.0)).frame(false)).clicked() {
            page -= 1;
        }
        egui::Grid::new("dyn-presets").num_columns(3).spacing([8.0, 8.0]).show(ui, |ui| {
            for (n, (id, mb)) in presets.iter().enumerate().skip(page * 6).take(6) {
                let tex = motion_thumb(app, &ctx, mb);
                let (r, resp) = ui.allocate_exact_size(egui::vec2(64.0, 36.0), egui::Sense::click());
                ui.painter().image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                let on = cur_preset == Some(*id);
                ui.painter().rect_stroke(r.expand(if on { 2.0 } else { 0.0 }), 4.0, egui::Stroke::new(if on { 2.0 } else { 1.0 }, if on { theme::ACCENT() } else { theme::gray(0x44) }), egui::StrokeKind::Outside);
                if resp.on_hover_text(&mb.name).clicked() {
                    pick = Some(*id);
                }
                if n % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        if ui.add_enabled(page + 1 < pages, egui::Button::new(RichText::new("\u{203a}").size(20.0)).frame(false)).clicked() {
            page += 1;
        }
    });
    // page dots
    ui.vertical_centered(|ui| {
        ui.horizontal(|ui| {
            let w = pages as f32 * 10.0;
            ui.add_space((ui.available_width() - w) / 2.0);
            for i in 0..pages {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::click());
                ui.painter().circle_filled(r.center(), 2.5, if i == page { theme::TEXT() } else { theme::TEXT_DIM() });
                if resp.clicked() {
                    page = i;
                }
            }
        });
    });
    app.insp.dyn_page = page;
    let size = app.doc.as_ref().map(|d| d.slide_size()).unwrap_or(kn_model::Size { w: 1920.0, h: 1080.0 });
    let poster_of = |mb: &kn_model::motionbg::MotionBg| kn_render::motion::poster_jpeg(mb, size.w as u32, size.h as u32);
    if let Some(id) = pick {
        let mb = presets.iter().find(|p| p.0 == id).map(|p| p.1.clone());
        if let (Some(mb), Some(doc)) = (mb, app.doc.as_mut()) {
            let poster = if mb.poster.is_none() { poster_of(&mb) } else { None };
            doc.begin(&mut app.hist, "Dynamic Background");
            doc.set_motion_background(&mut app.hist, slide, Some(id), poster);
            doc.end(&mut app.hist);
            app.bump(slide);
        }
        return;
    }
    let Some(mb) = cur else {
        ui.label(RichText::new("Choose a dynamic background.").color(theme::TEXT_DIM()));
        return;
    };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(theme::bold("Preview Motion"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            theme::toggle(ui, &mut app.preview_motion);
        });
    });
    let mut change: Option<kn_model::motionbg::MotionProp> = None;
    ui.label(theme::bold("Colors"));
    {
        // the gradient bar with a colour well per stop
        let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 14.0), egui::Sense::hover());
        let mut mesh = egui::Mesh::default();
        let stops = &mb.colors;
        for (i, w) in stops.windows(2).enumerate() {
            let x0 = bar.min.x + bar.width() * w[0].0;
            let x1 = bar.min.x + bar.width() * w[1].0;
            let base = mesh.vertices.len() as u32;
            for (x, c) in [(x0, w[0].1), (x1, w[1].1)] {
                mesh.colored_vertex(egui::pos2(x, bar.min.y), c32(c));
                mesh.colored_vertex(egui::pos2(x, bar.max.y), c32(c));
            }
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base + 1, base + 2, base + 3);
            let _ = i;
        }
        ui.painter().add(mesh);
        let mut colors = mb.colors.clone();
        let mut edited = false;
        ui.horizontal(|ui| {
            for (_, c) in colors.iter_mut() {
                let mut cc = c32(*c);
                if ui.color_edit_button_srgba(&mut cc).changed() {
                    *c = from_c32(cc);
                    edited = true;
                }
            }
        });
        if edited {
            change = Some(kn_model::motionbg::MotionProp::Colors(colors));
        }
    }
    if mb.family.starts_with("KNWaves") {
        ui.horizontal(|ui| {
            ui.label(theme::bold("Background Color"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut cc = c32(mb.background_color.unwrap_or(kn_model::Color::BLACK));
                if ui.color_edit_button_srgba(&mut cc).changed() {
                    change = Some(kn_model::motionbg::MotionProp::Background(from_c32(cc)));
                }
            });
        });
    }
    for &(label, prop, lo, hi, dec, suffix) in motion_controls(&mb.family) {
        ui.label(theme::bold(label));
        ui.horizontal(|ui| {
            let mut v = mb.num(prop, (lo + hi) / 2.0);
            let slider = ui.add_sized([ui.available_width() - 90.0, 20.0], egui::Slider::new(&mut v, lo..=hi).show_value(false));
            let stepped = theme::stepper(ui, &mut v, if dec == 0 { 1.0 } else { 0.1 }, lo..=hi, suffix, dec);
            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) || stepped {
                change = Some(kn_model::motionbg::MotionProp::Num(prop, v));
            }
        });
    }
    if let Some(c) = change {
        // the edited look, for the new poster still
        let mut look = mb.clone();
        match &c {
            kn_model::motionbg::MotionProp::Num(n, v) => {
                look.nums.insert(n, *v);
                if *n == "noise_scale_horizontal" {
                    look.nums.insert("noise_scale_vertical", *v);
                }
            }
            kn_model::motionbg::MotionProp::Colors(cs) => look.colors = cs.clone(),
            kn_model::motionbg::MotionProp::Background(c) => look.background_color = Some(*c),
        }
        let poster = poster_of(&look);
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Dynamic Background");
            doc.set_motion_property(&mut app.hist, slide, c.clone(), poster.clone());
            // Peaks sets both directions (Keynote keeps them equal)
            if let kn_model::motionbg::MotionProp::Num("noise_scale_horizontal", v) = c {
                doc.set_motion_property(&mut app.hist, slide, kn_model::motionbg::MotionProp::Num("noise_scale_vertical", v), poster);
            }
            doc.end(&mut app.hist);
        }
        app.bump(slide);
    }
}

/// Format > Table: Headers & Footer, Table Name, Alternating Row Color, row and column sizes.
/// A table style's thumbnail: a little 4×4 grid with its header, body and line colours.
fn table_preset_thumb(p: &egui::Painter, r: egui::Rect, look: &kn_model::tablelook::PresetLook) {
    let col = |c: Option<kn_model::Color>| c.map(c32);
    let white = Color32::WHITE;
    p.rect_filled(r, 0.0, col(look.body).unwrap_or(white));
    let (rows, cols) = (7, 4);
    let (ch, cw) = (r.height() / rows as f32, r.width() / cols as f32);
    for y in 0..rows {
        for x in 0..cols {
            let cr = egui::Rect::from_min_size(egui::pos2(r.min.x + x as f32 * cw, r.min.y + y as f32 * ch), egui::vec2(cw, ch));
            let fill = if y == 0 {
                col(look.header_row)
            } else if x == 0 {
                col(look.header_col)
            } else if y == rows - 1 && look.footer.is_some() {
                col(look.footer)
            } else if y % 2 == 0 {
                col(look.band)
            } else {
                None
            };
            if let Some(f) = fill {
                p.rect_filled(cr, 0.0, f);
            }
        }
    }
    let line = egui::Stroke::new(0.7, col(look.line).unwrap_or(Color32::from_gray(190)));
    let seg = |a: egui::Pos2, b: egui::Pos2| {
        if look.dashed {
            p.extend(egui::Shape::dashed_line(&[a, b], line, 1.5, 1.5));
        } else {
            p.line_segment([a, b], line);
        }
    };
    for y in 1..rows {
        let yy = r.min.y + y as f32 * ch;
        seg(egui::pos2(r.min.x, yy), egui::pos2(r.max.x, yy));
    }
    for x in 1..cols {
        let xx = r.min.x + x as f32 * cw;
        seg(egui::pos2(xx, r.min.y), egui::pos2(xx, r.max.y));
    }
    p.rect_stroke(r, 0.0, egui::Stroke::new(0.7, Color32::from_gray(150)), egui::StrokeKind::Inside);
}

fn table_panel(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
    use kn_model::tablerows::HeaderKind;
    let DrawableKind::Table(t) = &d.kind else { return };
    let id = d.id;
    let mut cmd: Option<Box<dyn FnOnce(&mut kn_model::Document, &mut kn_model::edit::History)>> = None;
    // Table Styles: the theme's presets, six to a page, and a "+" after them that saves this
    // table's look as a new style (R37, R41)
    let presets = app.doc.as_ref().map(|doc| doc.table_style_presets()).unwrap_or_default();
    if !presets.is_empty() {
        let tiles = presets.len() + 1;
        let pages = tiles.div_ceil(6);
        let key = ui.scope_id().with("table-style-page");
        let mut page = ui.data(|m| m.get_temp::<usize>(key)).unwrap_or(0).min(pages - 1);
        let arrow_w = 16.0;
        let w = ui.available_width() - 2.0 * arrow_w;
        let cell = egui::vec2((w / 3.0).floor(), 82.0);
        let mut save = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let arrow = |ui: &mut egui::Ui, text: &str, on: bool| ui.add_sized([arrow_w, cell.y * 2.0], egui::Button::new(RichText::new(text).size(20.0).color(if on { theme::TEXT() } else { theme::TEXT_DIM().gamma_multiply(0.4) })).frame(false)).clicked() && on;
            if arrow(ui, "‹", page > 0) {
                page -= 1;
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in 0..2 {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for col in 0..3 {
                            let i = page * 6 + row * 3 + col;
                            let (r, resp) = ui.allocate_exact_size(cell, egui::Sense::click());
                            let thumb = egui::Rect::from_center_size(r.center(), egui::vec2((r.width() - 16.0).min(88.0), 70.0));
                            if let Some(&(pid, _, net)) = presets.get(i) {
                                let look = net.and_then(|n| app.doc.as_ref().map(|doc| doc.table_preset_look(n))).unwrap_or_default();
                                table_preset_thumb(ui.painter(), thumb, &look);
                                // Keynote never marks the table's current style (R38), only the one under the pointer
                                if resp.hovered() {
                                    ui.painter().rect_stroke(r.shrink(3.0), 5.0, egui::Stroke::new(1.5, theme::ACCENT()), egui::StrokeKind::Inside);
                                }
                                if resp.clicked() {
                                    cmd = Some(Box::new(move |doc, h| {
                                        doc.apply_table_style_preset(h, id, pid);
                                    }));
                                }
                            } else if i == presets.len() {
                                // "+": save the table's look as a style of the theme
                                let p = ui.painter();
                                let line = egui::Stroke::new(1.2, if resp.hovered() { theme::ACCENT() } else { theme::TEXT_DIM() });
                                p.extend(egui::Shape::dashed_line(&[thumb.left_top(), thumb.right_top(), thumb.right_bottom(), thumb.left_bottom(), thumb.left_top()], line, 4.0, 3.0));
                                crate::theme::centered(p, thumb.center(), "+", egui::FontId::proportional(26.0), line.color);
                                if resp.on_hover_text("Save this table's look as a new style").clicked() {
                                    save = true;
                                }
                            }
                        }
                    });
                }
            });
            if arrow(ui, "›", page + 1 < pages) {
                page += 1;
            }
        });
        ui.vertical_centered(|ui| {
            ui.label(theme::bold("Table Styles"));
            ui.horizontal(|ui| {
                // page dots (click one to go there)
                ui.add_space((ui.available_width() - pages as f32 * 12.0) / 2.0);
                for k in 0..pages {
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(12.0, 10.0), egui::Sense::click());
                    let c = if k == page { theme::TEXT() } else { theme::TEXT_DIM().gamma_multiply(0.5) };
                    ui.painter().circle_filled(r.center(), 3.0, c);
                    if resp.clicked() {
                        page = k;
                    }
                }
            });
        });
        ui.data_mut(|m| m.insert_temp(key, page));
        if save {
            cmd = Some(Box::new(move |doc, h| {
                let _ = doc.save_table_style_preset(h, id);
            }));
        }
    }
    section(ui, "Table Options");
    // Title is the table's name; Caption the object caption below it
    let mut name = app.doc.as_ref().is_some_and(|doc| doc.table_name_shown(id));
    if theme::checkbox(ui, &mut name, "Title").changed() {
        cmd = Some(Box::new(move |doc, h| doc.set_table_name_shown(h, id, name)));
    }
    let mut caption = app.doc.as_ref().and_then(|doc| doc.file.obj(id)).and_then(|m| m.find_super("TSD.DrawableArchive")).is_some_and(|dm| {
        dm.bool("caption_hidden") == Some(false) && dm.ref_id("caption").is_some_and(|c| app.doc.as_ref().is_some_and(|doc| doc.file.type_name(c) == "TSA.CaptionInfoArchive"))
    });
    if theme::checkbox(ui, &mut caption, "Caption").changed() {
        if let Some(slide) = app.current_slide().map(|s| s.slide) {
            cmd = Some(Box::new(move |doc, h| {
                doc.set_caption(h, slide, id, false, caption);
            }));
        }
    }
    section(ui, "Headers & Footer");
    ui.columns(3, |c| {
        for (i, (kind, label, cur)) in [(HeaderKind::Rows, "Header Rows", t.header_rows), (HeaderKind::Columns, "Header Columns", t.header_cols), (HeaderKind::Footer, "Footer", t.footer_rows)].into_iter().enumerate() {
            c[i].vertical_centered(|ui| {
                let mut v = cur as f64;
                if theme::stepper(ui, &mut v, 1.0, 0.0..=5.0, "", 0) {
                    let n = v as u32;
                    cmd = Some(Box::new(move |doc, h| {
                        let _ = doc.set_table_header_count(h, id, kind, n);
                    }));
                }
                ui.label(RichText::new(label).size(11.0).color(theme::TEXT_DIM()));
            });
        }
    });
    // Rows / Columns: added after the last one (like it) or taken off the end
    ui.add_space(6.0);
    for (axis, label, cur) in [(kn_model::tablerows::Axis::Row, "Rows", t.rows), (kn_model::tablerows::Axis::Column, "Columns", t.cols)] {
        ui.horizontal(|ui| {
            ui.label(theme::bold(label));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut v = cur as f64;
                if theme::stepper(ui, &mut v, 1.0, 1.0..=999.0, "", 0) {
                    let n = v as u32;
                    cmd = Some(Box::new(move |doc, h| {
                        for i in cur..n {
                            let _ = doc.table_insert(h, id, axis, i, i - 1);
                        }
                        for i in (n..cur).rev() {
                            let _ = doc.table_delete(h, id, axis, i);
                        }
                    }));
                }
            });
        });
    }
    // Table Font Size: every cell's text a point bigger / smaller
    ui.horizontal(|ui| {
        let size = app.doc.as_ref().map(|doc| doc.table_font_size(id)).unwrap_or(12.0);
        ui.label(format!("Table Font Size  {size:.0} pt"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(RichText::new("A").size(16.0)).min_size(egui::vec2(32.0, 24.0))).on_hover_text("Bigger").clicked() {
                cmd = Some(Box::new(move |doc, h| {
                    doc.bump_table_font_size(h, id, 1.0);
                }));
            }
            if ui.add(egui::Button::new(RichText::new("A").size(11.0)).min_size(egui::vec2(32.0, 24.0))).on_hover_text("Smaller").clicked() {
                cmd = Some(Box::new(move |doc, h| {
                    doc.bump_table_font_size(h, id, -1.0);
                }));
            }
        });
    });

    // Table Outline: a line round the whole table
    ui.label(theme::bold("Table Outline"));
    let outline = app.doc.as_ref().and_then(|doc| doc.table_outline(id));
    if let Some(s) = stroke_editor(ui, &format!("table-outline-{id}"), &outline, true) {
        cmd = Some(Box::new(move |doc, h| {
            doc.set_table_outline(h, id, s.as_ref());
        }));
    }
    // Gridlines: the five switches, drawn as little tables with that line bold
    section(ui, "Gridlines");
    {
        use kn_model::tablelook::Gridline;
        ui.horizontal(|ui| {
            for (g, tip) in [
                (Gridline::BodyRows, "Horizontal lines in the body"),
                (Gridline::BodyColumns, "Vertical lines in the body"),
                (Gridline::HeaderRow, "Header row line"),
                (Gridline::HeaderColumn, "Header column line"),
                (Gridline::Footer, "Footer line"),
            ] {
                let on = app.doc.as_ref().is_some_and(|doc| doc.table_gridline(id, g));
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(40.0, 30.0), egui::Sense::click());
                let p = ui.painter();
                p.rect_filled(rect, 5.0, if on { theme::ACCENT() } else if resp.hovered() { theme::gray(0x48) } else { theme::gray(0x38) });
                let b = egui::Rect::from_center_size(rect.center(), egui::vec2(20.0, 16.0));
                let thin = egui::Stroke::new(1.0, theme::gray(0x90));
                let bold = egui::Stroke::new(2.0, if on { Color32::WHITE } else { theme::gray(0xee) });
                p.rect_stroke(b, 1.0, thin, egui::StrokeKind::Inside);
                let (x1, x2, y1, y2) = (b.min.x + b.width() / 3.0, b.min.x + 2.0 * b.width() / 3.0, b.min.y + b.height() / 3.0, b.min.y + 2.0 * b.height() / 3.0);
                let line = |p: &egui::Painter, a: egui::Pos2, z: egui::Pos2, strong: bool| {
                    p.line_segment([a, z], if strong { bold } else { thin });
                };
                match g {
                    Gridline::BodyRows => {
                        line(p, egui::pos2(b.min.x, y1), egui::pos2(b.max.x, y1), true);
                        line(p, egui::pos2(b.min.x, y2), egui::pos2(b.max.x, y2), true);
                    }
                    Gridline::BodyColumns => {
                        line(p, egui::pos2(x1, b.min.y), egui::pos2(x1, b.max.y), true);
                        line(p, egui::pos2(x2, b.min.y), egui::pos2(x2, b.max.y), true);
                    }
                    Gridline::HeaderRow => line(p, egui::pos2(b.min.x, y1), egui::pos2(b.max.x, y1), true),
                    Gridline::HeaderColumn => line(p, egui::pos2(x1, b.min.y), egui::pos2(x1, b.max.y), true),
                    Gridline::Footer => line(p, egui::pos2(b.min.x, y2), egui::pos2(b.max.x, y2), true),
                }
                if resp.on_hover_text(tip).clicked() {
                    cmd = Some(Box::new(move |doc, h| {
                        doc.set_table_gridline(h, id, g, !on);
                    }));
                }
            }
        });
    }

    // Alternating Row Color, with its colour
    let mut banded = app.doc.as_ref().is_some_and(|doc| doc.table_banded(id));
    ui.horizontal(|ui| {
        if theme::checkbox(ui, &mut banded, "Alternating Row Color").changed() {
            cmd = Some(Box::new(move |doc, h| {
                doc.set_table_banded(h, id, banded);
            }));
        }
        if banded {
            let cur = match app.doc.as_ref().and_then(|doc| doc.table_band_fill(id)) {
                Some(Fill::Color(c)) => c,
                _ => Color { r: 0.9, g: 0.9, b: 0.92, a: 1.0 },
            };
            if let Some(c) = color_button(ui, cur) {
                cmd = Some(Box::new(move |doc, h| {
                    doc.set_table_band_color(h, id, c);
                }));
            }
        }
    });

    section(ui, "Row & Column Size");
    // the selected rows / columns' height and width (all of them when nothing is selected)
    let (r0, c0, r1, c1) = match crate::tableops::range(app).filter(|r| r.0 == id) {
        Some((_, a, b, c, d)) => (a, b, c, d),
        None => (0, 0, t.rows.saturating_sub(1), t.cols.saturating_sub(1)),
    };
    for (rows, label, from, to, sizes) in [(true, "Height", r0, r1, &t.row_heights), (false, "Width", c0, c1, &t.col_widths)] {
        ui.horizontal(|ui| {
            ui.label(label);
            let mut v = sizes.get(from as usize).copied().unwrap_or(30.0) as f64;
            if theme::stepper(ui, &mut v, 1.0, 8.0..=2000.0, " pt", 0) {
                let set: Vec<(u32, f32)> = (from..=to).map(|i| (i, v as f32)).collect();
                cmd = Some(Box::new(move |doc, h| {
                    doc.set_table_sizes(h, id, rows, &set);
                }));
            }
            if ui.button("Fit").on_hover_text(if rows { "Fit the height to the text" } else { "Fit the width to the text" }).clicked() {
                let idx: Vec<u32> = (from..=to).collect();
                cmd = Some(Box::new(move |doc, h| {
                    doc.fit_table_to_content(h, id, rows, &idx);
                }));
            }
        });
    }
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 8.0) / 2.0;
        if ui.add(egui::Button::new("Distribute Rows").min_size(egui::vec2(w, 26.0))).clicked() {
            cmd = Some(Box::new(move |doc, h| {
                doc.distribute_table(h, id, true);
            }));
        }
        if ui.add(egui::Button::new("Distribute Columns").min_size(egui::vec2(w, 26.0))).clicked() {
            cmd = Some(Box::new(move |doc, h| {
                doc.distribute_table(h, id, false);
            }));
        }
    });
    if let Some(f) = cmd {
        let slide = app.current_slide().map(|s| s.slide);
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Table");
            f(doc, &mut app.hist);
            doc.end(&mut app.hist);
        }
        if let Some(s) = slide {
            app.bump(s);
        }
    }
}

/// Format > Movie / Audio: Volume, Repeat, Trim, Poster Frame, Start automatically, Play across
/// slides.
fn movie_panel(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
    use kn_model::imageedit::MovieProp;
    let DrawableKind::Movie(m) = &d.kind else { return };
    let id = d.id;
    let mut change: Option<MovieProp> = None;
    // the movie's length (ffprobe, once per movie)
    // (the file may still be copying to the cache: ask again every 2 s until known)
    let len = match app.insp.movie_len {
        Some((mid, l, at)) if mid == id && (l > 0.0 || at.elapsed().as_secs_f32() < 2.0) => l,
        _ => {
            let l = m.movie_data.and_then(|data| app.doc.as_ref().and_then(|doc| crate::media::cache_file(doc, data))).and_then(|(p, _)| crate::media::duration(&p)).unwrap_or(0.0) as f32;
            app.insp.movie_len = Some((id, l, std::time::Instant::now()));
            l
        }
    };
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
            let r = ui.add(egui::Slider::new(&mut t, 0.0..=len).suffix(" s"));
            if r.drag_stopped() {
                let frame = m.movie_data.and_then(|data| app.doc.as_ref().and_then(|doc| crate::media::cache_file(doc, data))).and_then(|(p, _)| crate::media::frame_jpeg(&p, t as f64));
                match frame {
                    Some(jpeg) => {
                        let slide = app.current_slide().map(|s| s.slide);
                        if let Some(doc) = app.doc.as_mut() {
                            doc.begin(&mut app.hist, "Poster Frame");
                            let _ = doc.set_movie_poster(&mut app.hist, id, t, jpeg);
                            doc.end(&mut app.hist);
                        }
                        if let Some(s) = slide {
                            app.bump(s);
                        }
                    }
                    None => app.notify("Couldn't read that frame (is ffmpeg installed?)"),
                }
            }
        }
    }
    section(ui, "Controls");
    let mut auto = m.autoplay;
    if theme::checkbox(ui, &mut auto, "Start automatically").changed() {
        change = Some(MovieProp::AutoPlay(auto));
    }
    let mut across = m.plays_across_slides;
    if theme::checkbox(ui, &mut across, "Play across slides").changed() {
        change = Some(MovieProp::AcrossSlides(across));
    }
    if let Some(c) = change {
        let slide = app.current_slide().map(|s| s.slide);
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Movie");
            doc.set_movie_prop(&mut app.hist, id, &c);
            doc.end(&mut app.hist);
        }
        if let Some(s) = slide {
            app.bump(s);
        }
    }
}

/// Format > Image, laid out like Keynote's: Replace, Edit Mask / Reset Mask, Remove Background,
/// Exposure, Saturation, Enhance, Adjust Image, Description.
fn image_panel(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
    let DrawableKind::Image(i) = &d.kind else { return };
    let id = d.id;
    // Keynote 15.4's Image tab: File Info; Edit Mask | Auto Crop; Remove Background;
    // Adjustments (Exposure, Saturation; Enhance | adjust | Reset); Description
    section(ui, "File Info");
    let name = i.data_id.and_then(|did| app.doc.as_ref().and_then(|doc| doc.data_name(did)).map(crate::inspector::shown_file_name)).unwrap_or_else(|| "Image".into());
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 18.0), egui::Sense::hover());
        crate::icons::draw(ui.painter(), r, crate::icons::Icon::File, theme::TEXT_DIM());
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
    let mut a = app.doc.as_ref().map(|doc| doc.image_adjustments(id)).unwrap_or_default();
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
    crate::icons::draw(ui.painter(), egui::Rect::from_center_size(adj.center(), egui::vec2(16.0, 16.0)), crate::icons::Icon::Sliders, theme::TEXT());
    if ar.on_hover_text("Adjust Image").clicked() {
        app.show_adjust_image = true;
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
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Description");
            doc.set_description(&mut app.hist, id, &text);
            doc.end(&mut app.hist);
        }
        if let Some(s) = slide {
            app.bump(s);
        }
    }
}

fn c32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied((c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8, (c.a * 255.0).round() as u8)
}

fn from_c32(c: Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: a as f32 / 255.0 }
}

/// Colour well; returns the new colour when the user changed it.
fn color_button(ui: &mut egui::Ui, c: Color) -> Option<Color> {
    // Apple's colour well (kn-ui, shared with Numdeck): presets popover, wheel button for the
    // Colors window; its id from the place in the panel
    let mut v = c32(c);
    let id = ui.next_auto_id();
    kn_ui::colors::color_well(ui, id, &mut v, 110.0).then(|| from_c32(v))
}

/// Fill editor as Keynote's (No Fill / Color / Gradient / Image / Advanced Gradient Fill);
/// returns a new fill when changed. `doc` takes the picture for an Image Fill.
fn fill_editor(ui: &mut egui::Ui, salt: &str, fill: &Fill, doc: Option<&mut kn_model::Document>) -> Option<FillSpec> {
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
fn stroke_editor(ui: &mut egui::Ui, salt: &str, stroke: &Option<kn_model::Stroke>, allow_none: bool) -> Option<Option<StrokeSpec>> {
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
        _ => Swatch::Color(crate::theme::gray(128)),
    };
    // theme presets grid ("Shape Styles" / "Text Styles"): 6 per page, arrows + page dots
    let mut preset_pick: Option<kn_format::ObjId> = None;
    if let DrawableKind::Shape(s) = &d.kind {
        if !s.is_line {
            let textbox = s.is_text_box;
            let presets = app.doc.as_ref().map(|doc| doc.shape_style_presets(textbox)).unwrap_or_default();
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
                                    let g = app.doc.as_ref().map(|doc| kn_model::style::graphic(&doc.file, Some(pid)));
                                    let inner = r.shrink(7.0);
                                    match g.as_ref().map(|g| &g.fill) {
                                        Some(Fill::Color(c)) => {
                                            ui.painter().rect_filled(inner, 3.0, c32(*c));
                                        }
                                        Some(Fill::Gradient { stops, .. }) if !stops.is_empty() => {
                                            ui.painter().rect_filled(inner, 3.0, c32(stops[0].1));
                                        }
                                        _ => {
                                            ui.painter().rect_filled(inner, 3.0, crate::theme::gray(0x2a));
                                            crate::theme::centered(ui.painter(), inner.center(), "Aa", egui::FontId::proportional(16.0), theme::TEXT());
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
        let presets = app.doc.as_ref().map(|doc| doc.image_style_presets()).unwrap_or_default();
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
                                let g = app.doc.as_ref().map(|doc| kn_model::style::graphic(&doc.file, Some(pid)));
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
                                        p.rect_filled(inner, 1.0, crate::theme::gray(0x60));
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
                fill_change = fill_editor(ui, "fill", &s.fill, app.doc.as_mut());
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
    let sw = if d.shadow.is_some() { Swatch::Color(crate::theme::gray(40)) } else { Swatch::None };
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
            app.doc.as_ref().and_then(|doc| doc.file.obj(d.id)).and_then(|m| m.find_super("TSD.DrawableArchive")).is_some_and(|dm| {
                dm.bool(hidden) == Some(false) && dm.ref_id(field).is_some_and(|c| app.doc.as_ref().is_some_and(|doc| doc.file.type_name(c) == "TSA.CaptionInfoArchive"))
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
            if let Some(doc) = app.doc.as_mut() {
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
fn icon_segments(ui: &mut egui::Ui, id: &str, n: usize, selected: Option<usize>, draw: impl Fn(&egui::Painter, egui::Rect, usize, Color32)) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, 7.0, crate::theme::gray(0x33));
    let w = rect.width() / n as f32;
    let mut clicked = None;
    for i in 0..n {
        let r = egui::Rect::from_min_size(egui::pos2(rect.min.x + w * i as f32, rect.min.y), egui::vec2(w, rect.height()));
        let resp = ui.interact(r, ui.scope_id().with((id, i)), egui::Sense::click());
        let sel = selected == Some(i);
        if sel {
            p.rect_filled(r.shrink(1.0), 6.0, theme::ACCENT());
        } else if resp.hovered() {
            p.rect_filled(r.shrink(2.0), 5.0, crate::theme::gray(0x40));
        }
        if i > 0 && !sel && selected != Some(i - 1) {
            p.line_segment([egui::pos2(r.min.x, r.min.y + 8.0), egui::pos2(r.min.x, r.max.y - 8.0)], egui::Stroke::new(1.0, crate::theme::gray(0x55)));
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
fn text(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
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
    let pstyle = app.doc.as_ref().and_then(|doc| doc.paragraph_style_name(tb.storage)).unwrap_or_else(|| "Body".into());
    // a light button with dark text in both appearances (theme::gray flips in light mode, which
    // made it a dark slab there)
    let (pfill, pink) = if crate::theme::is_light() { (crate::theme::tint(Color32::from_gray(0xdc)), Color32::from_gray(0x10)) } else { (crate::theme::gray(0xc8), crate::theme::gray(0x10)) };
    let fr = egui::Frame::new().fill(pfill).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(&pstyle).size(24.0).color(pink));
    });
    let r = fr.response.rect;
    crate::icons::draw(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.max.x - 22.0, r.center().y), egui::vec2(16.0, 16.0)), crate::icons::Icon::ChevronDown, if crate::theme::is_light() { Color32::from_gray(0x30) } else { crate::theme::gray(0x30) });
    // the paragraph styles, each in its own look (Keynote's paragraph style pop-up)
    let pr = ui.interact(r, ui.scope_id().with("para-style"), egui::Sense::click());
    let mut pick_style = None;
    egui::Popup::menu(&pr).width(r.width()).show(|ui| {
        let styles = app.doc.as_ref().map(|d| d.paragraph_styles()).unwrap_or_default();
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
            p.rect_filled(rect, 7.0, crate::theme::gray(0x33));
            let cw = rect.width() / 4.0;
            for (i, (lab, on)) in ["B", "I", "U", "S"].iter().zip(states).enumerate() {
                let r = egui::Rect::from_min_size(egui::pos2(rect.min.x + cw * i as f32, rect.min.y), egui::vec2(cw, rect.height()));
                let resp = ui.interact(r, ui.scope_id().with(("bius", i)), egui::Sense::click());
                if on {
                    p.rect_filled(r.shrink(1.0), 6.0, theme::ACCENT());
                } else if resp.hovered() {
                    p.rect_filled(r.shrink(2.0), 5.0, crate::theme::gray(0x40));
                }
                let font = if i == 0 { egui::FontId::new(15.0, egui::FontFamily::Name("bold".into())) } else { egui::FontId::proportional(15.0) };
                // white on the accent; off, the theme's text grey (white on the light mode's
                // light grey was hard to see)
                let fg = if on { Color32::WHITE } else { crate::theme::gray(0xee) };
                let g = p.layout_no_wrap(lab.to_string(), font, fg);
                let pos = r.center() - g.size() / 2.0;
                p.galley(pos, g.clone(), fg);
                if i == 2 {
                    p.line_segment([egui::pos2(pos.x, pos.y + g.size().y - 2.0), egui::pos2(pos.x + g.size().x, pos.y + g.size().y - 2.0)], egui::Stroke::new(1.2, fg));
                }
                if i == 3 {
                    p.line_segment([egui::pos2(pos.x - 1.0, r.center().y), egui::pos2(pos.x + g.size().x + 1.0, r.center().y)], egui::Stroke::new(1.2, fg));
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
            let styles = app.doc.as_ref().map(|d| d.character_styles()).unwrap_or_default();
            let mut pick: Option<Option<kn_format::ObjId>> = None;
            theme::combo("charstyles").selected_text("None").width(150.0).show_ui(ui, |ui| {
                if ui.selectable_label(false, "None").clicked() {
                    pick = Some(None);
                }
                for (id, name) in &styles {
                    if ui.selectable_label(false, name).clicked() {
                        pick = Some(Some(*id));
                    }
                }
            });
            if let Some(style) = pick {
                // the selected text while editing, else all of it
                let range = match app.canvas.editing.as_ref() {
                    Some(e) if e.cursor != e.anchor => Some((e.storage, e.cursor.min(e.anchor), e.cursor.max(e.anchor))),
                    _ => Some((tb.storage, 0, tb.text.len())),
                };
                if let (Some((st, a, b)), Some(doc)) = (range, app.doc.as_mut()) {
                    doc.begin(&mut app.hist, "Character Style");
                    doc.set_character_style(&mut app.hist, st, a, b, style);
                    doc.end(&mut app.hist);
                    if app.canvas.editing.is_some() {
                        doc.begin(&mut app.hist, "Typing");
                    }
                }
                if let Some(e) = app.canvas.editing.as_mut() {
                    e.version += 1;
                    e.scene = None;
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
fn arrange(app: &mut App, ui: &mut egui::Ui, selected: &[kn_model::Drawable]) {
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
                ui.painter().rect_filled(rect, 7.0, crate::theme::gray(0x33));
                let w = rect.width() / 2.0;
                for (i, (label, cmd, top)) in pair.iter().enumerate() {
                    let r = egui::Rect::from_min_size(egui::pos2(rect.min.x + w * i as f32, rect.min.y), egui::vec2(w, rect.height()));
                    let resp = ui.interact(r, ui.scope_id().with(("layer", *label)), egui::Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r.shrink(2.0), 5.0, crate::theme::gray(0x44));
                    }
                    layer_icon(ui.painter(), r.center(), *top, theme::TEXT());
                    crate::theme::centered(ui.painter(), egui::pos2(r.center().x, rect.max.y + 10.0), *label, egui::FontId::new(12.0, egui::FontFamily::Name("bold".into())), theme::TEXT());
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
    let g0 = app.renderer.effective_geometry(d);
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
            ui.painter().circle_filled(c, 16.0, crate::theme::gray(0x3a));
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
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Change Geometry");
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

pub fn transition_names() -> Vec<(String, String)> {
    include_str!("../data/transitions.txt")
        .lines()
        .skip(2)
        .filter_map(|l| {
            let mut it = l.split_whitespace().collect::<Vec<_>>();
            let id = it.pop()?.to_string();
            let name = it.join(" ");
            (!name.is_empty()).then(|| (title_case(&name), id))
        })
        .collect()
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn animate(app: &mut App, ui: &mut egui::Ui) {
    let Some(scene) = app.scene(app.cur) else { return };
    if !app.sel.is_empty() {
        build_inspector(app, ui, &scene);
        return;
    }
    // Keynote: "Transitions" heading, effect (or "No Transition Effect" + Add an Effect),
    // then Start Transition / Delay side by side, Build Order button at the bottom
    ui.vertical_centered(|ui| ui.label(RichText::new("Transitions").color(theme::TEXT_DIM())));
    ui.separator();
    let Some(info) = app.current_slide().cloned() else { return };
    let tr = scene.transition.clone();
    let names = transition_names();
    let current = tr.as_ref().map(|t| t.effect.clone()).unwrap_or_else(|| "none".into());
    let current_name = names.iter().find(|(_, id)| *id == current).map(|(n, _)| n.clone()).unwrap_or_else(|| if current == "none" { "None".into() } else { pretty_effect(&current) });
    let mut effect = current.clone();
    let mut duration = tr.as_ref().map(|t| t.duration).unwrap_or(1.0);
    let mut delay = tr.as_ref().map(|t| t.delay).unwrap_or(0.0);
    let mut automatic = tr.as_ref().map(|t| t.automatic).unwrap_or(false);
    let picker = |ui: &mut egui::Ui, effect: &mut String, with_none: bool| {
        egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
            ui.set_min_width(220.0);
            if with_none {
                ui.selectable_value(effect, "none".to_string(), "None");
                ui.separator();
            }
            for (n, id) in &names {
                ui.selectable_value(effect, id.clone(), n);
            }
        });
    };
    if current == "none" {
        ui.add_space(6.0);
        ui.vertical_centered(|ui| ui.label(RichText::new("No Transition Effect").size(17.0).color(theme::TEXT_DIM())));
        ui.add_space(8.0);
        let resp = ui.add_sized([ui.available_width(), 30.0], egui::Button::new(RichText::new("Add an Effect").size(14.0).color(Color32::WHITE)).fill(theme::ACCENT()));
        egui::Popup::menu(&resp).show(|ui| picker(ui, &mut effect, false));
    } else {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&current_name).size(17.0).family(egui::FontFamily::Name("bold".into())));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let resp = ui.button("Change");
                egui::Popup::menu(&resp).show(|ui| picker(ui, &mut effect, true));
            });
        });
        ui.add_space(4.0);
        if ui.add_sized([ui.available_width(), 26.0], egui::Button::new("Preview")).clicked() {
            app.start_slideshow(app.cur.saturating_sub(1));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Duration");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::stepper(ui, &mut duration, 0.1, 0.1..=60.0, " s", 2);
            });
        });
    }
    ui.separator();
    egui::Grid::new("tr-start").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        ui.label(RichText::new("Start Transition").family(egui::FontFamily::Name("bold".into())));
        ui.label(RichText::new("Delay").family(egui::FontFamily::Name("bold".into())));
        ui.end_row();
        theme::combo("tr-auto").selected_text(if automatic { "Automatically" } else { "On Click" }).width(150.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut automatic, false, "On Click");
            ui.selectable_value(&mut automatic, true, "Automatically");
        });
        ui.add_enabled_ui(automatic, |ui| theme::stepper(ui, &mut delay, 0.1, 0.0..=60.0, " s", 2));
        ui.end_row();
    });
    ui.separator();
    let room = ui.available_height() - 40.0;
    if room > 0.0 {
        ui.add_space(room);
    }
    if ui.add_sized([ui.available_width(), 28.0], egui::Button::new("Build Order")).clicked() {
        app.insp.show_build_order = !app.insp.show_build_order;
    }
    let changed = effect != current
        || tr.as_ref().is_some_and(|t| t.duration != duration || t.delay != delay || t.automatic != automatic)
        || (tr.is_none() && automatic);
    if changed {
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Transition");
            doc.set_transition(&mut app.hist, info.slide, &effect, duration, delay, automatic);
            doc.end(&mut app.hist);
        }
        app.bump(info.slide);
    }
}

pub fn pretty_effect(id: &str) -> String {
    if let Some((n, _)) = transition_names().into_iter().find(|(_, i)| i == id) {
        return n;
    }
    let s = id.rsplit(':').next().unwrap_or(id).rsplit('.').next().unwrap_or(id);
    let s = s.trim_start_matches("bc-").trim_start_matches("action-").replace(['-', '_'], " ");
    title_case(&s)
}

// --------------------------------------------------------------------------- document

/// Document inspector, laid out like Keynote 15.4: theme card, Slideshow Settings, Presentation
/// Type (+ delays for self-playing), Slide Size, password.
fn document(app: &mut App, ui: &mut egui::Ui) {
    use kn_model::slides::{ShowMode, ShowSetting};
    let mut t = app.insp.doc_tab;
    tabs(ui, &["Document", "Audio"], &mut t);
    app.insp.doc_tab = t;
    let Some(doc) = app.doc.as_ref() else { return };
    if t == 1 {
        section(ui, "Soundtrack");
        ui.label(RichText::new("No soundtrack").color(theme::TEXT_DIM()));
        return;
    }
    let theme_name = doc.theme_name();
    let st = doc.show_settings();
    let size = doc.slide_size();
    let first = app.slides.first().map(|s| s.slide);
    let mut change: Option<ShowSetting> = None;
    // theme card: the first slide's thumbnail, theme name, Change Theme
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(120.0, 120.0 * size.h / size.w.max(1.0)), egui::Sense::hover());
        let ctx = ui.ctx().clone();
        let mut budget = crate::textures::Budget::new(10);
        let tex = first.and_then(|sid| {
            let doc = app.doc.as_ref()?;
            app.tex.thumb(&ctx, &mut app.renderer, doc, || doc.scene(&doc.slides(), 0), sid, u64::MAX, &mut budget)
        });
        match tex {
            Some(t) => {
                ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                ui.painter().rect_filled(r, 2.0, Color32::BLACK);
            }
        }
        ui.vertical(|ui| {
            ui.label(RichText::new("Theme").color(theme::TEXT_DIM()));
            ui.label(theme::bold(&theme_name));
            if ui.button("Change Theme").clicked() {
                app.changing_theme = true;
                app.show_theme_chooser = true;
            }
        });
    });
    section(ui, "Slideshow Settings");
    let mut auto = st.auto_play_on_open;
    if theme::checkbox(ui, &mut auto, "Automatically play upon open").changed() {
        change = Some(ShowSetting::AutoPlayOnOpen(auto));
    }
    let mut looping = st.looping;
    if theme::checkbox(ui, &mut looping, "Loop slideshow").changed() {
        change = Some(ShowSetting::Loop(looping));
    }
    ui.horizontal(|ui| {
        ui.add_space(26.0);
        ui.label(theme::bold("Press \u{2018}Esc\u{2019} key to exit").size(12.0));
    });
    let mut idle = st.restart_if_idle;
    if theme::checkbox(ui, &mut idle, "Restart show if idle for").changed() {
        change = Some(ShowSetting::RestartIfIdle(idle));
    }
    ui.horizontal(|ui| {
        ui.add_space(26.0);
        let mut m = st.idle_minutes;
        ui.add_enabled_ui(idle, |ui| {
            if theme::stepper(ui, &mut m, 1.0, 1.0..=1440.0, " m", 0) {
                change = Some(ShowSetting::IdleMinutes(m));
            }
        });
    });
    section(ui, "Presentation Type");
    let names = [(ShowMode::Normal, "Normal"), (ShowMode::LinksOnly, "Links Only"), (ShowMode::SelfPlaying, "Self-Playing")];
    let cur = names.iter().find(|n| n.0 == st.mode).map(|n| n.1).unwrap_or("Normal");
    theme::combo("show-mode").selected_text(cur).width(ui.available_width()).show_ui(ui, |ui| {
        for (m, n) in names {
            if ui.selectable_label(m == st.mode, n).clicked() && m != st.mode {
                change = Some(ShowSetting::Mode(m));
            }
        }
    });
    ui.add_enabled_ui(st.mode == ShowMode::SelfPlaying, |ui| {
        ui.horizontal(|ui| {
            ui.label("Delay:");
            ui.columns(2, |c| {
                c[0].vertical_centered(|ui| {
                    let mut v = st.transition_delay;
                    if theme::stepper(ui, &mut v, 0.5, 0.0..=600.0, " s", 1) {
                        change = Some(ShowSetting::TransitionDelay(v));
                    }
                    ui.label(theme::bold("Transitions").size(12.0));
                });
                c[1].vertical_centered(|ui| {
                    let mut v = st.build_delay;
                    if theme::stepper(ui, &mut v, 0.5, 0.0..=600.0, " s", 1) {
                        change = Some(ShowSetting::BuildDelay(v));
                    }
                    ui.label(theme::bold("Builds").size(12.0));
                });
            });
        });
    });
    section(ui, "Slide Size");
    // Keynote's popup: Standard (4:3) 1024 × 768, Wide (16:9) 1920 × 1080, Custom Slide Size…
    let is = |w: f32, h: f32| (size.w - w).abs() < 0.5 && (size.h - h).abs() < 0.5;
    let size_name = if is(1920.0, 1080.0) {
        "Wide (16:9)".to_string()
    } else if is(1024.0, 768.0) {
        "Standard (4:3)".to_string()
    } else {
        format!("Custom ({:.0} \u{00d7} {:.0})", size.w, size.h)
    };
    let mut new_size: Option<(f32, f32)> = None;
    theme::combo("slide-size").selected_text(size_name).width(ui.available_width()).show_ui(ui, |ui| {
        if ui.selectable_label(is(1024.0, 768.0), "Standard (4:3)").clicked() {
            new_size = Some((1024.0, 768.0));
        }
        if ui.selectable_label(is(1920.0, 1080.0), "Wide (16:9)").clicked() {
            new_size = Some((1920.0, 1080.0));
        }
        ui.separator();
        if ui.selectable_label(false, "Custom Slide Size\u{2026}").clicked() {
            app.insp.custom_size = Some((size.w, size.h));
        }
    });
    if let Some((mut w, mut h)) = app.insp.custom_size {
        let mut open = true;
        let mut apply = false;
        egui::Window::new("Custom Slide Size").collapsible(false).resizable(false).open(&mut open).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ui.ctx(), |ui| {
            egui::Grid::new("custom-size").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Width:");
                theme::stepper32(ui, &mut w, 1.0, 16.0..=10000.0, " pt", 0);
                ui.end_row();
                ui.label("Height:");
                theme::stepper32(ui, &mut h, 1.0, 16.0..=10000.0, " pt", 0);
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    app.insp.custom_size = None;
                }
                if ui.button("OK").clicked() {
                    apply = true;
                }
            });
        });
        if app.insp.custom_size.is_some() {
            app.insp.custom_size = Some((w, h));
        }
        if apply {
            new_size = Some((w, h));
            app.insp.custom_size = None;
        }
        if !open {
            app.insp.custom_size = None;
        }
    }
    if let Some((w, h)) = new_size {
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Slide Size");
            let changed = doc.set_slide_size(&mut app.hist, w, h);
            doc.end(&mut app.hist);
            if changed {
                app.bump_all();
            }
        }
    }
    ui.add_space(6.0);
    ui.separator();
    let has_pw = app.doc.as_ref().is_some_and(|d| d.password.is_some());
    let mut pw = has_pw;
    if theme::checkbox(ui, &mut pw, "Require password to open").changed() {
        if pw {
            app.dialogs.set_password = Some((String::new(), String::new(), String::new()));
        } else if let Some(doc) = app.doc.as_mut() {
            doc.password = None;
            doc.dirty = true;
        }
    }
    ui.add_enabled_ui(has_pw, |ui| {
        ui.vertical_centered(|ui| {
            if ui.add(egui::Button::new("Change Password\u{2026}").min_size(egui::vec2(220.0, 26.0))).clicked() {
                app.dialogs.set_password = Some((String::new(), String::new(), String::new()));
            }
        })
    });
    if let Some(c) = change {
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Slideshow Settings");
            doc.set_show_setting(&mut app.hist, c);
            doc.end(&mut app.hist);
        }
    }
}

/// Animate inspector with objects selected: Build In / Action / Build Out effects (add, change,
/// remove) and the slide's Build Order (start on click / with / after previous).
fn build_inspector(app: &mut App, ui: &mut egui::Ui, scene: &kn_model::SlideScene) {
    use kn_model::buildedit::{effect_name, BuildKind, EFFECTS};
    let mut t = app.insp.animate_tab;
    tabs(ui, &["Build In", "Action", "Build Out"], &mut t);
    app.insp.animate_tab = t;
    let kind = [BuildKind::In, BuildKind::Action, BuildKind::Out][t];
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    let target = app.sel[0];
    let builds: Vec<_> = scene.builds.iter().filter(|b| b.drawable == target && b.kind == kind.as_str()).cloned().collect();
    enum Act {
        Add(String),
        Change(kn_format::ObjId, String, f64),
        Remove(kn_format::ObjId),
        Preview,
    }
    let mut act = None;
    let mut delivery: Option<(kn_format::ObjId, kn_model::buildedit::Delivery)> = None;
    let mut option: Option<(kn_format::ObjId, kn_model::buildedit::BuildOption)> = None;
    let mut reorder: Option<(usize, usize)> = None;
    // Keynote's effect picker: "None", then named sections (Keynote 15.4 screenshots)
    let sections: &[(&str, &[&str])] = match kind {
        BuildKind::Action => &[("Basic", &["Move", "Opacity", "Rotate", "Scale"]), ("Emphasis", &["Blink", "Bounce", "Flip", "Jiggle", "Pop", "Pulse"])],
        _ => &[
            ("Appear & Move", &["Appear", "Blur", "Compress", "Dissolve", "Drift", "Drift and Scale", "Drop", "Fade and Move", "Fade and Scale", "Fly In", "Iris", "Keyboard", "Move In", "Radial Wipe", "Shift", "Shift and Scale", "Wipe"]),
            ("Flip, Spin & Scale", &["Blinds", "Cube", "Flip", "Orbital", "Pivot", "Pop", "Scale", "Scale Big", "Spin", "Swoosh", "Twirl", "Twist and Scale"]),
            ("Special Effects", &["Anvil", "Blast", "Bouncy", "Comet", "Confetti", "Fireworks", "Flame", "Flash Bulbs", "Lens Flare", "Shimmer", "Skid", "Sparkle", "Squish", "Trace"]),
        ],
    };
    // UI name -> effect id (Keynote shows one name; prefer the id verified in a Keynote file)
    let id_for = |name: &'static str| -> Option<&'static str> {
        let out_names = |n: &'static str| -> &'static str { match (kind, n) {
            (BuildKind::Out, "Appear") => "Disappear",
            (BuildKind::Out, "Move In") => "Move Out",
            (BuildKind::Out, "Fly In") => "Fly Out",
            (BuildKind::Action, "Scale") => "Scale (action)",
            (BuildKind::Action, "Pop") => "Pop (action)",
            (BuildKind::Action, "Flip") => "Flip (action)",
            (_, other) => other,
        } };
        let n = out_names(name);
        let mut found: Vec<&(&str, &str, &[BuildKind], bool)> = EFFECTS.iter().filter(|(en, _, ks, _)| ks.contains(&kind) && (*en == n || en.strip_suffix(" (by character)") == Some(n))).collect();
        found.sort_by_key(|e| !e.3);
        found.first().map(|e| e.1)
    };
    let picker = |ui: &mut egui::Ui, current: Option<&str>, act: &mut Option<Act>, build: Option<kn_format::ObjId>, dur: f64| {
        egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
            ui.set_min_width(230.0);
            if ui.selectable_label(build.is_none(), "None").clicked() {
                if let Some(b) = build {
                    *act = Some(Act::Remove(b));
                }
            }
            for (title, names) in sections {
                ui.separator();
                ui.label(RichText::new(*title).size(12.0).color(theme::TEXT_DIM()));
                for n in *names {
                    match id_for(n) {
                        Some(id) => {
                            if ui.selectable_label(current == Some(id), format!("   {n}")).clicked() {
                                *act = Some(match build {
                                    Some(b) => Act::Change(b, id.to_string(), dur),
                                    None => Act::Add(id.to_string()),
                                });
                            }
                        }
                        None => {
                            ui.add_enabled(false, egui::Button::selectable(false, format!("   {n}"))).on_disabled_hover_text("Not available yet");
                        }
                    }
                }
            }
        });
    };
    // Keynote's popup button look: full width, value left, up/down chevrons right
    fn popup<R>(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, text: &str, add: impl FnOnce(&mut egui::Ui) -> R) {
        theme::combo(id).selected_text(text).width(ui.available_width()).show_ui(ui, add);
    }
    if builds.is_empty() {
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(["No Build In Effect", "No Action Build Effect", "No Build Out Effect"][t]).size(17.0).color(theme::TEXT_DIM()));
        });
        ui.add_space(8.0);
        let resp = ui.add_sized([ui.available_width(), 30.0], egui::Button::new(RichText::new("Add an Effect").size(14.0).color(Color32::WHITE)).fill(theme::ACCENT()));
        egui::Popup::menu(&resp).show(|ui| picker(ui, None, &mut act, None, 1.0));
    }
    let n_steps = scene.build_order.len();
    for b in &builds {
        let name = effect_name(&b.effect, &b.kind).map(|n| n.trim_end_matches(" (by character)").trim_end_matches(" (action)").to_string()).unwrap_or_else(|| pretty_effect(&b.effect));
        // header: effect tile, name, Change and Preview
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let (tile, _) = ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
            let inner = tile.shrink(6.0);
            ui.painter().rect_filled(inner, 3.0, Color32::from_rgb(0x3d, 0x6a, 0x9c));
            crate::theme::centered(ui.painter(), inner.center(), name.chars().next().unwrap_or(' ').to_string(), egui::FontId::proportional(22.0), Color32::WHITE);
            ui.vertical(|ui| {
                ui.label(RichText::new(&name).size(18.0).family(egui::FontFamily::Name("bold".into())));
                ui.horizontal(|ui| {
                    let w = ((ui.available_width() - 8.0) / 2.0).max(60.0);
                    let resp = ui.add_sized([w, 26.0], egui::Button::new("Change"));
                    egui::Popup::menu(&resp).show(|ui| picker(ui, Some(b.effect.as_str()), &mut act, Some(b.id), b.duration));
                    if ui.add_sized([w, 26.0], egui::Button::new("Preview  ▶")).clicked() {
                        act = Some(Act::Preview);
                    }
                });
            });
        });
        ui.separator();
        use kn_model::buildedit::BuildOption as O;
        let p = &b.params;
        let e = b.effect.as_str();
        let directional = direction_options(e).is_some();
        let rotate = e == "apple:action-rotation";
        // Duration (& Direction): slider + seconds stepper
        ui.label(RichText::new(if directional { "Duration & Direction" } else { "Duration" }).family(egui::FontFamily::Name("bold".into())));
        ui.horizontal(|ui| {
            let mut dur = b.duration;
            ui.spacing_mut().slider_width = (ui.available_width() - 96.0).max(60.0);
            if ui.add(egui::Slider::new(&mut dur, 0.1..=10.0).logarithmic(true).show_value(false)).drag_stopped() {
                act = Some(Act::Change(b.id, b.effect.clone(), dur));
            }
            let mut v = b.duration;
            if theme::stepper(ui, &mut v, 0.1, 0.1..=60.0, " s", 2) {
                act = Some(Act::Change(b.id, b.effect.clone(), v));
            }
        });
        if let Some((opts, default)) = direction_options(e) {
            // a missing or unknown value means the effect's default (Keynote, verified on the Mac)
            let cur_v = if opts.iter().any(|o| o.0 == b.direction) { b.direction } else { default };
            let cur = opts.iter().find(|o| o.0 == cur_v).map(|o| o.1).unwrap_or("");
            popup(ui, ("dir", b.id), cur, |ui| {
                for (v, n) in &opts {
                    if ui.selectable_label(*v == cur_v, *n).clicked() && *v != cur_v {
                        option = Some((b.id, O::Direction(*v)));
                    }
                }
            });
        }
        if e.contains("move in") {
            let mut bounce = p.bounce;
            if theme::checkbox(ui, &mut bounce, "Bounce").changed() {
                option = Some((b.id, O::Bounce(bounce)));
            }
        }
        if rotate {
            let cw = p.rotation_clockwise;
            popup(ui, ("rotdir", b.id), if cw { "↻  Clockwise" } else { "↺  Counterclockwise" }, |ui| {
                if ui.selectable_label(cw, "↻  Clockwise").clicked() && !cw {
                    option = Some((b.id, O::RotationClockwise(true)));
                }
                if ui.selectable_label(!cw, "↺  Counterclockwise").clicked() && cw {
                    option = Some((b.id, O::RotationClockwise(false)));
                }
            });
            ui.separator();
            ui.label(RichText::new("Angle & Rotation").family(egui::FontFamily::Name("bold".into())));
            // Keynote shows the angle within a turn plus whole rotations; the file stores the total
            let total = p.rotation_angle.unwrap_or(90.0) as f64;
            let mut turns = (total / 360.0).floor();
            let mut angle = total - turns * 360.0;
            ui.columns(2, |c| {
                c[0].vertical_centered(|ui| {
                    if theme::stepper(ui, &mut angle, 1.0, 0.0..=359.0, "°", 0) {
                        option = Some((b.id, O::RotationAngle(turns * 360.0 + angle)));
                    }
                    ui.label(RichText::new("Angle").size(12.0).family(egui::FontFamily::Name("bold".into())));
                });
                c[1].vertical_centered(|ui| {
                    if theme::stepper(ui, &mut turns, 1.0, 0.0..=9.0, "", 0) {
                        option = Some((b.id, O::RotationAngle(turns * 360.0 + angle)));
                    }
                    ui.label(RichText::new("Rotations").size(12.0).family(egui::FontFamily::Name("bold".into())));
                });
            });
        }
        if e == "apple:action-scale" {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Scale").family(egui::FontFamily::Name("bold".into())));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut pct = p.scale.unwrap_or(1.0) as f64 * 100.0;
                    if theme::stepper(ui, &mut pct, 1.0, 1.0..=5000.0, " %", 0) {
                        option = Some((b.id, O::ScaleSize(pct / 100.0)));
                    }
                });
            });
        }
        if e == "apple:action-opacity" {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Opacity").family(egui::FontFamily::Name("bold".into())));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut pct = p.opacity.unwrap_or(1.0) as f64 * 100.0;
                    if theme::stepper(ui, &mut pct, 1.0, 0.0..=100.0, " %", 0) {
                        option = Some((b.id, O::Opacity(pct / 100.0)));
                    }
                });
            });
        }
        if b.kind == "Action" {
            ui.separator();
            ui.label(RichText::new("Acceleration").family(egui::FontFamily::Name("bold".into())));
            let names = ["None", "Ease In", "Ease Out", "Ease In & Out"];
            let cur = p.acceleration.min(3) as usize;
            popup(ui, ("accel", b.id), names[cur], |ui| {
                for (i, n) in names.iter().enumerate() {
                    if ui.selectable_label(i == cur, *n).clicked() && i != cur {
                        option = Some((b.id, O::Acceleration(i as u32)));
                    }
                }
            });
        }
        let is_text = scene.drawables.iter().find(|d| d.id == b.drawable).is_some_and(|d| matches!(&d.kind, DrawableKind::Shape(s) if s.text.as_ref().is_some_and(|t| !t.text.trim().is_empty())));
        let cur_delivery = app.doc.as_ref().map(|d| d.build_delivery(b.id)).unwrap_or(kn_model::buildedit::Delivery::AllAtOnce);
        if is_text && b.kind != "Action" {
            use kn_model::buildedit::Delivery as D;
            ui.separator();
            ui.label(RichText::new("Text Animation").family(egui::FontFamily::Name("bold".into())));
            let ta = match cur_delivery {
                D::ByWord => "By Word",
                D::ByCharacter => "By Character",
                _ => "By Object",
            };
            popup(ui, ("textanim", b.id), ta, |ui| {
                for (d, n) in [(D::AllAtOnce, "By Object"), (D::ByWord, "By Word"), (D::ByCharacter, "By Character")] {
                    if ui.selectable_label(n == ta, n).clicked() && n != ta {
                        delivery = Some((b.id, d));
                    }
                }
            });
        }
        // Order: this build's place in the Build Order
        if let Some(pos) = scene.build_order.iter().position(|s| s.build == b.id) {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Order").family(egui::FontFamily::Name("bold".into())));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    theme::combo(("order", b.id)).selected_text(format!("{}", pos + 1)).width(80.0).show_ui(ui, |ui| {
                        for k in 0..n_steps {
                            if ui.selectable_label(k == pos, format!("{}", k + 1)).clicked() && k != pos {
                                reorder = Some((pos, k));
                            }
                        }
                    });
                });
            });
        }
        if is_text && b.kind != "Action" {
            use kn_model::buildedit::Delivery as D;
            ui.separator();
            ui.label(RichText::new("Delivery").family(egui::FontFamily::Name("bold".into())));
            let dv = match cur_delivery {
                D::ByParagraph | D::ByParagraphGroup | D::ByHighlightedParagraph => cur_delivery.name(),
                _ => "All at Once",
            };
            popup(ui, ("delivery", b.id), dv, |ui| {
                for d in [D::AllAtOnce, D::ByParagraph, D::ByParagraphGroup, D::ByHighlightedParagraph] {
                    let n = d.name();
                    if ui.selectable_label(n == dv, n).clicked() && n != dv {
                        delivery = Some((b.id, d));
                    }
                }
            });
        }
        if b.kind == "Action" {
            ui.separator();
            let resp = ui.add_sized([ui.available_width(), 28.0], egui::Button::new("Add Action"));
            egui::Popup::menu(&resp).show(|ui| picker(ui, None, &mut act, None, 1.0));
        }
    }
    if let Some((from, to)) = reorder {
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Reorder Builds");
            doc.move_build_step(&mut app.hist, slide, from, to);
            doc.end(&mut app.hist);
        }
        app.bump(slide);
    }
    // Build Order button at the bottom of the inspector; the list is a floating window (Keynote)
    let room = ui.available_height() - 40.0;
    if room > 0.0 {
        ui.add_space(room);
    }
    ui.separator();
    if ui.add_sized([ui.available_width(), 28.0], egui::Button::new("Build Order")).clicked() {
        app.insp.show_build_order = !app.insp.show_build_order;
    }
    if app.doc.is_none() {
        return;
    }
    if let Some((id, o)) = option {
        let doc = app.doc.as_mut().unwrap();
        doc.begin_merge(&mut app.hist, "Effect Options");
        doc.set_build_option(&mut app.hist, id, o);
        doc.end(&mut app.hist);
        app.bump(slide);
    }
    if let Some((id, d)) = delivery {
        let doc = app.doc.as_mut().unwrap();
        doc.begin(&mut app.hist, "Delivery");
        doc.set_build_delivery(&mut app.hist, slide, id, d);
        doc.end(&mut app.hist);
        app.bump(slide);
    }
    match act {
        Some(Act::Add(effect)) => {
            let doc = app.doc.as_mut().unwrap();
            doc.begin(&mut app.hist, "Add Build");
            let r = doc.add_build(&mut app.hist, slide, target, kind, &effect, 1.0);
            doc.end(&mut app.hist);
            if let Err(e) = r {
                app.notify(format!("Couldn't add the effect: {e}"));
            }
            app.bump(slide);
        }
        Some(Act::Change(id, effect, dur)) => {
            let doc = app.doc.as_mut().unwrap();
            doc.begin(&mut app.hist, "Change Build");
            doc.set_build_effect(&mut app.hist, slide, id, &effect, dur);
            doc.end(&mut app.hist);
            app.bump(slide);
        }
        Some(Act::Remove(id)) => {
            let doc = app.doc.as_mut().unwrap();
            doc.begin(&mut app.hist, "Delete Build");
            doc.remove_build(&mut app.hist, slide, id);
            doc.end(&mut app.hist);
            app.bump(slide);
        }
        Some(Act::Preview) => app.start_slideshow(app.cur),
        None => {}
    }
}

/// Keynote's Build Order window (from Keynote 15.4 screenshots): numbered rows (object icon,
/// object name, effect on the right) where steps that start automatically are joined into one
/// block with the step before (solid line = After, dashed = With); drag a row to reorder; the
/// selected step's Start (On Click / With Build N / After Build N) and Delay at the bottom, then
/// Preview.
pub fn build_order_window(app: &mut App, ctx: &egui::Context) {
    use kn_model::buildedit::{effect_name, BuildStart};
    if !app.insp.show_build_order {
        return;
    }
    let Some(scene) = app.scene(app.cur) else { return };
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    let chunk_ids: Vec<kn_format::ObjId> = app.doc.as_ref().and_then(|d| d.file.obj(slide)).map(|s| s.ref_ids("buildChunks")).unwrap_or_default();
    let n = scene.build_order.len();
    let mut open = true;
    let mut select: Option<(usize, kn_format::ObjId)> = None;
    let mut move_step: Option<(usize, usize)> = None;
    let mut start_change: Option<(kn_format::ObjId, BuildStart, f64)> = None;
    let mut preview = false;
    let sel = app.insp.build_order_sel.min(n.saturating_sub(1));
    let drag_id = egui::Id::unique("build-order-drag");
    egui::Window::new("Build Order")
        .default_size([380.0, 520.0])
        .default_pos([ctx.content_rect().right() - 760.0, 160.0])
        .resizable(true)
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            let list_h = (ui.available_height() - 120.0).max(160.0);
            let mut rows: Vec<egui::Rect> = Vec::new();
            egui::ScrollArea::vertical().max_height(list_h).min_scrolled_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let w = ui.available_width();
                for (i, step) in scene.build_order.iter().enumerate() {
                    // joined to the previous row when it starts automatically
                    let joined = i > 0 && step.automatic;
                    let next_joined = scene.build_order.get(i + 1).is_some_and(|s| s.automatic);
                    if !joined && i > 0 {
                        ui.add_space(8.0);
                    }
                    let (row, resp) = ui.allocate_exact_size(egui::vec2(w, 42.0), egui::Sense::click_and_drag());
                    rows.push(row);
                    let card = egui::Rect::from_min_max(egui::pos2(row.min.x + 34.0, row.min.y), row.max);
                    let p = ui.painter();
                    let r = |top: bool, bottom: bool| egui::CornerRadius {
                        nw: if top { 8 } else { 0 },
                        ne: if top { 8 } else { 0 },
                        sw: if bottom { 8 } else { 0 },
                        se: if bottom { 8 } else { 0 },
                    };
                    let selected = i == sel;
                    let fill = if selected { theme::ACCENT() } else { crate::theme::gray(44) };
                    p.rect_filled(card, r(!joined, !next_joined), fill);
                    if joined {
                        // line between joined steps: solid for After, dashed for With
                        let y = card.min.y;
                        let (x0, x1) = (card.min.x + 8.0, card.max.x - 8.0);
                        let col = Color32::from_gray(if selected { 230 } else { 90 });
                        if step.with_previous {
                            let mut x = x0;
                            while x < x1 {
                                p.line_segment([egui::pos2(x, y), egui::pos2((x + 4.0).min(x1), y)], egui::Stroke::new(1.0, col));
                                x += 7.0;
                            }
                        } else {
                            p.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], egui::Stroke::new(1.0, col));
                        }
                    }
                    crate::theme::centered(&p, egui::pos2(row.min.x + 14.0, row.center().y), format!("{}", i + 1), egui::FontId::proportional(14.0), crate::theme::gray(230));
                    let b = scene.builds.iter().find(|b| b.id == step.build);
                    let d = b.and_then(|b| scene.drawables.iter().find(|d| d.id == b.drawable));
                    let object = d
                        .map(|d| match &d.kind {
                            DrawableKind::Shape(s) => s.text.as_ref().map(|t| t.text.trim().lines().next().unwrap_or("").chars().take(26).collect::<String>()).filter(|t| !t.is_empty()).unwrap_or_else(|| "Shape".into()),
                            DrawableKind::Image(_) => "Image".into(),
                            DrawableKind::Movie(m) => if m.audio_only { "Audio" } else { "Movie" }.into(),
                            DrawableKind::Table(_) => "Table".into(),
                            DrawableKind::Chart(_) => "Chart".into(),
                            DrawableKind::Group(_) => "Group".into(),
                            #[allow(unreachable_patterns)]
                            _ => "Object".into(),
                        })
                        .unwrap_or_default();
                    let effect = b.and_then(|b| effect_name(&b.effect, &b.kind).map(str::to_string)).or_else(|| b.map(|b| pretty_effect(&b.effect))).unwrap_or_default();
                    // small object icon
                    let icon = egui::Rect::from_center_size(egui::pos2(card.min.x + 22.0, card.center().y), egui::vec2(28.0, 18.0));
                    p.rect_filled(icon, 3.0, Color32::from_rgb(0x4a, 0x9c, 0xf5));
                    p.rect_stroke(icon, 3.0, egui::Stroke::new(1.0, crate::theme::gray(200)), egui::StrokeKind::Inside);
                    let fg = if selected { Color32::WHITE } else { crate::theme::gray(225) };
                    p.text(egui::pos2(icon.max.x + 10.0, card.center().y), egui::Align2::LEFT_CENTER, object, egui::FontId::proportional(14.0), fg);
                    p.text(egui::pos2(card.max.x - 14.0, card.center().y), egui::Align2::RIGHT_CENTER, effect, egui::FontId::proportional(14.0), if selected { Color32::WHITE } else { crate::theme::gray(160) });
                    if resp.clicked() || resp.drag_started() {
                        select = Some((i, d.map(|d| d.id).unwrap_or(0)));
                    }
                    if resp.drag_started() {
                        ui.ctx().data_mut(|m| m.insert_temp(drag_id, i));
                    }
                }
            });
            // drag to reorder: drop line and move on release
            if let Some(from) = ui.ctx().data(|m| m.get_temp::<usize>(drag_id)) {
                if let Some(p) = ui.ctx().pointer_interact_pos() {
                    let target = rows.iter().position(|r| p.y < r.center().y).unwrap_or(rows.len());
                    if let Some(r) = rows.get(target.min(rows.len().saturating_sub(1))) {
                        let y = if target >= rows.len() { r.max.y } else { r.min.y };
                        ui.painter().hline(r.min.x + 34.0..=r.max.x, y, egui::Stroke::new(2.0, theme::ACCENT()));
                    }
                    if ui.input(|i| i.pointer.any_released()) {
                        ui.ctx().data_mut(|m| m.remove::<usize>(drag_id));
                        let to = if target > from { target - 1 } else { target };
                        if to != from {
                            move_step = Some((from, to));
                        }
                    }
                }
            }
            ui.separator();
            if n == 0 {
                ui.label(RichText::new("No builds on this slide").color(theme::TEXT_DIM()));
                return;
            }
            let step = &scene.build_order[sel];
            let cur = match (step.automatic, step.with_previous) {
                (false, _) => BuildStart::OnClick,
                (true, false) => BuildStart::AfterPrevious,
                (true, true) => BuildStart::WithPrevious,
            };
            let label = |s: BuildStart| match s {
                BuildStart::OnClick => "On Click".to_string(),
                // the first build is timed from the slide's transition
                BuildStart::WithPrevious if sel == 0 => "With Transition".to_string(),
                BuildStart::AfterPrevious if sel == 0 => "After Transition".to_string(),
                BuildStart::WithPrevious => format!("With Build {sel}"),
                BuildStart::AfterPrevious => format!("After Build {sel}"),
            };
            if let Some(&cid) = chunk_ids.get(sel) {
                egui::Grid::new("bo-start").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    ui.label(RichText::new("Start").family(egui::FontFamily::Name("bold".into())));
                    ui.label(RichText::new("Delay").family(egui::FontFamily::Name("bold".into())));
                    ui.end_row();
                    let mut pick = cur;
                    theme::combo(("bo-start", cid)).selected_text(label(cur)).width(200.0).show_ui(ui, |ui| {
                        // Keynote's order: On Click, With Build N, After Build N (none for the first build)
                        for s in [BuildStart::OnClick, BuildStart::WithPrevious, BuildStart::AfterPrevious] {
                            ui.selectable_value(&mut pick, s, label(s));
                        }
                    });
                    if pick != cur {
                        start_change = Some((cid, pick, step.delay));
                    }
                    let mut delay = step.delay;
                    if theme::stepper(ui, &mut delay, 0.1, 0.0..=60.0, " s", 2) {
                        start_change = Some((cid, cur, delay));
                    }
                    ui.end_row();
                });
            }
            ui.separator();
            ui.vertical_centered(|ui| {
                if ui.add(egui::Button::new("Preview  ▶").min_size(egui::vec2(120.0, 26.0))).clicked() {
                    preview = true;
                }
            });
        });
    app.insp.show_build_order = open;
    if let Some((i, drawable)) = select {
        app.insp.build_order_sel = i;
        if drawable != 0 {
            app.sel = vec![drawable];
        }
    }
    if preview {
        app.start_slideshow(app.cur);
    }
    let Some(doc) = app.doc.as_mut() else { return };
    if let Some((from, to)) = move_step {
        doc.begin(&mut app.hist, "Reorder Builds");
        doc.move_build_step(&mut app.hist, slide, from, to);
        doc.end(&mut app.hist);
        app.insp.build_order_sel = to;
        app.bump(slide);
    } else if let Some((cid, start, delay)) = start_change {
        doc.begin_merge(&mut app.hist, "Build Start");
        doc.set_build_start_mode(&mut app.hist, cid, start, delay);
        doc.end(&mut app.hist);
        app.bump(slide);
    }
}

/// Direction choices of a build effect and its default, as Keynote 15.4 offers them (read back
/// from Keynote's own inspector on the Mac, round 8): stored value, UI label.
fn direction_options(effect: &str) -> Option<(Vec<(u32, &'static str)>, u32)> {
    let ltr8 = vec![
        (11, "→  Left to Right"),
        (12, "←  Right to Left"),
        (13, "↓  Top to Bottom"),
        (14, "↑  Bottom to Top"),
        (21, "↘  Top Left to Bottom Right"),
        (22, "↙  Top Right to Bottom Left"),
        (23, "↗  Bottom Left to Top Right"),
        (24, "↖  Bottom Right to Top Left"),
    ];
    let from8 = vec![
        (11, "→  From Left"),
        (12, "←  From Right"),
        (13, "↓  From Top"),
        (14, "↑  From Bottom"),
        (21, "↘  From Top Left"),
        (22, "↙  From Top Right"),
        (23, "↗  From Bottom Left"),
        (24, "↖  From Bottom Right"),
    ];
    let e = effect.to_ascii_lowercase();
    let pick = |v: &[(u32, &'static str)], keep: &[u32]| v.iter().filter(|o| keep.contains(&o.0)).copied().collect::<Vec<_>>();
    Some(if e.contains("drift and scale") {
        (vec![(43, "↑  Up"), (44, "↓  Down")], 43)
    } else if e.contains("wipe-iris") || e.contains("iris") {
        (vec![(41, "In"), (42, "Out")], 42)
    } else if e.contains("radial") {
        return None;
    } else if e.contains("move in") || e.contains("fade and move") {
        (ltr8, 11)
    } else if e.contains("wipe") {
        (from8, 11)
    } else if e.contains("cube") || e.contains("bc-flip") {
        (pick(&ltr8, &[11, 12, 13, 14]), 11)
    } else if e.contains("blinds") {
        (pick(&ltr8, &[11, 12, 13, 14]), 12)
    } else if e.contains("drift") || e.contains("skid") {
        (pick(&ltr8, &[11, 12]), 11)
    } else if e.contains("swoosh") {
        (pick(&from8, &[11, 12]), 11)
    } else if e.contains("pivot") {
        (pick(&from8, &[21, 22, 23, 24]), 21)
    } else {
        return None;
    })
}
