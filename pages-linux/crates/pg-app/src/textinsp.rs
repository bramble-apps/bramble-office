//! Format > Text for the caret's text (body, text box, header, footnote), laid out like Pages
//! 15.4 (research screenshots P-insp-body-caret-Text-*): the paragraph style button (with "*"
//! and Update when the text is overridden), Style | Layout | More.
//!   Style: font family, typeface, size, B I U S, ⚙ advanced (spacing, baseline,
//!          capitalisation, ligatures, outline, background), character styles, text colour,
//!          alignment, indent level, Spacing (lines / at least / exactly / between, before /
//!          after), Bullets & Lists, Drop Cap.
//!   Layout: Columns (body), Indents (first / left / right), Tabs (default stops), Paragraph
//!          Borders (top / bottom / left / right, colour, width), Paragraph Background.
//!   More:  Pagination & Breaks, Hyphenation & Ligatures, Following Paragraph Style.
//! With nothing selected it applies to what is typed next (Pages' typing attributes).

use egui::{Color32, RichText};
use kn_ui::theme;
use pg_core::kn_format::{Msg, ObjId};
use pg_core::kn_model::{self, Align, CharStyle, LineSpacing};

use crate::app::{App, Owner};
use crate::objinsp::{color_button, section};
use crate::text::CharChange;

/// The text the inspector works on: storage, stored range, caret.
#[derive(Clone, Copy)]
pub struct Target {
    pub storage: ObjId,
    pub a: usize,
    pub b: usize,
    pub owner: Owner,
}

pub fn target(app: &App) -> Option<Target> {
    if let Some(c) = app.cursor {
        let (a, b) = c.range();
        return Some(Target { storage: c.storage, a, b, owner: c.owner });
    }
    // a selected text box / shape: all its text
    let id = app.canvas.obj?;
    let d = app.drawables.iter().find(|d| d.id == id)?;
    let tb = crate::text::drawable_text(d)?;
    let len = app.doc()?.storage_text(tb.storage).len();
    Some(Target { storage: tb.storage, a: 0, b: len, owner: Owner::Drawable(id) })
}

/// The character look at the target (the first character of a selection, or what typing gets).
fn char_style(app: &App, t: &Target) -> CharStyle {
    let Some(d) = app.doc() else { return CharStyle::default() };
    let ps = pg_core::edit::para_style_at(d, t.storage, t.a);
    let at = if t.a < t.b { t.a + 1 } else { t.a };
    let cs = pg_core::edit::char_style_at(d, t.storage, at);
    let mut s = kn_model::style::char_style(&d.file, ps, cs);
    if t.a == t.b {
        for p in &app.typing_props {
            match p {
                CharChange::Bold(b) => s.bold = *b,
                CharChange::Italic(b) => s.italic = *b,
                CharChange::Underline(b) => s.underline = *b,
                CharChange::Strike(b) => s.strike = *b,
                CharChange::Size(v) => s.size = *v,
                CharChange::Font(f) => s.font_name = f.clone(),
                CharChange::Color(c) => s.color = *c,
                CharChange::Script(v) => s.script = *v,
            }
        }
    }
    s
}

/// The paragraph properties chain of the paragraph at the target.
fn para_chain(app: &App, t: &Target) -> Vec<Msg> {
    let Some(d) = app.doc() else { return Vec::new() };
    let ps = pg_core::edit::para_style_at(d, t.storage, t.a);
    kn_model::style::chain(&d.file, ps).into_iter().filter_map(|m| m.msg("para_properties").cloned()).collect()
}

fn lookup<T>(chain: &[Msg], f: impl Fn(&Msg) -> Option<T>) -> Option<T> {
    chain.iter().find_map(f)
}

/// Apply a character change to the target (or to what is typed next).
pub fn apply_char(app: &mut App, label: &str, change: CharChange) {
    let Some(t) = target(app) else { return };
    if t.a == t.b {
        app.typing_props.retain(|p| std::mem::discriminant(p) != std::mem::discriminant(&change));
        app.typing_props.push(change);
        return;
    }
    let st = t.storage;
    let typing = app.cursor.is_some();
    app.edit(label, |d, h| {
        pg_core::edit::set_char_props(d, h, st, t.a, t.b, &|cp| change.apply(cp));
    });
    if !typing {
        app.cursor = None;
    }
}

/// Apply a paragraph property change to the paragraphs the target touches.
pub fn apply_para(app: &mut App, label: &str, f: impl Fn(&mut Msg) + 'static) {
    let Some(t) = target(app) else { return };
    let st = t.storage;
    app.edit(label, |d, h| {
        pg_core::edit::set_para_props(d, h, st, t.a, t.b, &f);
    });
}

fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

/// The paragraph style button and its popover.
fn style_button(app: &mut App, ui: &mut egui::Ui, t: &Target) {
    let (named, name, over) = app.doc().map(|d| pg_core::edit::named_style(d, pg_core::edit::para_style_at(d, t.storage, t.a))).unwrap_or((None, "Body".into(), false));
    let cur_style = app.doc().and_then(|d| pg_core::edit::para_style_at(d, t.storage, t.a));
    let shown = if over { format!("{name}*") } else { name.clone() };
    let fr = egui::Frame::new().fill(crate::look::style_button_colors().0).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(&shown).size(17.0).color(crate::look::style_button_colors().1));
    });
    let r = fr.response.rect;
    kn_ui::icons::draw(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.max.x - 22.0, r.center().y), egui::vec2(16.0, 16.0)), kn_ui::icons::Icon::ChevronDown, crate::look::style_button_colors().1);
    let pr = ui.interact(r, ui.scope_id().with("para-style"), egui::Sense::click());
    let mut pick = None;
    let mut update = false;
    let mut create = false;
    egui::Popup::menu(&pr).width(r.width()).show(|ui| {
        ui.horizontal(|ui| {
            ui.label(theme::bold("Paragraph Styles"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+").on_hover_text("Create a style from this text").clicked() {
                    create = true;
                    ui.close();
                }
            });
        });
        ui.separator();
        let styles = app.doc().map(|d| d.paragraph_styles()).unwrap_or_default();
        // in the stylesheet's order of importance (Title, Subtitle, Heading…, Body…)
        let order = ["Title", "Subtitle", "Heading", "Heading 2", "Heading 3", "Heading Red", "Body", "Caption", "Header & Footer", "Footnote", "Label", "Label Dark", "Quote", "Attribution"];
        let mut styles: Vec<(ObjId, String)> = styles;
        styles.sort_by_key(|(_, n)| order.iter().position(|o| o == n).unwrap_or(100));
        egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
            for (id, sname) in styles {
                let cs = app.doc().map(|d| kn_model::style::char_style(&d.file, Some(id), None)).unwrap_or_default();
                let mut txt = RichText::new(&sname).size(cs.size.clamp(10.0, 24.0));
                if cs.bold {
                    txt = txt.strong();
                }
                if cs.italic {
                    txt = txt.italics();
                }
                ui.horizontal(|ui| {
                    // Pages: a check mark beside the current style
                    let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    if Some(id) == named {
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "✓", egui::FontId::proportional(13.0), theme::TEXT());
                    }
                    if ui.add(egui::Button::selectable(false, (txt, egui::Atom::grow())).min_size(egui::vec2(ui.available_width() - if Some(id) == named && over { 60.0 } else { 0.0 }, 0.0))).clicked() {
                        pick = Some(id);
                        ui.close();
                    }
                    if Some(id) == named && over && ui.small_button("Update").clicked() {
                        update = true;
                        ui.close();
                    }
                });
            }
        });
    });
    if over {
        // Pages: two regular buttons side by side under the style button
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let gap = ui.spacing().item_spacing.x;
            let w = ((ui.available_width() - gap) / 2.0).max(40.0);
            let button = |ui: &mut egui::Ui, label: &str, tip: &str| {
                ui.add(egui::Button::new(RichText::new(label).size(13.0)).min_size(egui::vec2(w, 26.0)).corner_radius(7).fill(theme::gray(0x38))).on_hover_text(tip).clicked()
            };
            if button(ui, "Update", "Change the style to match this text") {
                update = true;
            }
            if button(ui, "Revert", "Remove this text's style overrides") {
                pick = named;
            }
        });
    }
    let st = t.storage;
    if let Some(id) = pick {
        app.edit("Paragraph Style", |d, h| {
            pg_core::edit::set_para_style(d, h, st, t.a, t.b, id);
        });
    }
    if update {
        if let Some(v) = cur_style {
            app.edit("Update Style", |d, h| {
                pg_core::edit::update_named_style(d, h, v);
            });
        }
    }
    if create {
        app.new_style = Some(format!("{name} 2"));
    }
}

pub fn text_panel(app: &mut App, ui: &mut egui::Ui) {
    let Some(t) = target(app) else {
        nothing_selected(ui);
        return;
    };
    style_button(app, ui, &t);
    ui.add_space(8.0);
    let mut sub = app.text_tab.min(2);
    crate::objinsp::seg_small(ui, &["Style", "Layout", "More"], &mut sub);
    app.text_tab = sub;
    ui.add_space(4.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match sub {
        1 => layout_tab(app, ui, &t),
        2 => more_tab(app, ui, &t),
        _ => style_tab(app, ui, &t),
    });
}

pub fn nothing_selected(ui: &mut egui::Ui) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        kn_ui::icons::draw(ui.painter(), egui::Rect::from_center_size(ui.cursor().min + egui::vec2(ui.available_width() / 2.0, 20.0), egui::vec2(34.0, 34.0)), kn_ui::icons::Icon::Format, theme::TEXT_DIM());
        ui.add_space(50.0);
        ui.label(RichText::new("Nothing selected.").size(15.0).color(theme::TEXT_DIM()));
        ui.label(RichText::new("Select an object or text to format.").color(theme::TEXT_DIM()));
    });
}

fn style_tab(app: &mut App, ui: &mut egui::Ui, t: &Target) {
    let cs = char_style(app, t);
    let chain = para_chain(app, t);
    section(ui, "Font");
    let family = kn_render::fonts::parse_ps_name(&cs.font_name).0;
    if app.font_names.is_none() {
        app.font_names = Some(crate::editing::font_names(app));
    }
    let fonts = app.font_names.clone().unwrap_or_default();
    let mut changes: Vec<(&str, CharChange)> = Vec::new();
    theme::combo("font").selected_text(&family).width(ui.available_width()).height(400.0).show_ui(ui, |ui| {
        for f in &fonts {
            if ui.selectable_label(*f == family, f).clicked() && *f != family {
                let ps = app.canvas.tiles.ps_name_for_family(f).unwrap_or_else(|| f.replace(' ', ""));
                changes.push(("Font", CharChange::Font(ps)));
            }
        }
    });
    ui.horizontal(|ui| {
        let typeface = match (cs.bold, cs.italic) {
            (true, true) => "Bold Italic",
            (true, false) => "Bold",
            (false, true) => "Italic",
            _ => "Regular",
        };
        theme::combo("typeface").selected_text(typeface).width(ui.available_width() - 100.0).show_ui(ui, |ui| {
            for (n, b, i) in [("Regular", false, false), ("Italic", false, true), ("Bold", true, false), ("Bold Italic", true, true)] {
                if ui.selectable_label(n == typeface, n).clicked() && n != typeface {
                    if b != cs.bold {
                        changes.push(("Bold", CharChange::Bold(b)));
                    }
                    if i != cs.italic {
                        changes.push(("Italic", CharChange::Italic(i)));
                    }
                }
            }
        });
        let mut size = cs.size as f64;
        if theme::stepper(ui, &mut size, 1.0, 1.0..=1638.0, " pt", 0) {
            changes.push(("Font Size", CharChange::Size(size.round() as f32)));
        }
    });
    // B I U S
    ui.horizontal(|ui| {
        let w = ui.available_width();
        ui.allocate_ui(egui::vec2(w - 110.0, 30.0), |ui| {
            let states = [cs.bold, cs.italic, cs.underline, cs.strike];
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
                        0 => ("Bold", CharChange::Bold(!on)),
                        1 => ("Italic", CharChange::Italic(!on)),
                        2 => ("Underline", CharChange::Underline(!on)),
                        _ => ("Strikethrough", CharChange::Strike(!on)),
                    });
                }
            }
        });
        let gear = ui.add_sized([96.0, 30.0], egui::Button::new("⚙ ⌄"));
        egui::Popup::menu(&gear).show(|ui| {
            ui.set_min_width(240.0);
            let mut tracking = cs.tracking * 100.0;
            row(ui, "Character Spacing", |ui| {
                if theme::stepper32(ui, &mut tracking, 1.0, -50.0..=200.0, " %", 0) {
                    let v = tracking.round() / 100.0;
                    let t2 = *t;
                    let st = t2.storage;
                    app.edit("Character Spacing", |d, h| {
                        pg_core::edit::set_char_props(d, h, st, t2.a, t2.b.max(t2.a + 1), &|cp| cp.set_f32("tracking", v));
                    });
                }
            });
            let mut shift = cs.baseline_shift;
            row(ui, "Baseline Shift", |ui| {
                if theme::stepper32(ui, &mut shift, 1.0, -100.0..=100.0, " pt", 0) {
                    let t2 = *t;
                    let st = t2.storage;
                    app.edit("Baseline Shift", |d, h| {
                        pg_core::edit::set_char_props(d, h, st, t2.a, t2.b.max(t2.a + 1), &|cp| cp.set_f32("baseline_shift", shift));
                    });
                }
            });
            ui.separator();
            ui.label(RichText::new("Baseline").color(theme::TEXT_DIM()));
            for (v, n) in [(0i8, "Default"), (1, "Superscript"), (-1, "Subscript")] {
                if ui.radio(cs.script == v, n).clicked() {
                    changes.push(("Baseline", CharChange::Script(v)));
                }
            }
            ui.separator();
            ui.label(RichText::new("Capitalization").color(theme::TEXT_DIM()));
            for (c, n) in [(kn_model::Caps::None, "None"), (kn_model::Caps::All, "All Caps"), (kn_model::Caps::Small, "Small Caps"), (kn_model::Caps::Title, "Title Case"), (kn_model::Caps::Start, "Start Case")] {
                if ui.radio(cs.caps == c, n).clicked() {
                    let t2 = *t;
                    let st = t2.storage;
                    app.edit("Capitalization", |d, h| {
                        pg_core::edit::set_char_props(d, h, st, t2.a, t2.b.max(t2.a + 1), &|cp| crate::text::set_caps(cp, n));
                    });
                }
            }
            ui.separator();
            row(ui, "Text Background", |ui| {
                if let Some(c) = color_button(ui, cs.background.unwrap_or(kn_model::Color::TRANSPARENT)) {
                    let t2 = *t;
                    let st = t2.storage;
                    app.edit("Text Background", |d, h| {
                        pg_core::edit::set_char_props(d, h, st, t2.a, t2.b.max(t2.a + 1), &|cp| {
                            cp.remove("background_color");
                            if c.a > 0.0 {
                                cp.push_msg("background_color", kn_model::format::color_msg(c));
                            }
                        });
                    });
                }
            });
            if ui.button("No Background").clicked() {
                let t2 = *t;
                let st = t2.storage;
                app.edit("Text Background", |d, h| {
                    pg_core::edit::set_char_props(d, h, st, t2.a, t2.b.max(t2.a + 1), &|cp| cp.remove("background_color"));
                });
            }
        });
    });
    // character styles
    ui.add_space(6.0);
    row(ui, "Character Styles", |ui| {
        let styles = app.doc().map(|d| d.character_styles()).unwrap_or_default();
        let cur = app.doc().and_then(|d| pg_core::edit::char_style_at(d, t.storage, if t.a < t.b { t.a + 1 } else { t.a })).and_then(|id| app.doc().map(|d| pg_core::edit::named_style(d, Some(id)).1)).filter(|n| !n.is_empty()).unwrap_or_else(|| "None".into());
        let mut pick: Option<Option<ObjId>> = None;
        theme::combo("charstyles").selected_text(&cur).width(150.0).show_ui(ui, |ui| {
            if ui.selectable_label(cur == "None", "None").clicked() {
                pick = Some(None);
            }
            // the stylesheet's own "None" style is the entry above
            for (id, name) in styles.iter().filter(|(_, n)| n != "None") {
                if ui.selectable_label(*name == cur, name).clicked() {
                    pick = Some(Some(*id));
                }
            }
        });
        if let Some(style) = pick {
            let (st, a, b) = (t.storage, t.a, t.b);
            if a < b {
                app.edit("Character Style", |d, h| {
                    d.set_character_style(h, st, a, b, style);
                });
            }
        }
    });
    ui.separator();
    row(ui, "Text Color", |ui| {
        if let Some(c) = color_button(ui, cs.color) {
            changes.push(("Text Color", CharChange::Color(c)));
        }
    });
    ui.separator();
    // alignment
    let align = lookup(&chain, |p| p.enum_name("alignment")).unwrap_or("TATvalue0");
    let cur = match align {
        "TATvalue2" => 1,
        "TATvalue1" => 2,
        "TATvalue3" => 3,
        _ => 0,
    };
    if let Some(i) = crate::objinsp::icon_segments(ui, "align", 4, Some(cur), |pt, r, i, col| {
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
    }) {
        if i != cur {
            let v = ["TATvalue0", "TATvalue2", "TATvalue1", "TATvalue3"][i];
            apply_para(app, "Alignment", move |pp| pp.set_enum("alignment", v));
        }
    }
    ui.add_space(4.0);
    if let Some(i) = crate::objinsp::icon_segments(ui, "indent", 2, None, |pt, r, i, col| {
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
        let st = t.storage;
        let delta = if i == 0 { -1 } else { 1 };
        let in_list = app.layout.as_ref().and_then(|l| l.body.as_ref()).is_some_and(|b| b.tb.paragraphs.get(b.para_at_stored(t.a)).is_some_and(|p| p.label.is_some()));
        if in_list || t.owner != Owner::Body {
            app.edit("Indent", |d, h| pg_core::edit::change_level(d, h, st, t.a, t.b, delta));
        } else {
            // plain paragraphs: the left indent moves by a tab stop (Pages' Increase Indent)
            let left = lookup(&chain, |p| p.f32("left_indent")).unwrap_or(0.0);
            let v = (left + delta as f32 * 36.0).max(0.0);
            apply_para(app, "Indent", move |pp| pp.set_f32("left_indent", v));
        }
    }
    ui.add_space(4.0);
    // Spacing
    if theme::disclosure(ui, "spacing", "Spacing", None) {
        let ls = lookup(&chain, |p| p.msg("line_spacing").filter(|m| !m.fields.is_empty()).cloned());
        let mode = ls.as_ref().and_then(|m| m.enum_name("mode")).unwrap_or("kRelativeLineSpacing");
        let amount = ls.as_ref().and_then(|m| m.f32("amount")).unwrap_or(1.0);
        let modes = [("Lines", "kRelativeLineSpacing"), ("At Least", "kMinimumLineSpacing"), ("Exactly", "kExactLineSpacing"), ("Between", "kSpaceBetweenLineSpacing")];
        let cur_name = modes.iter().find(|m| m.1 == mode).map(|m| m.0).unwrap_or("Lines");
        let mut set: Option<(String, f32)> = None;
        ui.horizontal(|ui| {
            theme::combo("linespacing-mode").selected_text(cur_name).width(120.0).show_ui(ui, |ui| {
                for (n, m) in modes {
                    if ui.selectable_label(m == mode, n).clicked() && m != mode {
                        let v = if m == "kRelativeLineSpacing" { 1.0 } else { cs.size * 1.2 };
                        set = Some((m.to_string(), v));
                    }
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let relative = mode == "kRelativeLineSpacing";
                let mut v = amount as f64;
                if theme::stepper(ui, &mut v, if relative { 0.1 } else { 1.0 }, if relative { 0.1..=10.0 } else { 0.0..=500.0 }, if relative { "" } else { " pt" }, if relative { 1 } else { 0 }) {
                    set = Some((mode.to_string(), v as f32));
                }
            });
        });
        if let Some((m, v)) = set {
            apply_para(app, "Line Spacing", move |pp| {
                pp.remove("line_spacing");
                let ls = pp.msg_mut_or_create("line_spacing");
                ls.set_enum("mode", &m);
                ls.set_f32("amount", v);
            });
        }
        for (label, field) in [("Before Paragraph", "space_before"), ("After Paragraph", "space_after")] {
            let cur = lookup(&chain, |p| p.f32(field)).unwrap_or(0.0);
            row(ui, label, |ui| {
                let mut v = cur as f64;
                if theme::stepper(ui, &mut v, 1.0, 0.0..=1000.0, " pt", 0) {
                    let v = v.round() as f32;
                    apply_para(app, label, move |pp| pp.set_f32(field, v));
                }
            });
        }
    }
    // Bullets & Lists and Drop Cap (Pages' popovers and details)
    ui.separator();
    crate::listcap::bullets_section(app, ui, t);
    ui.separator();
    crate::listcap::drop_cap_section(app, ui, t);
    for (label, c) in changes {
        apply_char(app, label, c);
    }
}

fn layout_tab(app: &mut App, ui: &mut egui::Ui, t: &Target) {
    let chain = para_chain(app, t);
    if t.owner == Owner::Body {
        section(ui, "Columns");
        let (count, gap) = app.wb.as_ref().map(|w| w.columns()).unwrap_or((1, 0.05));
        let tw = app.layout.as_ref().map(|l| l.setup.text_width()).unwrap_or(468.0);
        let mut set = None;
        row(ui, "Columns", |ui| {
            let mut n = count as f64;
            if theme::stepper(ui, &mut n, 1.0, 1.0..=10.0, "", 0) {
                set = Some((n as u32, gap));
            }
        });
        ui.add_enabled_ui(count > 1, |ui| {
            row(ui, "Gutter", |ui| {
                let mut g = (gap * tw) as f64;
                if theme::stepper(ui, &mut g, 1.0, 0.0..=200.0, " pt", 0) {
                    set = Some((count, g as f32 / tw));
                }
            });
        });
        if let Some((n, g)) = set {
            app.edit_pages("Columns", |p, h| p.set_columns(h, n, g));
        }
    }
    section(ui, "Indents");
    for (label, field) in [("First", "first_line_indent"), ("Left", "left_indent"), ("Right", "right_indent")] {
        let cur = lookup(&chain, |p| p.f32(field)).unwrap_or(0.0);
        row(ui, label, |ui| {
            let mut v = cur as f64;
            if theme::stepper(ui, &mut v, 1.0, -500.0..=1000.0, " pt", 0) {
                let v = v as f32;
                apply_para(app, "Indent", move |pp| pp.set_f32(field, v));
            }
        });
    }
    section(ui, "Tabs");
    let stops = lookup(&chain, |p| p.f32("default_tab_stops")).unwrap_or(36.0);
    row(ui, "Default Spacing", |ui| {
        let mut v = stops as f64;
        if theme::stepper(ui, &mut v, 1.0, 1.0..=500.0, " pt", 0) {
            let v = v as f32;
            apply_para(app, "Tabs", move |pp| pp.set_f32("default_tab_stops", v));
        }
    });
    section(ui, "Paragraph Borders");
    let stroke = lookup(&chain, |p| p.msg("stroke").and_then(kn_model::style::stroke));
    let positions = lookup(&chain, |p| p.u64("border_positions")).unwrap_or(0);
    let names = ["None", "Line", "Box"];
    let cur = if positions == 0 || stroke.is_none() { 0 } else if positions == 15 { 2 } else { 1 };
    let mut pick = None;
    theme::combo("borders").selected_text(names[cur]).width(ui.available_width()).show_ui(ui, |ui| {
        for (i, n) in names.iter().enumerate() {
            if ui.selectable_label(i == cur, *n).clicked() {
                pick = Some(i);
            }
        }
    });
    if let Some(i) = pick {
        apply_para(app, "Paragraph Borders", move |pp| {
            if i == 0 {
                pp.set_u64("border_positions", 0);
            } else {
                pp.set_u64("border_positions", if i == 2 { 15 } else { 1 });
                if pp.msg("stroke").is_none() {
                    let s = pg_core::kn_model::format::StrokeSpec { color: kn_model::Color::BLACK, width: 1.0, dash: pg_core::kn_model::format::Dash::Solid };
                    pp.push_msg("stroke", pg_core::edit::para_stroke_msg(&s));
                }
            }
        });
    }
    if cur > 0 {
        let s = stroke.clone().unwrap();
        row(ui, "Color", |ui| {
            if let Some(c) = color_button(ui, s.color) {
                let w = s.width;
                apply_para(app, "Border Color", move |pp| {
                    pp.remove("stroke");
                    pp.push_msg("stroke", pg_core::edit::para_stroke_msg(&pg_core::kn_model::format::StrokeSpec { color: c, width: w, dash: pg_core::kn_model::format::Dash::Solid }));
                });
            }
        });
        row(ui, "Width", |ui| {
            let mut v = s.width as f64;
            if theme::stepper(ui, &mut v, 0.5, 0.5..=20.0, " pt", 1) {
                let (c, w) = (s.color, v as f32);
                apply_para(app, "Border Width", move |pp| {
                    pp.remove("stroke");
                    pp.push_msg("stroke", pg_core::edit::para_stroke_msg(&pg_core::kn_model::format::StrokeSpec { color: c, width: w, dash: pg_core::kn_model::format::Dash::Solid }));
                });
            }
        });
    }
    section(ui, "Paragraph Background");
    let fill = lookup(&chain, |p| p.msg("fill").map(kn_model::style::color));
    row(ui, "Color", |ui| {
        if ui.small_button("None").clicked() {
            apply_para(app, "Paragraph Background", |pp| {
                pp.remove("fill");
            });
        }
        if let Some(c) = color_button(ui, fill.unwrap_or(kn_model::Color::TRANSPARENT)) {
            apply_para(app, "Paragraph Background", move |pp| {
                pp.remove("fill");
                pp.push_msg("fill", kn_model::format::color_msg(c));
            });
        }
    });
}

fn more_tab(app: &mut App, ui: &mut egui::Ui, t: &Target) {
    let chain = para_chain(app, t);
    section(ui, "Pagination & Breaks");
    for (label, field, default) in [("Keep lines on same page", "keep_lines_together", false), ("Keep with next paragraph", "keep_with_next", false), ("Start paragraph on a new page", "page_break_before", false), ("Prevent widow & orphan lines", "widow_control", true)] {
        let mut on = lookup(&chain, |p| p.bool(field)).unwrap_or(default);
        if theme::checkbox(ui, &mut on, label).changed() {
            apply_para(app, label, move |pp| pp.set_bool(field, on));
        }
    }
    section(ui, "Hyphenation & Ligatures");
    let mut hy = lookup(&chain, |p| p.bool("hyphenate")).unwrap_or(true);
    if theme::checkbox(ui, &mut hy, "Hyphenate paragraph").changed() {
        apply_para(app, "Hyphenation", move |pp| pp.set_bool("hyphenate", hy));
    }
    let mut lig = app.wb.as_ref().map(|w| w.setup().ligatures).unwrap_or(true);
    if theme::checkbox(ui, &mut lig, "Show ligatures").changed() {
        app.edit_pages("Ligatures", |p, h| {
            let mut s = p.setup();
            s.ligatures = lig;
            p.set_setup(h, &s);
        });
    }
    section(ui, "Following Paragraph Style");
    let following = lookup(&chain, |p| p.ref_id("following_style"));
    let fname = following.and_then(|f| app.doc().map(|d| pg_core::edit::named_style(d, Some(f)).1)).unwrap_or_else(|| "Same".into());
    let styles = app.doc().map(|d| d.paragraph_styles()).unwrap_or_default();
    let mut pick: Option<Option<ObjId>> = None;
    theme::combo("following").selected_text(&fname).width(ui.available_width()).show_ui(ui, |ui| {
        if ui.selectable_label(following.is_none(), "Same").clicked() {
            pick = Some(None);
        }
        for (id, n) in &styles {
            if ui.selectable_label(Some(*id) == following, n).clicked() {
                pick = Some(Some(*id));
            }
        }
    });
    if let Some(f) = pick {
        // the style itself changes (a property of the paragraph style in Pages)
        if let Some(named) = app.doc().and_then(|d| pg_core::edit::named_style(d, pg_core::edit::para_style_at(d, t.storage, t.a)).0) {
            app.edit("Following Style", |d, h| {
                d.touch(h, named);
                if let Some(m) = d.file.obj_mut(named) {
                    let pp = m.msg_mut_or_create("para_properties");
                    match f {
                        Some(id) => pp.set_ref("following_style", id),
                        None => pp.remove("following_style"),
                    }
                }
                if let Some(a) = d.file.archive_mut(named) {
                    a.refresh_object_references();
                }
            });
        }
    }
}


#[allow(dead_code)]
fn unused(_a: Align, _l: LineSpacing) {}
