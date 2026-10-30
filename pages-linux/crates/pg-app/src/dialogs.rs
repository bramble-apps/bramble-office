//! Small windows: Add Link, a comment being written, the Colors window, the template chooser
//! (File > New), Create Style, Insert Equation.

use egui::{vec2, Color32, RichText};
use kn_ui::theme;

use crate::app::{App, Owner};

/// A window that takes the keyboard is open (the canvas and shortcuts keep out of it).
pub fn modal_open(app: &App) -> bool {
    app.link_edit.is_some() || app.comment_edit.is_some() || app.chooser || app.new_style.is_some() || app.equation.is_some() || app.go_to.is_some() || app.doc_title.is_some() || app.show_settings || app.tb.customizing || app.set_password.is_some() || app.tabs.modal()
}

fn centered(ctx: &egui::Context, title: &str) -> egui::Window<'static> {
    egui::Window::new(title.to_string()).collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center())
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    link_window(app, ctx);
    comment_window(app, ctx);
    colors_window(app, ctx);
    chooser(app, ctx);
    new_style(app, ctx);
    equation(app, ctx);
}

/// Format > Add Link: Webpage / Email / Phone Number / Bookmark for the selected text (or object).
fn link_window(app: &mut App, ctx: &egui::Context) {
    let Some((kind, mut text)) = app.link_edit.clone() else { return };
    let mut done: Option<bool> = None;
    let bookmarks = app.wb.as_ref().map(|w| w.bookmarks()).unwrap_or_default();
    centered(ctx, &format!("Link to {kind}")).show(ctx, |ui| {
        if kind == "Bookmark" {
            if bookmarks.is_empty() {
                ui.label("There are no bookmarks in this document.");
            }
            for (_, t, a, _) in &bookmarks {
                if ui.selectable_label(text == a.to_string(), t.trim()).clicked() {
                    text = a.to_string();
                }
            }
        } else {
            let hint = match kind.as_str() {
                "Email" => "name@example.com",
                "Phone Number" => "+1 555 123 4567",
                _ => "www.example.com",
            };
            let r = ui.add(egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(300.0));
            r.request_focus();
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !text.trim().is_empty() {
                done = Some(true);
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            done = Some(false);
        }
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                done = Some(false);
            }
            if ui.add_enabled(!text.trim().is_empty(), egui::Button::new("Done")).clicked() {
                done = Some(true);
            }
        });
    });
    match done {
        Some(true) => {
            let t = text.trim();
            let url = match kind.as_str() {
                "Email" => format!("mailto:{}", t.trim_start_matches("mailto:")),
                "Phone Number" => format!("tel:{}", t.trim_start_matches("tel:").replace(' ', "")),
                "Bookmark" => format!("#bookmark-{t}"),
                _ if t.contains("://") => t.to_string(),
                _ => format!("https://{t}"),
            };
            app.link_edit = None;
            match app.cursor.filter(|c| !c.is_empty()) {
                Some(c) => {
                    let (a, b) = c.range();
                    app.edit_pages("Add Link", |p, h| p.add_link(h, c.storage, a, b, &url));
                }
                None => {
                    let ids = app.selected_objs();
                    app.edit("Add Link", |d, h| {
                        for id in &ids {
                            d.set_hyperlink(h, *id, Some(&url));
                        }
                    });
                }
            }
        }
        Some(false) => app.link_edit = None,
        None => app.link_edit = Some((kind, text)),
    }
}

/// Insert > Comment: write it (a new comment over the selection, or an existing one).
fn comment_window(app: &mut App, ctx: &egui::Context) {
    let Some((hl, mut text)) = app.comment_edit.clone() else { return };
    let mut done = None;
    centered(ctx, if hl.is_some() { "Edit Comment" } else { "Comment" }).show(ctx, |ui| {
        ui.label(RichText::new(crate::text::author(app)).strong());
        let r = ui.add(egui::TextEdit::multiline(&mut text).desired_rows(4).desired_width(300.0).hint_text("Comment"));
        r.request_focus();
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
            if ui.add_enabled(!text.trim().is_empty(), egui::Button::new("Done")).clicked() {
                done = Some(true);
            }
        });
    });
    match done {
        Some(true) => {
            app.comment_edit = None;
            let who = crate::text::author(app);
            match hl {
                Some(h) => {
                    app.edit_pages("Edit Comment", |p, hi| p.set_text_comment(hi, h, text.trim()));
                }
                None => {
                    if let Some(c) = app.cursor {
                        let (mut a, mut b) = c.range();
                        if a == b {
                            // the word at the caret
                            let t = app.doc().map(|d| d.storage_text(c.storage)).unwrap_or_default();
                            (a, b) = crate::canvas::word_at(&t, a);
                        }
                        let st = c.storage;
                        let t = text.trim().to_string();
                        let res = app.edit_pages("Add Comment", |p, h| p.add_text_comment(h, st, a, b, &t, &who));
                        if res.flatten().is_none() {
                            app.notify("Select some text to comment on.");
                        }
                        app.show_comments = true;
                    }
                }
            }
        }
        Some(false) => app.comment_edit = None,
        None => app.comment_edit = Some((hl, text)),
    }
}

/// View > Show Colors: the suite's Colors window; "Apply" colours the selected text or objects.
fn colors_window(app: &mut App, ctx: &egui::Context) {
    if app.color_state.saved.is_empty() && !app.settings.saved_colors.is_empty() {
        app.color_state.saved_from_string(&app.settings.saved_colors.clone());
    }
    let before = app.color_state.saved.clone();
    kn_ui::colors::colors_window(ctx, &mut app.color_state);
    if app.show_colors {
        let mut open = true;
        let mut apply = false;
        let mut c = app.colors_pick;
        let any = app.cursor.is_some_and(|c| !c.is_empty()) || app.canvas.obj.is_some();
        kn_ui::colors::colors_window_with(ctx, "view-colors", &mut app.color_state, &mut c, &mut open, |ui| {
            if ui.add_enabled(any, egui::Button::new("Apply to Selection").min_size(vec2(232.0, 26.0))).clicked() {
                apply = true;
            }
        });
        app.colors_pick = c;
        app.show_colors = open;
        if apply {
            let col = crate::objinsp::from_c32(c);
            if app.cursor.is_some_and(|c| !c.is_empty()) {
                crate::textinsp::apply_char(app, "Text Color", crate::text::CharChange::Color(col));
            } else {
                let ids = app.selected_objs();
                let spec = pg_core::kn_model::format::FillSpec::Color(col);
                app.edit("Fill", |d, h| {
                    for i in &ids {
                        d.set_fill(h, *i, &spec);
                    }
                });
            }
        }
    }
    if app.color_state.saved != before {
        app.settings.saved_colors = app.color_state.saved_to_string();
        app.settings.save();
    }
}

/// A recent document's first page as PNG (Word, RTF and text files hold no preview picture).
fn first_page_png(p: &std::path::Path) -> Option<Vec<u8>> {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let doc = match ext.as_str() {
        "docx" => pg_core::wordmode::open_docx(p).ok().map(|o| o.pages),
        "txt" | "md" | "markdown" | "rtf" | "html" | "htm" => pg_core::import::import_file(p).ok(),
        _ => pg_core::Pages::open(p).ok(),
    }?;
    let pm = pg_core::export::first_page(&doc, 0.6)?;
    pm.encode_png().ok()
}

/// File > New (and a new window): the start screen with the recent documents and Pages' Basic
/// templates, as Keydeck's.
fn chooser(app: &mut App, ctx: &egui::Context) {
    if !app.chooser {
        return;
    }
    const ITEMS: [&str; 6] = ["Blank", "Blank Landscape", "Blank A4", "Note Taking", "Blank Layout", "Essay"];
    let templates: Vec<kn_ui::welcome::Template> = ITEMS.iter().map(|n| kn_ui::welcome::Template { name: n.to_string(), preview: None }).collect();
    let recent = app.recent();
    // a hint of each template's contents on its page
    let draw = |p: &egui::Painter, tile: egui::Rect, i: usize| {
        let page = if i == 1 { egui::Rect::from_center_size(tile.center(), vec2(tile.width() - 16.0, (tile.width() - 16.0) * 0.77)) } else { tile };
        if i == 1 {
            p.rect_filled(tile, 6.0, theme::gray(0x30));
            p.rect_filled(page, 2.0, Color32::WHITE);
        }
        let k = page.width() / 110.0;
        let line = |y: f32, w: f32, t: f32| p.rect_filled(egui::Rect::from_min_size(page.min + vec2(12.0 * k, y * k), vec2(w * k, t * k)), 0.0, Color32::from_gray(0xb0));
        match i {
            3 => {
                line(14.0, 60.0, 4.0);
                for n in 0..9 {
                    p.hline(page.x_range().shrink(8.0 * k), page.min.y + (30.0 + n as f32 * 12.0) * k, egui::Stroke::new(0.6, Color32::from_rgb(0x9c, 0xc2, 0xf0)));
                }
            }
            4 => {
                p.rect_stroke(page.shrink(14.0 * k), 0.0, egui::Stroke::new(0.8, Color32::from_gray(0xc8)), egui::StrokeKind::Middle);
            }
            5 => {
                line(16.0, 70.0, 5.0);
                line(30.0, 40.0, 3.0);
                for n in 0..6 {
                    line(44.0 + n as f32 * 8.0, 84.0, 2.0);
                }
            }
            _ => {}
        }
    };
    let spec = kn_ui::welcome::Spec {
        app: "Letters",
        id: "letters",
        heading: "Choose a Template",
        section: "Templates",
        all: "All Templates",
        hint: "or drop a .pages or .docx file here",
        recent: &recent,
        templates: &templates,
        default: "Blank",
        tile: vec2(124.0, 160.0),
        draw: &draw,
        cancel: true,
        loading: false,
        render: Some(first_page_png),
    };
    match kn_ui::welcome::overlay(ctx, app.welcome_top, &spec) {
        Some(kn_ui::welcome::Action::Create(i)) => {
            app.chooser = false;
            crate::menus::new_from_template(app, i);
        }
        Some(kn_ui::welcome::Action::Open(p)) => {
            app.chooser = false;
            app.open(&p);
        }
        Some(kn_ui::welcome::Action::Browse) => {
            app.chooser = false;
            app.open_dialog();
        }
        Some(kn_ui::welcome::Action::Cancel) => app.chooser = false,
        None => {}
    }
}

/// Format > Advanced > Create Style: a new paragraph style from the caret's paragraph.
fn new_style(app: &mut App, ctx: &egui::Context) {
    let Some(mut name) = app.new_style.clone() else { return };
    let mut done = None;
    centered(ctx, "Create Paragraph Style").show(ctx, |ui| {
        ui.label("Name:");
        let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(240.0));
        r.request_focus();
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
            if ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Create")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
        });
    });
    match done {
        Some(true) => {
            app.new_style = None;
            let Some(c) = app.cursor else { return };
            let (a, b) = c.range();
            let st = c.storage;
            let n = name.trim().to_string();
            app.edit("Create Style", |d, h| {
                let Some(cur) = pg_core::edit::para_style_at(d, st, a) else { return };
                let sheet = d.file.stylesheet_id();
                let touch: Vec<_> = sheet.into_iter().collect();
                let Ok(v) = d.structural(h, &touch, |k| k.new_style_variation(cur)) else { return };
                d.touch(h, v);
                if let Some(s) = d.file.obj_mut(v).and_then(|m| m.find_super_mut("TSS.StyleArchive")) {
                    s.set_str("name", &n);
                    s.set_bool("is_variation", false);
                }
                pg_core::edit::set_para_style(d, h, st, a, b, v);
            });
        }
        Some(false) => app.new_style = None,
        None => app.new_style = Some(name),
    }
}

/// Insert > Equation…: LaTeX typed here goes in as an inline picture that keeps its source
/// (equation_source_text, as Keynote stores it), typeset by the system's TeX when there is one
/// (Keydeck's way) and otherwise drawn by Letters from the readable form (x², a⁄b, α).
fn equation(app: &mut App, ctx: &egui::Context) {
    let Some(mut src) = app.equation.clone() else { return };
    let mut done = None;
    centered(ctx, "Equation").show(ctx, |ui| {
        ui.label("Enter a LaTeX equation:");
        let r = ui.add(egui::TextEdit::multiline(&mut src).desired_rows(3).desired_width(340.0).font(egui::TextStyle::Monospace).hint_text("x^2 + y^2 = z^2"));
        r.request_focus();
        ui.label(RichText::new(latex_to_text(&src)).size(18.0));
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
            if ui.add_enabled(!src.trim().is_empty(), egui::Button::new("Insert")).clicked() {
                done = Some(true);
            }
        });
    });
    match done {
        Some(true) => {
            app.equation = None;
            let src = src.trim().to_string();
            // editing an equation: the new one goes where the old one is
            let target = app.equation_target.take();
            let target_at = target.and_then(|t| app.wb.as_ref().and_then(|w| w.inline_attachment(t)).map(|(_, b)| (t, b)));
            let body = app.wb.as_ref().and_then(|w| w.body_storage());
            let cursor = match (target_at, body) {
                (Some((_, b)), Some(st)) => Some(crate::app::Cursor { storage: st, owner: Owner::Body, anchor: b, caret: b, goal_x: None }),
                _ => app.cursor.filter(|c| c.owner == Owner::Body),
            };
            match cursor {
                Some(c) => {
                    let (png, w, h) = match typeset(&src) {
                        Some(t) => t,
                        None => match draw_equation(app, &latex_to_text(&src)) {
                            Some(t) => t,
                            None => return app.notify("Couldn't draw the equation"),
                        },
                    };
                    let at = c.range().0;
                    let res = app.edit_pages("Insert Equation", |p, hh| -> Result<pg_core::kn_format::ObjId, pg_core::kn_format::Error> {
                        if let Some((old, _)) = target_at {
                            p.delete_objects(hh, &[old]);
                        }
                        let id = p.insert_image(hh, png, "equation.png", pg_core::objects::Placement::Inline { at }, f32::MAX)?;
                        let g = p.file().obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(pg_core::kn_model::scene::geometry).unwrap_or_default();
                        p.doc.set_frame(hh, id, pg_core::kn_model::Geometry { w, h, ..g });
                        p.doc.set_equation_source(hh, id, &src);
                        Ok(id)
                    });
                    match res {
                        Some(Ok(_)) => {
                            // the caret after the equation
                            let after = at + '\u{FFFC}'.len_utf8();
                            app.cursor = Some(crate::app::Cursor { anchor: after, caret: after, goal_x: None, ..c });
                        }
                        Some(Err(e)) => app.notify(format!("Couldn't insert the equation: {e}")),
                        None => {}
                    }
                }
                None => app.notify("Click in the text where the equation should go."),
            }
        }
        Some(false) => {
            app.equation = None;
            app.equation_target = None;
        }
        None => app.equation = Some(src),
    }
}

/// The formula typeset by the system's pdflatex (600 dpi PNG) and its size in points; None
/// when there's no TeX or it can't typeset this.
fn typeset(latex: &str) -> Option<(Vec<u8>, f32, f32)> {
    let dir = std::env::temp_dir().join(format!("cs-eq-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let doc = format!("\\documentclass[border=2pt]{{standalone}}\n\\usepackage{{amsmath,amssymb}}\n\\begin{{document}}\n$\\displaystyle {latex}$\n\\end{{document}}\n");
    std::fs::write(dir.join("eq.tex"), doc).ok()?;
    let ok = std::process::Command::new("pdflatex")
        .args(["-interaction=nonstopmode", "-halt-on-error", "eq.tex"])
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .is_some_and(|o| o.status.success())
        && std::process::Command::new("pdftoppm").args(["-png", "-r", "600", "-singlefile", "eq.pdf", "eq"]).current_dir(&dir).status().is_ok_and(|s| s.success());
    let png = ok.then(|| std::fs::read(dir.join("eq.png")).ok()).flatten();
    let _ = std::fs::remove_dir_all(&dir);
    let png = png?;
    let (w, h) = pg_core::kn_format::objects::image_size(&png)?;
    Some((png, w as f32 * 72.0 / 600.0, h as f32 * 72.0 / 600.0))
}

/// The readable form of a formula drawn in a math face at 4× (PNG) and its size in points.
fn draw_equation(app: &mut App, text: &str) -> Option<(Vec<u8>, f32, f32)> {
    use pg_core::kn_model::{Align, CharStyle, VAlign};
    let style = CharStyle { size: 13.0, italic: true, font_name: "TimesNewRomanPS-ItalicMT".into(), ..Default::default() };
    let tb = kn_render::simple_text_box(text, style, Align::Left, VAlign::Top);
    let fonts = &mut app.canvas.tiles.renderer.fonts;
    let tl = kn_render::text::layout(fonts, &tb, 2000.0, 200.0, true, true);
    let (w, h) = ((tl.content_w + 4.0).ceil(), (tl.content_h + 4.0).ceil());
    let k = 4.0;
    let mut pm = tiny_skia::Pixmap::new((w * k) as u32, (h * k) as u32)?;
    app.canvas.tiles.renderer.draw_text(&tl, &mut pm, tiny_skia::Transform::from_scale(k, k).pre_translate(2.0, 2.0));
    let img = image::RgbaImage::from_raw(pm.width(), pm.height(), pm.data().to_vec())?;
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some((out.into_inner(), w, h))
}

/// A readable rendering of simple LaTeX: x^2 -> x², \alpha -> α, \frac{a}{b} -> a⁄b.
pub fn latex_to_text(s: &str) -> String {
    let sup = |c: char| match c {
        '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴', '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹', '+' => '⁺', '-' => '⁻', 'n' => 'ⁿ', 'i' => 'ⁱ', _ => c,
    };
    let sub = |c: char| match c {
        '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄', '5' => '₅', '6' => '₆', '7' => '₇', '8' => '₈', '9' => '₉', '+' => '₊', '-' => '₋', _ => c,
    };
    let names = [
        ("\\alpha", "α"), ("\\beta", "β"), ("\\gamma", "γ"), ("\\delta", "δ"), ("\\epsilon", "ε"), ("\\theta", "θ"), ("\\lambda", "λ"), ("\\mu", "μ"), ("\\pi", "π"), ("\\sigma", "σ"), ("\\phi", "φ"), ("\\omega", "ω"),
        ("\\Delta", "Δ"), ("\\Sigma", "Σ"), ("\\Omega", "Ω"), ("\\infty", "∞"), ("\\pm", "±"), ("\\times", "×"), ("\\cdot", "·"), ("\\leq", "≤"), ("\\geq", "≥"), ("\\neq", "≠"), ("\\approx", "≈"),
        ("\\sum", "∑"), ("\\int", "∫"), ("\\sqrt", "√"), ("\\rightarrow", "→"), ("\\leftarrow", "←"), ("\\partial", "∂"), ("\\in", "∈"),
    ];
    let mut t = s.to_string();
    for (a, b) in names {
        t = t.replace(a, b);
    }
    // \frac{a}{b}
    while let Some(i) = t.find("\\frac{") {
        let rest = &t[i + 6..];
        let Some(e1) = rest.find('}') else { break };
        let num = rest[..e1].to_string();
        let rest2 = &rest[e1 + 1..];
        let (den, used) = if rest2.starts_with('{') { rest2[1..].find('}').map(|e| (rest2[1..1 + e].to_string(), e + 2)).unwrap_or((String::new(), 0)) } else { (String::new(), 0) };
        t = format!("{}{}⁄{}{}", &t[..i], num, den, &rest2[used..]);
    }
    let mut out = String::new();
    let cs: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if (c == '^' || c == '_') && i + 1 < cs.len() {
            let map: &dyn Fn(char) -> char = if c == '^' { &sup } else { &sub };
            if cs[i + 1] == '{' {
                let mut j = i + 2;
                while j < cs.len() && cs[j] != '}' {
                    out.push(map(cs[j]));
                    j += 1;
                }
                i = j + 1;
            } else {
                out.push(map(cs[i + 1]));
                i += 2;
            }
            continue;
        }
        if c != '{' && c != '}' && c != '\\' {
            out.push(c);
        }
        i += 1;
    }
    out
}

#[allow(dead_code)]
fn unused(_o: Owner) {}
