//! The start screen of all three apps, after Keynote's theme chooser: a card with a sidebar
//! (Recent, All Templates), your recent documents as thumbnails, the themes or templates to
//! start a new one from, and Open… / Create (and Cancel when a document is already open).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use egui::{pos2, vec2, Color32, Rect, RichText, Sense, Stroke, StrokeKind};

use crate::theme;

/// A theme or template to start from.
pub struct Template {
    pub name: String,
    /// its picture, if it has one (else the app draws the tile: `Spec::draw`)
    pub preview: Option<egui::TextureHandle>,
}

/// What the start screen shows.
pub struct Spec<'a> {
    /// the app's own name (top of the sidebar) and its id (preview cache, remembered choice)
    pub app: &'a str,
    pub id: &'a str,
    /// "Choose a Theme" / "Choose a Template", "Themes" / "Templates", "All Themes" / "All Templates"
    pub heading: &'a str,
    pub section: &'a str,
    pub all: &'a str,
    /// "or drop a .key file here"
    pub hint: &'a str,
    pub recent: &'a [PathBuf],
    pub templates: &'a [Template],
    /// the template picked first (by name)
    pub default: &'a str,
    /// the size of a template's tile (16:9 for slides, a page for documents)
    pub tile: egui::Vec2,
    /// draws a template's tile when it has no picture (index into `templates`)
    pub draw: &'a dyn Fn(&egui::Painter, Rect, usize),
    /// a document is open already: offer Cancel
    pub cancel: bool,
    /// a document is opening: a spinner instead
    pub loading: bool,
    /// a recent document's thumbnail when it holds no preview.jpg (a Word file: its first
    /// page, as PNG bytes), made on a thread
    pub render: Option<fn(&Path) -> Option<Vec<u8>>>,
}

/// What the user chose.
pub enum Action {
    Open(PathBuf),
    /// a template (index into `templates`)
    Create(usize),
    /// Open…
    Browse,
    Cancel,
}

/// Preview pictures of recent documents: path -> image once read (None = none in the file).
static PREVIEWS: Mutex<Option<HashMap<PathBuf, Option<egui::ColorImage>>>> = Mutex::new(None);

/// The preview iWork keeps in a document (preview.jpg), read on a thread (the file may be on a
/// slow cloud mount) and cached in ~/.cache/<app>/previews while the file is unchanged.
fn preview_image(ctx: &egui::Context, app: &str, path: &Path, render: Option<fn(&Path) -> Option<Vec<u8>>>) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique(("recent-preview", path));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    let mut g = PREVIEWS.lock().ok()?;
    let map = g.get_or_insert_with(HashMap::new);
    match map.get(path) {
        Some(Some(img)) => {
            let tex = ctx.load_texture(format!("recent-{}", path.display()), img.clone(), egui::TextureOptions::LINEAR);
            ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
            Some(tex)
        }
        Some(None) => None,
        None => {
            map.insert(path.to_path_buf(), None);
            let (p, ctx, app) = (path.to_path_buf(), ctx.clone(), app.to_string());
            std::thread::spawn(move || {
                let img = read_preview(&app, &p, render).and_then(|b| image::load_from_memory(&b).ok()).map(|i| {
                    let i = i.thumbnail(480, 480).to_rgba8();
                    egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw())
                });
                if img.is_some() {
                    if let Ok(mut g) = PREVIEWS.lock() {
                        g.get_or_insert_with(HashMap::new).insert(p, img);
                    }
                    ctx.request_repaint();
                }
            });
            None
        }
    }
}

fn read_preview(app: &str, path: &Path, render: Option<fn(&Path) -> Option<Vec<u8>>>) -> Option<Vec<u8>> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, meta.len(), meta.modified().ok()).hash(&mut h);
    let cache = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?.join(app).join("previews");
    let cached = cache.join(format!("{:016x}.jpg", h.finish()));
    if let Ok(b) = std::fs::read(&cached) {
        return Some(b);
    }
    let packaged = || -> Option<Vec<u8>> {
        if path.is_dir() {
            return std::fs::read(path.join("preview.jpg")).ok();
        }
        let mut z = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
        let mut f = z.by_name("preview.jpg").ok()?;
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut b).ok()?;
        Some(b)
    };
    let bytes = packaged().or_else(|| render.and_then(|r| r(path)))?;
    let _ = std::fs::create_dir_all(&cache);
    let _ = std::fs::write(&cached, &bytes);
    Some(bytes)
}

/// A card: picture (or a drawn tile) with a name under it; selected / hover rings.
fn card(ui: &mut egui::Ui, tex: Option<&egui::TextureHandle>, draw: Option<&dyn Fn(&egui::Painter, Rect)>, name: &str, size: egui::Vec2, selected: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(size + vec2(0.0, 24.0), Sense::click());
    let pic = Rect::from_min_size(r.min, size);
    let p = ui.painter();
    p.add(egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(if theme::is_light() { 28 } else { 60 }) }.as_shape(pic, 6.0));
    match (tex, draw) {
        (Some(t), _) => {
            // the picture fitted into the tile (letterboxed on the tile colour)
            p.rect_filled(pic, 6.0, theme::gray(0x18));
            let [w, h] = t.size();
            let s = (size.x / w as f32).min(size.y / h as f32);
            let fit = Rect::from_center_size(pic.center(), vec2(w as f32 * s, h as f32 * s));
            // rounded like the tile, so the picture's corners don't stick out past its edge
            egui::Image::new((t.id(), fit.size())).corner_radius(6).paint_at(ui, fit);
        }
        (None, Some(d)) => {
            p.rect_filled(pic, 6.0, Color32::WHITE);
            d(p, pic);
        }
        (None, None) => {
            p.rect_filled(pic, 6.0, theme::gray(0x30));
            // (no preview in the file: its kind)
            let ext = std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_else(|| "…".into());
            p.text(pic.center(), egui::Align2::CENTER_CENTER, ext, egui::FontId::proportional(16.0), theme::TEXT_DIM());
        }
    }
    p.rect_stroke(pic, 6.0, Stroke::new(1.0, theme::SEPARATOR()), StrokeKind::Inside);
    if selected {
        p.rect_stroke(pic.expand(3.0), 8.0, Stroke::new(3.0, theme::ACCENT()), StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_stroke(pic.expand(2.0), 7.0, Stroke::new(2.0, theme::ACCENT().gamma_multiply(0.6)), StrokeKind::Inside);
    }
    let shown = std::path::Path::new(name).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|_| name.contains('.')).unwrap_or_else(|| name.to_string());
    let mut job = egui::text::LayoutJob::single_section(shown, egui::TextFormat { font_id: egui::FontId::proportional(12.5), color: if selected { theme::ACCENT() } else { theme::TEXT() }, ..Default::default() });
    job.wrap = egui::text::TextWrapping { max_width: size.x, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    job.halign = egui::Align::Center;
    let galley = p.layout_job(job);
    p.galley(pos2(pic.center().x, pic.max.y + 6.0), galley, theme::TEXT());
    resp
}

fn section(ui: &mut egui::Ui, title: &str) -> Rect {
    ui.add_space(4.0);
    let r = ui.label(RichText::new(title).size(15.0).strong().color(theme::TEXT())).rect;
    ui.add_space(6.0);
    r
}

/// Draws the start screen centred in `ui`'s rectangle; returns what the user chose.
pub fn show(ui: &mut egui::Ui, spec: &Spec) -> Option<Action> {
    let ctx = ui.ctx().clone();
    let screen = ui.max_rect();
    let w = (screen.width() - 64.0).clamp(320.0, 1080.0);
    let h = (screen.height() - 64.0).clamp(300.0, 720.0);
    let cardr = Rect::from_center_size(screen.center(), vec2(w, h));
    let p = ui.painter().clone();
    // a soft shadow (a hard offset slab read as a thick grey edge in light mode)
    p.add(egui::epaint::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if theme::is_light() { 40 } else { 90 }) }.as_shape(cardr, 14.0));
    p.rect_filled(cardr, 14.0, theme::PANEL_BG());
    // (clicks on the card stay on it)
    ui.interact(cardr, egui::Id::unique(("welcome-card", spec.id)), Sense::click_and_drag());

    if spec.loading {
        ui.scope_builder(egui::UiBuilder::new().max_rect(cardr), |ui| {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(h / 2.0 - 30.0);
                    ui.spinner();
                    ui.label("Opening…");
                });
            });
        });
        return None;
    }

    let jump_key = egui::Id::unique(("welcome-jump", spec.id));
    let sel_key = egui::Id::unique(("welcome-template", spec.id));
    let mut selected: String = ctx.data(|d| d.get_temp::<String>(sel_key)).unwrap_or_else(|| {
        spec.templates.iter().find(|t| t.name == spec.default).or(spec.templates.first()).map(|t| t.name.clone()).unwrap_or_default()
    });
    let recent = spec.recent;

    // sidebar
    let side_w = if w > 640.0 { 190.0 } else { 0.0 };
    let side = Rect::from_min_size(cardr.min, vec2(side_w, h));
    let mut jump: Option<u8> = None;
    if side_w > 0.0 {
        p.rect_filled(side, egui::CornerRadius { nw: 14, sw: 14, ne: 0, se: 0 }, theme::SIDEBAR_BG());
        p.vline(side.max.x, side.y_range(), Stroke::new(1.0, theme::SEPARATOR()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(side.shrink2(vec2(12.0, 16.0))), |ui| {
            ui.add_space(26.0);
            ui.label(RichText::new(spec.app).size(20.0).strong());
            ui.add_space(14.0);
            let current = ctx.data(|d| d.get_temp::<u8>(jump_key)).unwrap_or(if recent.is_empty() { 1 } else { 0 });
            for (i, (icon, name)) in [("◷", "Recent"), ("▦", spec.all)].into_iter().enumerate() {
                if i == 0 && recent.is_empty() {
                    continue;
                }
                let on = current == i as u8;
                let b = egui::Button::new(RichText::new(format!("{icon}  {name}")).size(14.0).color(if on { theme::ACCENT() } else { theme::TEXT() }))
                    .fill(if on { theme::ACCENT().gamma_multiply(0.15) } else { Color32::TRANSPARENT })
                    .stroke(Stroke::NONE)
                    .min_size(vec2(ui.available_width(), 30.0));
                if ui.add(b).clicked() {
                    jump = Some(i as u8);
                }
            }
        });
    }
    // the card's edge, after the sidebar so the sidebar doesn't paint over it
    p.rect_stroke(cardr, 14.0, Stroke::new(1.0, theme::SEPARATOR()), StrokeKind::Inside);

    // header, footer, and the scrolling middle
    let main = Rect::from_min_max(pos2(side.max.x, cardr.min.y), cardr.max);
    let header = Rect::from_min_size(main.min + vec2(24.0, 20.0), vec2(main.width() - 48.0, 40.0));
    p.text(header.left_center(), egui::Align2::LEFT_CENTER, spec.heading, egui::FontId::new(26.0, egui::FontFamily::Name("bold".into())), theme::TEXT());
    let footer = Rect::from_min_max(pos2(main.min.x, main.max.y - 56.0), main.max);
    p.hline(footer.x_range(), footer.min.y, Stroke::new(1.0, theme::SEPARATOR()));
    let body = Rect::from_min_max(pos2(main.min.x + 24.0, header.max.y + 10.0), pos2(main.max.x - 12.0, footer.min.y - 4.0));

    let mut action = None;
    let mut recent_top = None;
    let mut templates_top = None;
    // recent documents' tiles: the templates' shape, a little wider for slides
    let recent_tile = if spec.tile.x > spec.tile.y { spec.tile } else { vec2(spec.tile.x * 1.2, spec.tile.y * 1.2) };
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::vertical().id_salt(("welcome-scroll", spec.id)).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(body.width() - 16.0);
            if !recent.is_empty() {
                recent_top = Some(section(ui, "Recent"));
                egui::ScrollArea::horizontal().id_salt(("welcome-recent", spec.id)).auto_shrink([false, true]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 18.0;
                        for path in recent {
                            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            let tex = preview_image(&ctx, spec.id, path, spec.render);
                            let resp = card(ui, tex.as_ref(), None, &name, recent_tile, false).on_hover_text(path.display().to_string());
                            if resp.clicked() {
                                action = Some(Action::Open(path.clone()));
                            }
                        }
                    });
                });
                ui.add_space(14.0);
            }
            templates_top = Some(section(ui, spec.section));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(18.0, 16.0);
                for (i, t) in spec.templates.iter().enumerate() {
                    let draw = |p: &egui::Painter, r: Rect| (spec.draw)(p, r, i);
                    let resp = card(ui, t.preview.as_ref(), Some(&draw), &t.name, spec.tile, t.name == selected);
                    if resp.clicked() {
                        selected = t.name.clone();
                    }
                    if resp.double_clicked() {
                        action = Some(Action::Create(i));
                    }
                }
            });
            ui.add_space(12.0);
            if let Some(j) = jump {
                let target = if j == 0 { recent_top } else { templates_top };
                if let Some(r) = target {
                    ui.scroll_to_rect(r.expand(8.0), Some(egui::Align::TOP));
                }
                ctx.data_mut(|d| d.insert_temp(jump_key, j));
            }
        });
    });

    // footer: Open… on the left; Cancel and Create on the right
    ui.scope_builder(egui::UiBuilder::new().max_rect(footer.shrink2(vec2(20.0, 12.0))), |ui| {
        ui.horizontal_centered(|ui| {
            if ui.add(egui::Button::new(RichText::new("Open…").size(14.0)).min_size(vec2(90.0, 30.0))).clicked() {
                action = Some(Action::Browse);
            }
            ui.label(RichText::new(spec.hint).size(12.0).color(theme::TEXT_DIM()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let b = egui::Button::new(RichText::new("Create").size(14.0).color(Color32::WHITE)).fill(theme::ACCENT()).min_size(vec2(96.0, 30.0));
                if ui.add_enabled(!selected.is_empty(), b).clicked()
                    && let Some(i) = spec.templates.iter().position(|t| t.name == selected)
                {
                    action = Some(Action::Create(i));
                }
                if spec.cancel && (ui.add(egui::Button::new(RichText::new("Cancel").size(14.0)).min_size(vec2(84.0, 30.0))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
                    action = Some(Action::Cancel);
                }
            });
        });
    });
    ctx.data_mut(|d| d.insert_temp(sel_key, selected));
    action
}

/// The start screen over an open document (Values, Letters): a backdrop in the canvas colour
/// below `top` (the title and toolbar stay visible), then the card.
pub fn overlay(ctx: &egui::Context, top: f32, spec: &Spec) -> Option<Action> {
    let screen = ctx.content_rect();
    let area = Rect::from_min_max(pos2(screen.min.x, top), screen.max);
    egui::Area::new(egui::Id::unique(("welcome-overlay", spec.id)))
        .order(egui::Order::Middle)
        .fixed_pos(area.min)
        .show(ctx, |ui| {
            ui.set_min_size(area.size());
            let (r, _) = ui.allocate_exact_size(area.size(), Sense::click_and_drag());
            ui.painter().rect_filled(r, 0.0, theme::CANVAS_BG());
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
            show(&mut child, spec)
        })
        .inner
}
