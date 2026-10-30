//! The table inspector's Table and Cell tabs: Keydeck's (kn-app inspector.rs; Keynote 15.4's
//! table inspector is Numbers 15.4's), for the sheet's selected table and cells.

#![allow(clippy::too_many_lines)]

use egui::{Color32, RichText};
use kn_ui::theme;
use pg_core::kn_format::ObjId;
use pg_core::kn_model;
use pg_core::kn_model::format::{Dash, StrokeSpec};
use pg_core::kn_model::{Color, DrawableKind, Fill};

use crate::app::App;
use crate::objinsp::{c32, color_button, fill_editor, section, stroke_editor};

/// The selected cells as (table, row0, col0, row1, col1), rows in the table's own order.
fn range(app: &App) -> Option<(ObjId, u32, u32, u32, u32)> {
    let sel = app.sel?;
    // merged cells are taken in whole (a border round a merge goes round all of it)
    let (r0, c0, r1, c1) = app.with_merges(sel.table, sel.block());
    let m: Vec<u32> = (r0..=r1).filter_map(|r| app.mrow(sel.table, r)).collect();
    Some((sel.table, *m.iter().min()?, c0, *m.iter().max()?, c1))
}

/// The cell the cursor is on.
fn table_cursor(app: &App) -> Option<(ObjId, u32, u32)> {
    let sel = app.sel?;
    Some((sel.table, app.mrow(sel.table, sel.cursor.0)?, sel.cursor.1))
}

pub fn cell_panel(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
    use kn_model::tablefmt::{BorderEdges, CellFormat};
    let DrawableKind::Table(t) = &d.kind else { return };
    let id = d.id;
    // the selected cells, or the whole table when none are (Keynote formats every cell then)
    let block = match range(app).filter(|r| r.0 == id) {
        Some((_, r0, c0, r1, c1)) => (r0, c0, r1, c1),
        None => match table_cursor(app).filter(|c| c.0 == id) {
            Some((_, r, c)) => (r, c, r, c),
            None => (0, 0, t.rows.saturating_sub(1), t.cols.saturating_sub(1)),
        },
    };
    let (r, c) = (block.0, block.1);
    let mut cmd: Option<(&'static str, Box<dyn FnOnce(&mut kn_model::Document, &mut kn_model::edit::History) -> Result<(), String>>)> = None;

    // ------------------------------------------------------------------ Data Format
    section(ui, "Data Format");
    let cur = app.wb.as_ref().map(|w| &w.doc).map(|doc| doc.cell_format(id, r, c)).unwrap_or(CellFormat::Automatic);
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
        CellFormat::Custom { .. } => usize::MAX,
    };
    let mut k = kind;
    theme::combo(("cell-format", id)).selected_text(kinds.get(kind).copied().unwrap_or("Custom")).width(ui.available_width()).show_ui(ui, |ui| {
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
    if let Some(spec) = fill_editor(ui, &format!("cell-fill-{id}"), &fill, app.wb.as_mut().map(|w| &mut w.doc)) {
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
                // no edges picked yet: the style goes to all of the selection's borders
                if edges.is_none() {
                    edges = Some(BorderEdges::All);
                }
                picked = true;
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
        app.notify(if line.is_none() { format!("Borders removed ({e:?})") } else { format!("Border set ({e:?})") });
    }

    // ------------------------------------------------------------------ Conditional Highlighting
    section(ui, "Conditional Highlighting");
    if ui.add_sized([ui.available_width(), 24.0], egui::Button::new("Conditional Highlighting…")).clicked() {
        crate::condwin::show();
    }

    if let Some((name, f)) = cmd {
        let slide = app.current_slide().map(|s| s.slide);
        let mut res = Ok(());
        let merge = app.pointer_down;
        if let Some(doc) = app.wb.as_mut().map(|w| &mut w.doc) {
            if merge {
                doc.begin_merge(&mut app.hist, name);
            } else {
                doc.begin(&mut app.hist, name);
            }
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

pub fn table_panel(app: &mut App, ui: &mut egui::Ui, d: &kn_model::Drawable) {
    use kn_model::tablerows::HeaderKind;
    let DrawableKind::Table(t) = &d.kind else { return };
    let id = d.id;
    let mut cmd: Option<Box<dyn FnOnce(&mut kn_model::Document, &mut kn_model::edit::History)>> = None;
    // Table Styles: the theme's presets, six to a page, and a "+" after them that saves this
    // table's look as a new style (R37, R41)
    // Numbers keeps them in TN.ThemeArchive (nm-core reads them)
    let presets: Vec<(ObjId, Option<ObjId>, Option<ObjId>)> = app.wb.as_ref().map(|w| pg_core::styles::table_style_presets(&w.doc).into_iter().map(|(p, n)| (p, None, Some(n))).collect()).unwrap_or_default();
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
                                let look = net.and_then(|n| app.wb.as_ref().map(|w| &w.doc).map(|doc| doc.table_preset_look(n))).unwrap_or_default();
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
                                theme::centered(p, thumb.center(), "+", egui::FontId::proportional(26.0), line.color);
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
    let mut name = app.wb.as_ref().map(|w| &w.doc).is_some_and(|doc| doc.table_name_shown(id));
    if theme::checkbox(ui, &mut name, "Title").changed() {
        cmd = Some(Box::new(move |doc, h| doc.set_table_name_shown(h, id, name)));
    }
    let mut caption = app.wb.as_ref().map(|w| &w.doc).and_then(|doc| doc.file.obj(id)).and_then(|m| m.find_super("TSD.DrawableArchive")).is_some_and(|dm| {
        dm.bool("caption_hidden") == Some(false) && dm.ref_id("caption").is_some_and(|c| app.wb.as_ref().map(|w| &w.doc).is_some_and(|doc| doc.file.type_name(c) == "TSA.CaptionInfoArchive"))
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
        let size = app.wb.as_ref().map(|w| &w.doc).map(|doc| doc.table_font_size(id)).unwrap_or(12.0);
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
    let outline = app.wb.as_ref().map(|w| &w.doc).and_then(|doc| doc.table_outline(id));
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
                let on = app.wb.as_ref().map(|w| &w.doc).is_some_and(|doc| doc.table_gridline(id, g));
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
    let mut banded = app.wb.as_ref().map(|w| &w.doc).is_some_and(|doc| doc.table_banded(id));
    ui.horizontal(|ui| {
        if theme::checkbox(ui, &mut banded, "Alternating Row Color").changed() {
            cmd = Some(Box::new(move |doc, h| {
                doc.set_table_banded(h, id, banded);
            }));
        }
        if banded {
            let cur = match app.wb.as_ref().map(|w| &w.doc).and_then(|doc| doc.table_band_fill(id)) {
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
    let (r0, c0, r1, c1) = match range(app).filter(|r| r.0 == id) {
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
        let merge = app.pointer_down;
        if let Some(doc) = app.wb.as_mut().map(|w| &mut w.doc) {
            if merge {
                doc.begin_merge(&mut app.hist, "Table");
            } else {
                doc.begin(&mut app.hist, "Table");
            }
            f(doc, &mut app.hist);
            doc.end(&mut app.hist);
        }
        if let Some(s) = slide {
            app.bump(s);
        }
    }
}

