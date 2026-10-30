//! The Format inspector: Table / Cell / Text / Arrange tabs (Numbers 15.4, N-insp-cell-selected-*).
//! Every control calls one of Keydeck's table edits on the selection.

use egui::{Color32, RichText};
use nm_core::kn_model::tablecond::{Cmp, LOOKS};

use crate::app::{App, InspTab};
use crate::canvas::commit;

fn heading(ui: &mut egui::Ui, s: &str) {
    crate::objinsp::section(ui, s);
}

fn tabs(app: &mut App, ui: &mut egui::Ui) {
    let order = [InspTab::Table, InspTab::Cell, InspTab::Text, InspTab::Arrange];
    let mut cur = order.iter().position(|t| *t == app.insp_tab).unwrap_or(0);
    crate::objinsp::tabs(ui, &["Table", "Cell", "Text", "Arrange"], &mut cur);
    app.insp_tab = order[cur];
}

/// Nothing selected: the sheet (Numbers' Sheet inspector: its name).
fn sheet_panel(app: &mut App, ui: &mut egui::Ui) {
    let mut cur = 0;
    crate::objinsp::tabs(ui, &["Sheet"], &mut cur);
    crate::objinsp::section(ui, "Sheet Name");
    let Some(s) = app.sheets().get(app.sheet).cloned() else { return };
    let key = egui::Id::unique("sheet-name").with(s.id);
    let mut name: String = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| s.name.clone());
    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
    if r.lost_focus() && !name.trim().is_empty() && name.trim() != s.name {
        let n = name.trim().to_string();
        app.edit("Rename Sheet", |d, h| nm_core::sheets::rename_sheet(d, h, s.id, &n));
        ui.data_mut(|d| d.remove::<String>(key));
    } else {
        ui.data_mut(|d| d.insert_temp(key, name));
    }
    // Tab Color (Numbers' Sheet inspector: a colour well; the swatch next to it clears it)
    ui.add_space(8.0);
    crate::objinsp::section(ui, "Tab Color");
    let cur = app.wb.as_ref().and_then(|w| nm_core::sheets::tab_color(w.file(), s.id));
    let mut set: Option<Option<nm_core::kn_model::Color>> = None;
    ui.horizontal(|ui| {
        let shown = cur.unwrap_or(nm_core::kn_model::Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
        if let Some(c) = crate::objinsp::color_button(ui, shown) {
            set = Some(Some(c));
        }
        if ui.add_enabled(cur.is_some(), egui::Button::new("None")).clicked() {
            set = Some(None);
        }
    });
    if let Some(c) = set {
        if let Some(Err(e)) = app.edit("Change Tab Color", |d, h| nm_core::sheets::set_tab_color(d, h, s.id, c)) {
            app.notify(format!("Tab Color: {e}"));
        }
    }
    ui.add_space(12.0);
    // Numbers' Sheet pane buttons
    let i = app.sheet;
    let n = app.sheets().iter().filter(|s| !s.hidden).count();
    let w = ui.available_width();
    if ui.add_sized([w, 28.0], egui::Button::new("Duplicate Sheet")).clicked() {
        app.duplicate_sheet(i);
    }
    if ui.add_enabled_ui(n > 1, |ui| ui.add_sized([w, 28.0], egui::Button::new("Hide Sheet"))).inner.clicked() {
        app.hide_sheet(i);
    }
    if ui.add_enabled_ui(n > 1, |ui| ui.add_sized([w, 28.0], egui::Button::new("Delete Sheet"))).inner.clicked() {
        app.edit("Delete Sheet", |d, h| nm_core::sheets::delete_sheet(d, h, i));
        app.sel = None;
        app.sheet = app.sheet.min(app.sheets().len().saturating_sub(1));
        app.canvas.reset();
    }
    ui.add_space(12.0);
    ui.label(RichText::new("Select a table, cells or an object to format them.").color(kn_ui::theme::TEXT_DIM()));
}

/// While a formula is typed the inspector is Numbers' Functions browser: search, categories,
/// the function's syntax and description; Insert Function puts it in the formula.
fn functions_browser(app: &mut App, ui: &mut egui::Ui) {
    let id = egui::Id::unique("fn-browser");
    let (mut search, mut cat, mut picked) = ui.data(|d| d.get_temp::<(String, String, String)>(id)).unwrap_or_default();
    // the function being typed (or the last one called) is the one described
    if let Some(e) = &app.canvas.edit {
        let up = e.text.to_ascii_uppercase();
        let mut best: Option<&str> = None;
        let mut word = String::new();
        for ch in up.chars().chain(std::iter::once(' ')) {
            if ch.is_ascii_alphanumeric() || ch == '.' {
                word.push(ch);
            } else {
                if !word.is_empty() {
                    if let Some(f) = nm_core::formula::catalog().iter().find(|f| f.name == word) {
                        best = Some(f.name);
                    }
                }
                word.clear();
            }
        }
        if let Some(b) = best {
            picked = b.to_string();
        }
    }
    heading(ui, "Functions");
    ui.add(egui::TextEdit::singleline(&mut search).hint_text("Search").desired_width(ui.available_width() - 8.0));
    let cats: Vec<&str> = {
        let mut c: Vec<&str> = nm_core::formula::catalog().iter().map(|f| f.category).collect();
        c.sort_unstable();
        c.dedup();
        c
    };
    kn_ui::theme::combo("fn-cat").selected_text(if cat.is_empty() { "All" } else { cat.as_str() }).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
        if ui.selectable_label(cat.is_empty(), "All").clicked() {
            cat.clear();
        }
        for c in cats {
            if ui.selectable_label(cat == c, c).clicked() {
                cat = c.to_string();
            }
        }
    });
    let q = search.to_ascii_uppercase();
    let list: Vec<&nm_core::formula::FunctionInfo> = nm_core::formula::catalog().iter().filter(|f| (cat.is_empty() || f.category == cat) && (q.is_empty() || f.name.contains(&q))).collect();
    let mut insert = None;
    egui::ScrollArea::vertical().id_salt("fn-list").max_height(260.0).show(ui, |ui| {
        for f in &list {
            let r = ui.selectable_label(picked == f.name, f.name);
            if r.clicked() {
                picked = f.name.to_string();
            }
            if r.double_clicked() {
                insert = Some(f.name);
            }
        }
    });
    if let Some(f) = nm_core::formula::catalog().iter().find(|f| f.name == picked) {
        ui.separator();
        ui.label(RichText::new(f.syntax).strong());
        ui.label(f.description);
        if ui.button("Insert Function").clicked() {
            insert = Some(f.name);
        }
    }
    ui.data_mut(|d| d.insert_temp(id, (search, cat, picked)));
    if let (Some(name), Some(e)) = (insert, app.canvas.edit.as_mut()) {
        // replace a partly typed name at the end, else add the call
        let tail = e.text.len() - e.text.trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '.').len();
        if tail > 0 && name.starts_with(&e.text[e.text.len() - tail..].to_ascii_uppercase()) {
            let n = e.text.len() - tail;
            e.text.truncate(n);
        }
        e.text.push_str(name);
        e.text.push('(');
        e.focus = true;
    }
}

pub fn inspector(app: &mut App, ui: &mut egui::Ui) {
    // an object (text box, shape, picture, chart): Keydeck's object inspector
    if let Some(id) = app.canvas.obj.filter(|o| app.table(*o).is_none()) {
        crate::objinsp::ui(app, ui, id);
        return;
    }
    let Some(sel) = app.sel else {
        sheet_panel(app, ui);
        return;
    };
    if app.canvas.edit.as_ref().is_some_and(|e| e.text.trim_start().starts_with('=')) {
        functions_browser(app, ui);
        return;
    }
    if app.insp_tab == InspTab::Organize {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| organize(app, ui, sel.table));
        return;
    }
    tabs(app, ui);
    let Some(d) = app.table(sel.table).map(|(d, _)| d.clone()) else { return };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match app.insp_tab {
        InspTab::Table => crate::tblinsp::table_panel(app, ui, &d),
        InspTab::Cell => {
            crate::tblinsp::cell_panel(app, ui, &d);
            cond_highlight(app, ui);
        }
        InspTab::Text => cell_text(app, ui, sel),
        InspTab::Arrange => crate::objinsp::arrange(app, ui, std::slice::from_ref(&d)),
        InspTab::Organize => {}
    });
}

/// Organize (Numbers 15.4, N-organize-*): Sort, Filter and Categories for the selected table.
fn organize(app: &mut App, ui: &mut egui::Ui, t: nm_core::kn_format::ObjId) {
    crate::organize::ui(app, ui, t);
}

/// Conditional Highlighting (Numbers' Cell tab): the rules of the selected cell, in the order
/// they are tried (the first that holds styles the cell), each removable; Add a Rule puts one
/// after them on every selected cell with a value; Clear All Rules.
fn cond_highlight(app: &mut App, ui: &mut egui::Ui) {
    let Some(sel) = app.sel else { return };
    let t = sel.table;
    let blocks = app.model_blocks(t, sel.block());
    heading(ui, "Conditional Highlighting");
    let cmps = [(Cmp::Equal, "is equal to"), (Cmp::NotEqual, "is not equal to"), (Cmp::Greater, "is greater than"), (Cmp::GreaterOrEqual, "is greater than or equal to"), (Cmp::Less, "is less than"), (Cmp::LessOrEqual, "is less than or equal to")];
    let cmp_name = |c: Option<Cmp>| c.and_then(|c| cmps.iter().find(|x| x.0 == c)).map(|x| x.1).unwrap_or("a rule");
    let cur = (app.mrow(t, sel.cursor.0).unwrap_or(sel.cursor.0), sel.cursor.1);
    let rules = app.wb.as_ref().map(|w| w.doc.highlight_rules(t, cur.0, cur.1)).unwrap_or_default();
    let mut remove: Option<usize> = None;
    for (i, r) in rules.iter().enumerate() {
        egui::Frame::new().fill(kn_ui::theme::CONTROL_BG()).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width() - 8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Rule {}", i + 1)).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Delete Rule").clicked() {
                        remove = Some(i);
                    }
                });
            });
            let v = if r.value.fract() == 0.0 { format!("{}", r.value as i64) } else { format!("{}", r.value) };
            ui.label(format!("{} {v}", cmp_name(r.cmp)));
            let look = LOOKS.iter().find(|l| l.name == r.look);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                let c = look.and_then(|l| l.fill.or(l.text)).map(|c| Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)).unwrap_or(kn_ui::theme::TEXT_DIM());
                ui.painter().rect_filled(rect, 3.0, c);
                ui.label(&r.look);
            });
        });
        ui.add_space(4.0);
    }
    if rules.is_empty() {
        ui.label(RichText::new("The selected cell has no rules.").color(kn_ui::theme::TEXT_DIM()));
    }
    // a rule goes away: the cells' rules are cleared and the others put back, in order
    if let Some(i) = remove {
        commit(app);
        let keep: Vec<_> = rules.iter().enumerate().filter(|(k, _)| *k != i).filter_map(|(_, r)| Some((r.cmp?, r.value, LOOKS.iter().find(|l| l.name == r.look)?))).collect();
        if let Some(Err(e)) = app.edit("Delete Rule", |d, h| -> Result<(), nm_core::kn_format::Error> {
            for b in &blocks {
                d.clear_highlight_rules(h, t, *b)?;
                for (cmp, v, look) in &keep {
                    d.add_highlight_rule(h, t, *b, *cmp, *v, look)?;
                }
            }
            Ok(())
        }) {
            app.notify(format!("Conditional Highlighting: {e}"));
        }
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Add a Rule").strong());
    let id = egui::Id::unique("cond-rule");
    let (mut cmp_i, mut value, mut look_i) = ui.data(|d| d.get_temp::<(usize, String, usize)>(id)).unwrap_or((2, "0".into(), 0));
    kn_ui::theme::combo("cond-cmp").selected_text(cmps[cmp_i].1).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
        for (i, (_, n)) in cmps.iter().enumerate() {
            ui.selectable_value(&mut cmp_i, i, *n);
        }
    });
    ui.add(egui::TextEdit::singleline(&mut value).desired_width(ui.available_width() - 8.0));
    kn_ui::theme::combo("cond-look").selected_text(LOOKS[look_i].name).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
        for (i, l) in LOOKS.iter().enumerate() {
            ui.selectable_value(&mut look_i, i, l.name);
        }
    });
    let v: Option<f64> = value.trim().parse().ok();
    ui.horizontal(|ui| {
        if ui.add_enabled(v.is_some(), egui::Button::new("Add Rule")).clicked() {
            commit(app);
            let cmp = cmps[cmp_i].0;
            if let Some(Err(e)) = app.edit("Conditional Highlighting", |d, h| blocks.iter().try_for_each(|b| d.add_highlight_rule(h, t, *b, cmp, v.unwrap_or(0.0), &LOOKS[look_i]).map(|_| ()))) {
                app.notify(format!("Conditional Highlighting: {e}"));
            }
        }
        if ui.add_enabled(!rules.is_empty() || blocks.len() > 1 || sel.block().0 != sel.block().2 || sel.block().1 != sel.block().3, egui::Button::new("Clear All Rules")).clicked() {
            commit(app);
            if let Some(Err(e)) = app.edit("Clear All Rules", |d, h| blocks.iter().try_for_each(|b| d.clear_highlight_rules(h, t, *b).map(|_| ()))) {
                app.notify(format!("Conditional Highlighting: {e}"));
            }
        }
    });
    ui.label(RichText::new("Rules go on the selected cells that hold values.").small().color(kn_ui::theme::TEXT_DIM()));
    ui.data_mut(|d| d.insert_temp(id, (cmp_i, value, look_i)));
}

/// Text, for the selected cells (Numbers' Text inspector with cells selected): Font (family,
/// typeface, size, bold / italic / underline / strikethrough, colour), Alignment (left, centre,
/// right, justify, auto; top, middle, bottom) and Wrap Text in Cell.
fn cell_text(app: &mut App, ui: &mut egui::Ui, sel: crate::app::Sel) {
    use nm_core::kn_model::format::TextProp;
    use nm_core::kn_model::tablecells::CellAlign;
    use nm_core::kn_model::{Align, VAlign};
    use kn_ui::theme;
    let t = sel.table;
    let Some(st) = app.table(t).and_then(|(_, td)| td.cell_styles.get((sel.cursor.0 * td.cols + sel.cursor.1) as usize).cloned()) else { return };
    let run = st.text.clone();
    let b = sel.block();
    // rows go by the model's rows (categories show them in another order)
    let block = (app.mrow(t, b.0).unwrap_or(b.0), b.1, app.mrow(t, b.2).unwrap_or(b.2), b.3);
    let mut props: Vec<(&str, TextProp)> = Vec::new();
    let mut align: Option<CellAlign> = None;
    let mut valign: Option<VAlign> = None;
    let mut wrap: Option<bool> = None;

    let mut pad: Option<[f32; 4]> = None;
    let para = st.para.clone();
    // Style | Layout (Numbers' Text inspector)
    let key = ui.scope_id().with("cell-text-subtab");
    let mut sub = ui.data(|m| m.get_temp::<usize>(key)).unwrap_or(0);
    ui.add_space(4.0);
    crate::objinsp::seg_small(ui, &["Style", "Layout"], &mut sub);
    ui.data_mut(|m| m.insert_temp(key, sub));
    let pt_row = |ui: &mut egui::Ui, label: &str, v: f32, range: std::ops::RangeInclusive<f64>| -> Option<f32> {
        let mut out = None;
        ui.horizontal(|ui| {
            ui.label(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut x = f64::from(v);
                if theme::stepper(ui, &mut x, 1.0, range, " pt", 0) {
                    out = Some(x.round() as f32);
                }
            });
        });
        out
    };
    if sub == 1 {
        // Layout: Text Inset, Indents
        crate::objinsp::section(ui, "Text Inset");
        if let Some(v) = pt_row(ui, "All Sides", st.padding[0], 0.0..=200.0) {
            pad = Some([v; 4]);
        }
        crate::objinsp::section(ui, "Indents");
        if let Some(v) = pt_row(ui, "First", para.first_line_indent, -500.0..=500.0) {
            props.push(("First Line Indent", TextProp::FirstIndent(v)));
        }
        if let Some(v) = pt_row(ui, "Left", para.left_indent, 0.0..=500.0) {
            props.push(("Left Indent", TextProp::LeftIndent(v)));
        }
        if let Some(v) = pt_row(ui, "Right", para.right_indent, 0.0..=500.0) {
            props.push(("Right Indent", TextProp::RightIndent(v)));
        }
    } else {
        crate::objinsp::section(ui, "Font");
        let family = kn_render::fonts::parse_ps_name(&run.font_name).0;
        if app.font_names.is_none() {
            app.font_names = Some(crate::editing::font_names(app));
        }
        let fonts = app.font_names.clone().unwrap_or_default();
        theme::combo("cell-font").selected_text(&family).width(ui.available_width()).height(400.0).show_ui(ui, |ui| {
            for f in &fonts {
                if ui.selectable_label(*f == family, f).clicked() && *f != family {
                    props.push(("Font", TextProp::Font(f.replace(' ', ""))));
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
            theme::combo("cell-typeface").selected_text(typeface).width(ui.available_width() - 100.0).show_ui(ui, |ui| {
                for (n, bo, it) in [("Regular", false, false), ("Italic", false, true), ("Bold", true, false), ("Bold Italic", true, true)] {
                    if ui.selectable_label(n == typeface, n).clicked() && n != typeface {
                        if bo != run.bold {
                            props.push(("Bold", TextProp::Bold(bo)));
                        }
                        if it != run.italic {
                            props.push(("Italic", TextProp::Italic(it)));
                        }
                    }
                }
            });
            let mut size = run.size as f64;
            if theme::stepper(ui, &mut size, 1.0, 1.0..=1000.0, " pt", 0) {
                props.push(("Font Size", TextProp::Size(size.round() as f32)));
            }
        });
        // B I U S, then the ⚙ advanced options (Numbers' Font row)
        ui.horizontal(|ui| {
            let w = ui.available_width();
            ui.allocate_ui(egui::vec2(w - 68.0, 30.0), |ui| {
                if let Some(c) = crate::objinsp::bius(ui, "cell-bius", &run) {
                    props.push(c);
                }
            });
            crate::objinsp::font_gear(ui, &run, &mut props);
        });
        ui.horizontal(|ui| {
            ui.label("Text Color");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(c) = crate::objinsp::color_button(ui, run.color) {
                    props.push(("Text Color", TextProp::Color(c)));
                }
            });
        });

        crate::objinsp::section(ui, "Alignment");
        // left, centre, right, justify, Auto-Align (Numbers' icon row)
        let hcur = match st.align {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
            Align::Justify => 3,
            Align::Natural => 4,
        };
        let tips = ["Align Left", "Center", "Align Right", "Justify", "Auto-Align Text"];
        let before = ui.cursor().min;
        if let Some(i) = crate::objinsp::icon_segments(ui, "cell-align", 5, Some(hcur), crate::objinsp::align_icon) {
            if i != hcur {
                align = Some([CellAlign::Left, CellAlign::Center, CellAlign::Right, CellAlign::Justify, CellAlign::Auto][i]);
            }
        }
        // tooltips for the icons
        let row = egui::Rect::from_min_size(before, egui::vec2(ui.available_width(), 30.0));
        if let Some(p) = ui.ctx().pointer_hover_pos().filter(|p| row.contains(*p)) {
            let k = (((p.x - row.min.x) / (row.width() / 5.0)) as usize).min(4);
            egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), ui.scope_id().with("cell-align-tip"), egui::PopupAnchor::Pointer).gap(12.0).show(|ui| {
                ui.label(tips[k]);
            });
        }
        ui.add_space(6.0);
        let vcur = match st.valign {
            VAlign::Top => 0,
            VAlign::Middle => 1,
            VAlign::Bottom => 2,
        };
        if let Some(i) = crate::objinsp::icon_segments(ui, "cell-valign", 3, Some(vcur), crate::objinsp::valign_icon) {
            if i != vcur {
                valign = Some([VAlign::Top, VAlign::Middle, VAlign::Bottom][i]);
            }
        }
        ui.add_space(6.0);
        let mut w = st.wrap;
        if theme::checkbox(ui, &mut w, "Wrap text in cell").changed() {
            wrap = Some(w);
        }
        // indent: decrease / increase (a tab stop's worth each)
        ui.add_space(6.0);
        if let Some(i) = crate::objinsp::icon_segments(ui, "cell-indent", 2, None, |pt, r, i, col| {
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
            let v = if i == 0 { (para.left_indent - 36.0).max(0.0) } else { para.left_indent + 36.0 };
            props.push((if i == 0 { "Decrease Indent" } else { "Increase Indent" }, TextProp::LeftIndent(v)));
        }
        // Spacing: line spacing (lines), before and after paragraphs
        if theme::disclosure(ui, "cell-spacing", "Spacing", None) {
            let amount = match para.line_spacing {
                nm_core::kn_model::LineSpacing::Relative(a) => a,
                _ => 1.0,
            };
            ui.horizontal(|ui| {
                ui.label("Lines");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut v = f64::from(amount);
                    if theme::stepper(ui, &mut v, 0.1, 0.5..=10.0, "", 1) {
                        props.push(("Line Spacing", TextProp::LineSpacing(v as f32)));
                    }
                });
            });
            if let Some(v) = pt_row(ui, "Before Paragraph", para.space_before, 0.0..=500.0) {
                props.push(("Space Before", TextProp::SpaceBefore(v)));
            }
            if let Some(v) = pt_row(ui, "After Paragraph", para.space_after, 0.0..=500.0) {
                props.push(("Space After", TextProp::SpaceAfter(v)));
            }
        }
    }

    // apply (each as Numbers names the step)
    if props.is_empty() && align.is_none() && valign.is_none() && wrap.is_none() && pad.is_none() {
        return;
    }
    if let Some(p) = pad {
        if let Some(Err(e)) = app.edit("Text Inset", |d, h| d.set_cells_padding(h, t, block, p).map(|_| ())) {
            app.notify(format!("{e}"));
        }
    }
    commit(app);
    let mut err = None;
    for (label, p) in props {
        if let Some(Err(e)) = app.edit(label, |d, h| d.set_cells_text_prop(h, t, block, &p)) {
            err = Some(e);
        }
    }
    if let Some(a) = align {
        if let Some(Err(e)) = app.edit("Alignment", |d, h| d.set_cells_alignment(h, t, block, a)) {
            err = Some(e);
        }
    }
    if let Some(v) = valign {
        if let Some(Err(e)) = app.edit("Alignment", |d, h| d.set_cells_valign(h, t, block, v).map(|_| ())) {
            err = Some(e);
        }
    }
    if let Some(w) = wrap {
        if let Some(Err(e)) = app.edit("Wrap Text", |d, h| d.set_cells_wrap(h, t, block, w).map(|_| ())) {
            err = Some(e);
        }
    }
    if let Some(e) = err {
        app.notify(format!("{e}"));
    }
}
