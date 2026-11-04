//! Math Notes: the notes list, the editor (results painted after each "=" in the accent colour,
//! aligned to the text's own layout) and the graph view (pan by drag, zoom by scroll/pinch).

use crate::app::{toolbar_button, App, PopAct, Popup};
use crate::draw::{self, col, fade};
use crate::fonts;
use calc_core::history::short_date;
use calc_core::notes::{self, LineResult};
use calc_core::session::now;
use egui::text::{LayoutJob, TextFormat};
use egui::{pos2, vec2, Align2, Color32, Id, Rect, Sense, Shape, Stroke, Ui, UiBuilder};

const CURVES: [u32; 4] = [0, 0xFF9F0AFF, 0x30D158FF, 0xFF375FFF];
const BODY: f32 = 15.0;
const TITLE: f32 = 22.0;

/// The visible window of the graph, in data units.
#[derive(Clone, Copy)]
pub struct GraphView {
    pub note: u64,
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
}

pub fn ui(app: &mut App, ui: &mut Ui, live: bool) {
    let screen = ui.ctx().content_rect();
    let th = app.theme();
    let p = ui.painter().clone();
    let side = (screen.width() * 0.045).clamp(12.0, 28.0);
    let bar = Rect::from_min_size(pos2(screen.left(), screen.top() + 6.0), vec2(screen.width(), 44.0));
    // the list uses the engine's mode button; the editor has its own top bar
    let mode_r = app
        .lay
        .toolbar
        .iter()
        .find(|b| b.id == "mode")
        .map(|b| draw::rect(b.rect))
        .unwrap_or(Rect::from_min_size(pos2(screen.right() - side - 44.0, bar.top()), vec2(44.0, 44.0)));
    let left_r = Rect::from_min_size(pos2(screen.left() + side, bar.top()), vec2(44.0, 44.0));
    if app.ov.note.is_none() {
        if button(ui, &p, &th, "mode", mode_r, "calculator", live) {
            app.open_menu(mode_r);
        }
        list(app, ui, Rect::from_min_max(pos2(screen.left(), bar.bottom()), screen.max), live, &th);
        return;
    }
    if button(ui, &p, &th, "back", left_r, "back", live) {
        app.close_note();
        return;
    }
    // top right: calculator (mode menu), share (copy the note), ellipsis (more)
    let w3 = 3.0 * 40.0 + 8.0;
    let group = Rect::from_min_size(pos2(screen.right() - side - w3, bar.top()), vec2(w3, 44.0));
    match icon_group(ui, &p, &th, group, &["calculator", "share", "ellipsis"], live) {
        Some((0, r)) => app.open_menu(r),
        Some((1, _)) => app.ctx.copy_text(app.ov.note_text.clone()),
        Some((2, r)) => {
            if let Some(id) = app.ov.note {
                app.ov.popup = Some(Popup { pos: pos2(r.center().x, r.bottom() + 52.0), items: vec![("Copy", PopAct::CopyText(app.ov.note_text.clone())), ("Delete Note", PopAct::DeleteNote(id))] });
            }
        }
        _ => {}
    }
    // bottom bar: checklist, attachment, markup (decorative), new note
    let bb = Rect::from_min_size(pos2(screen.left() + side, screen.bottom() - 58.0), vec2(w3, 44.0));
    icon_group(ui, &p, &th, bb, &["checklist", "paperclip", "markup"], false);
    let compose = Rect::from_min_size(pos2(screen.right() - side - 44.0, bb.top()), vec2(44.0, 44.0));
    if button(ui, &p, &th, "compose", compose, "compose", live) {
        app.close_note();
        let id = app.s.notes.create(now());
        app.s.save_notes();
        app.open_note(id);
        return;
    }
    editor(app, ui, Rect::from_min_max(pos2(screen.left(), bar.bottom() + 6.0), pos2(screen.right(), bb.top() - 8.0)), live);
}

/// A toolbar button: glass circle on iOS 26, plain accent icon otherwise.
fn button(ui: &Ui, p: &egui::Painter, th: &calc_core::theme::Theme, id: &str, r: Rect, icon: &str, live: bool) -> bool {
    let mut down = false;
    let mut clicked = false;
    if live {
        let resp = ui.interact(r, Id::unique(("notes_tb", id)), Sense::click());
        down = resp.is_pointer_button_down_on();
        clicked = resp.clicked();
    }
    toolbar_button(p, th, r, icon, down);
    clicked
}

/// Icons sharing one glass capsule (iOS 26) or standing free; returns the tapped one.
fn icon_group(ui: &Ui, p: &egui::Painter, th: &calc_core::theme::Theme, r: Rect, icons: &[&str], live: bool) -> Option<(usize, Rect)> {
    if th.glass_toolbar {
        let (top, bot) = crate::app::glass_fill(false);
        draw::glass_shape(p, r, r.height() / 2.0, top, bot);
    }
    let cw = r.width() / icons.len() as f32;
    let mut hit = None;
    for (i, ic) in icons.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(r.left() + i as f32 * cw, r.top()), vec2(cw, r.height()));
        let mut down = false;
        if live {
            let resp = ui.interact(cell, Id::unique(("notes_grp", *ic)), Sense::click());
            down = resp.is_pointer_button_down_on();
            if resp.clicked() {
                hit = Some((i, cell));
            }
        }
        let c = if th.glass_toolbar { Color32::WHITE } else { col(th.accent) };
        draw::icon(p, ic, Rect::from_center_size(cell.center(), vec2(21.0, 21.0)), fade(c, if down { 0.45 } else { 1.0 }));
    }
    hit
}

fn list(app: &mut App, ui: &mut Ui, r: Rect, live: bool, th: &calc_core::theme::Theme) {
    let p = ui.painter().clone();
    let text_c = col(th.panel_text);
    let dim = col(th.panel_dim);
    let accent = col(th.accent);
    let pad = 20.0;
    draw::text(&p, pos2(r.left() + pad, r.top() + 44.0), Align2::LEFT_BOTTOM, "Math Notes", fonts::ui(700, 32.0), text_c);
    let compose = Rect::from_min_size(pos2(r.right() - pad - 44.0, r.bottom() - 60.0), vec2(44.0, 44.0));
    let body = Rect::from_min_max(pos2(r.left(), r.top() + 58.0), pos2(r.right(), compose.top() - 8.0));
    let items: Vec<(u64, String, String, String)> = app
        .s
        .notes
        .sorted()
        .iter()
        .map(|n| (n.id, notes::title(&n.text), notes::preview(&n.text), short_date(n.modified, app.utc)))
        .collect();
    if items.is_empty() {
        draw::text(&p, pos2(body.center().x, body.center().y - 12.0), Align2::CENTER_CENTER, "No Math Notes", fonts::ui(600, 20.0), dim);
        draw::text(&p, pos2(body.center().x, body.center().y + 16.0), Align2::CENTER_CENTER, "Type an expression ending in “=” to see its result.", fonts::ui(400, 14.0), dim);
    }
    let mut open = None;
    let mut lui = ui.new_child(UiBuilder::new().max_rect(body));
    lui.set_clip_rect(body);
    egui::ScrollArea::vertical().id_salt("notes_list").auto_shrink(false).show(&mut lui, |ui| {
        let w = ui.available_width();
        let row_h = 68.0;
        let total = Rect::from_min_size(ui.cursor().min + vec2(pad - 4.0, 0.0), vec2(w - 2.0 * (pad - 4.0), row_h * items.len() as f32));
        if !items.is_empty() {
            ui.painter().rect_filled(total, 12.0, if th.light { Color32::WHITE } else { draw::mix(col(th.bg), Color32::WHITE, 0.1) });
        }
        for (i, (id, title, preview, date)) in items.iter().enumerate() {
            let (rr, resp) = ui.allocate_exact_size(vec2(w, row_h), Sense::click());
            let rr = Rect::from_min_max(pos2(total.left(), rr.top()), pos2(total.right(), rr.bottom()));
            let p = ui.painter().clone();
            if resp.is_pointer_button_down_on() {
                p.rect_filled(rr, 12.0, Color32::from_gray(128).gamma_multiply(0.2));
            }
            draw::text(&p, pos2(rr.left() + 16.0, rr.top() + 26.0), Align2::LEFT_BOTTOM, title, fonts::ui(600, 17.0), text_c);
            let dt = draw::text(&p, pos2(rr.left() + 16.0, rr.top() + 50.0), Align2::LEFT_BOTTOM, date, fonts::ui(400, 15.0), text_c);
            let prev = if preview.is_empty() { "No additional text" } else { preview };
            let job = LayoutJob::simple_singleline(prev.to_string(), fonts::ui(400, 15.0), dim);
            let mut job = job;
            job.wrap = egui::text::TextWrapping::truncate_at_width(rr.right() - dt.right() - 30.0);
            let g = p.layout_job(job);
            p.galley(pos2(dt.right() + 8.0, dt.top()), g, dim);
            if i + 1 < items.len() {
                p.line_segment([pos2(rr.left() + 16.0, rr.bottom() - 0.5), pos2(rr.right(), rr.bottom() - 0.5)], Stroke::new(0.6, col(th.separator)));
            }
            if live && resp.clicked() {
                open = Some(*id);
            }
            if live && resp.secondary_clicked() {
                let pos = resp.interact_pointer_pos().unwrap_or(rr.center());
                app.ov.popup = Some(Popup { pos, items: vec![("Delete", PopAct::DeleteNote(*id))] });
            }
        }
    });
    let mut down = false;
    let mut clicked = false;
    if live {
        let resp = ui.interact(compose, Id::unique("compose"), Sense::click());
        down = resp.is_pointer_button_down_on();
        clicked = resp.clicked();
    }
    if th.glass_toolbar {
        toolbar_button(&p, th, compose, "compose", down);
    } else {
        draw::icon(&p, "compose", Rect::from_center_size(compose.center(), vec2(28.0, 28.0)), fade(accent, if down { 0.5 } else { 1.0 }));
    }
    if clicked {
        let id = app.s.notes.create(now());
        app.s.save_notes();
        open = Some(id);
    }
    if let Some(id) = open {
        app.open_note(id);
    }
}

/// Defined names (and the graphed "y") are tinted like iOS; the first line is the title.
fn job(text: &str, results: &[LineResult], th: &calc_core::theme::Theme, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let text_c = col(th.panel_text);
    let accent = col(th.accent);
    let mut names: Vec<String> = results.iter().filter_map(|r| r.var.clone()).collect();
    names.sort();
    names.dedup();
    let f = fonts::ui(400, BODY);
    let plain = TextFormat::simple(f.clone(), text_c);
    let tint = TextFormat::simple(f.clone(), accent);
    for (n, line) in text.split_inclusive('\n').enumerate() {
        if n == 0 {
            job.append(line, 0.0, TextFormat::simple(fonts::ui(700, TITLE), text_c));
            continue;
        }
        let mut run = String::new();
        let mut word = String::new();
        let flush = |job: &mut LayoutJob, run: &mut String, word: &mut String| {
            if names.iter().any(|v| v == word) {
                if !run.is_empty() {
                    job.append(run, 0.0, plain.clone());
                    run.clear();
                }
                job.append(word, 0.0, tint.clone());
            } else {
                run.push_str(word);
            }
            word.clear();
        };
        for ch in line.chars() {
            if ch.is_alphanumeric() || ch == '_' {
                word.push(ch);
            } else {
                flush(&mut job, &mut run, &mut word);
                run.push(ch);
            }
        }
        flush(&mut job, &mut run, &mut word);
        if !run.is_empty() {
            job.append(&run, 0.0, plain.clone());
        }
    }
    if text.is_empty() || text.ends_with('\n') {
        // keep the cursor row the right size
        let (w, sz) = if text.is_empty() { (700, TITLE) } else { (400, BODY) };
        job.append("", 0.0, TextFormat::simple(fonts::ui(w, sz), text_c));
    }
    job
}

fn editor(app: &mut App, ui: &mut Ui, r: Rect, live: bool) {
    let th = app.theme();
    let Some(id) = app.ov.note else { return };
    let rad = app.s.calc.rad;
    let style = app.s.settings.style;
    let results = notes::evaluate(&app.ov.note_text, rad, style);
    let graphs = notes::graphs(&app.ov.note_text);
    let pad = 20.0;
    let text_r = Rect::from_min_max(pos2(r.left() + pad, r.top()), pos2(r.right() - pad, r.bottom()));
    let mut tui = ui.new_child(UiBuilder::new().max_rect(text_r));
    tui.set_clip_rect(text_r);
    let accent = col(th.accent);
    let mut changed = false;
    egui::ScrollArea::vertical().id_salt(("note_scroll", id)).auto_shrink(false).show(&mut tui, |ui| {
        let res = results.clone();
        let th2 = th.clone();
        let mut layouter = move |ui: &Ui, buf: &dyn egui::TextBuffer, wrap: f32| ui.fonts_mut(|f| f.layout_job(job(buf.as_str(), &res, &th2, wrap)));
        let out = egui::TextEdit::multiline(&mut app.ov.note_text)
            .frame(egui::Frame::NONE)
            .desired_width(text_r.width())
            .desired_rows(if graphs.is_empty() { 12 } else { 3 })
            .interactive(live)
            .lock_focus(true)
            .layouter(&mut layouter)
            .id(Id::unique(("note_edit", id)))
            .show(ui);
        changed = out.response.changed();
        // results after the "=" of their line, from the galley's own rows
        let p = ui.painter().clone();
        let g = &out.galley;
        let mut line = 0usize;
        let mut ends: Vec<(usize, Rect)> = Vec::new();
        for (i, row) in g.rows.iter().enumerate() {
            if row.ends_with_newline || i + 1 == g.rows.len() {
                ends.push((line, row.rect().translate(out.galley_pos.to_vec2())));
            }
            if row.ends_with_newline {
                line += 1;
            }
        }
        for res in &results {
            let Some((_, rr)) = ends.iter().find(|(l, _)| *l == res.line) else { continue };
            let size = if res.line == 0 { TITLE } else { BODY };
            let pos = pos2(rr.right() + 4.0, rr.center().y);
            match res.kind {
                "result" => {
                    draw::text(&p, pos, Align2::LEFT_CENTER, &res.text, fonts::ui(500, size), accent);
                }
                "error" => {
                    draw::text(&p, pos, Align2::LEFT_CENTER, "Error", fonts::ui(500, size), col(0xFF453AFF));
                }
                "graph" => {
                    let chip = Rect::from_min_size(pos2(rr.right() + 8.0, rr.center().y - 10.0), vec2(28.0, 20.0));
                    p.rect_filled(chip, 10.0, fade(accent, 0.18));
                    draw::icon(&p, "graph", chip.shrink2(vec2(7.0, 3.0)), accent);
                    if live && ui.interact(chip, Id::unique(("chip", res.line)), Sense::click()).clicked() {
                        app.ov.graph = None;
                    }
                }
                _ => {}
            }
        }
        // the graph sits inline, right below the text
        if !graphs.is_empty() {
            ui.add_space(14.0);
            let w = ui.available_width();
            let gh = (w * 1.1).min(text_r.height() * 0.9).max(160.0);
            let (gr, _) = ui.allocate_exact_size(vec2(w, gh), Sense::hover());
            graph(app, ui, gr, &graphs, live);
            ui.add_space(20.0);
        }
    });
    if changed {
        app.s.notes.set_text(id, app.ov.note_text.clone(), now());
        app.s.save_notes();
    }
}

/// A "nice" grid step (1, 2, 5 × 10^n) for about `n` divisions of `range`.
fn step(range: f64, n: f64) -> f64 {
    let raw = (range / n).max(1e-12);
    let mag = 10f64.powf(raw.log10().floor());
    let f = raw / mag;
    mag * if f < 1.5 {
        1.0
    } else if f < 3.5 {
        2.0
    } else if f < 7.5 {
        5.0
    } else {
        10.0
    }
}

fn fmt_tick(v: f64, st: f64) -> String {
    let d = (-st.log10().floor()).max(0.0) as usize;
    let s = format!("{:.*}", d, v);
    if s == "-0" {
        "0".into()
    } else {
        s
    }
}

fn fit(text: &str, graphs: &[(usize, String)], rad: bool, note: u64) -> GraphView {
    let (x0, x1) = (-10.0, 10.0);
    let mut ys: Vec<f64> = graphs.iter().flat_map(|(l, _)| notes::graph(text, *l, rad, x0, x1, 201)).flatten().filter(|y| y.is_finite()).collect();
    ys.sort_by(|a, b| a.total_cmp(b));
    let (mut y0, mut y1) = if ys.is_empty() {
        (-10.0, 10.0)
    } else {
        let lo = ys[(ys.len() as f64 * 0.05) as usize];
        let hi = ys[((ys.len() as f64 * 0.95) as usize).min(ys.len() - 1)];
        (lo, hi)
    };
    if y1 - y0 < 1e-9 {
        y0 -= 1.0;
        y1 += 1.0;
    }
    let span = y1 - y0;
    if y0 > 0.0 && y0 < span {
        y0 = 0.0;
    }
    if y1 < 0.0 && -y1 < span {
        y1 = 0.0;
    }
    let pad = (y1 - y0) * 0.12;
    GraphView { note, x0, x1, y0: y0 - pad, y1: y1 + pad }
}

fn graph(app: &mut App, ui: &mut Ui, r: Rect, graphs: &[(usize, String)], live: bool) {
    let th = app.theme();
    let id = app.ov.note.unwrap_or(0);
    let rad = app.s.calc.rad;
    if app.ov.graph.is_none_or(|g| g.note != id) {
        app.ov.graph = Some(fit(&app.ov.note_text, graphs, rad, id));
    }
    let mut v = app.ov.graph.unwrap();
    let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let bg = if th.light { draw::mix(col(th.bg), Color32::BLACK, 0.04) } else { draw::mix(col(th.bg), Color32::WHITE, 0.07) };
    p.rect_filled(r, 14.0, bg);
    // interaction: drag pans, scroll / pinch zooms around the pointer, double-click refits
    if live {
        let resp = ui.interact(r, Id::unique("graph_view"), Sense::click_and_drag());
        let d = resp.drag_delta();
        if d != egui::Vec2::ZERO {
            let dx = -(d.x as f64) / r.width() as f64 * (v.x1 - v.x0);
            let dy = (d.y as f64) / r.height() as f64 * (v.y1 - v.y0);
            v.x0 += dx;
            v.x1 += dx;
            v.y0 += dy;
            v.y1 += dy;
        }
        if resp.double_clicked() {
            v = fit(&app.ov.note_text, graphs, rad, id);
        }
        if let Some(hp) = resp.hover_pos() {
            // the wheel zooms the graph instead of scrolling the note
            let (scroll, zoom) = ui.ctx().input_mut(|i| {
                let s = i.smooth_scroll_delta.y;
                i.smooth_scroll_delta = egui::Vec2::ZERO;
                (s, i.zoom_delta())
            });
            let k = (-(scroll as f64) * 0.004).exp() / zoom as f64;
            if (k - 1.0).abs() > 1e-6 {
                let fx = v.x0 + (hp.x - r.left()) as f64 / r.width() as f64 * (v.x1 - v.x0);
                let fy = v.y1 - (hp.y - r.top()) as f64 / r.height() as f64 * (v.y1 - v.y0);
                v.x0 = fx + (v.x0 - fx) * k;
                v.x1 = fx + (v.x1 - fx) * k;
                v.y0 = fy + (v.y0 - fy) * k;
                v.y1 = fy + (v.y1 - fy) * k;
            }
        }
        app.ov.graph = Some(v);
    }
    let sx = |x: f64| r.left() + ((x - v.x0) / (v.x1 - v.x0)) as f32 * r.width();
    let sy = |y: f64| r.bottom() - ((y - v.y0) / (v.y1 - v.y0)) as f32 * r.height();
    let grid_c = fade(col(th.panel_dim), 0.22);
    let axis_c = fade(col(th.panel_dim), 0.8);
    let lab_f = fonts::ui(400, 11.0);
    let dim = col(th.panel_dim);
    let stx = step(v.x1 - v.x0, (r.width() / 70.0) as f64);
    let sty = step(v.y1 - v.y0, (r.height() / 50.0) as f64);
    let ax_y = sy(0.0).clamp(r.top() + 2.0, r.bottom() - 14.0);
    let ax_x = sx(0.0).clamp(r.left() + 2.0, r.right() - 30.0);
    let mut x = (v.x0 / stx).ceil() * stx;
    while x <= v.x1 {
        let px = sx(x);
        p.line_segment([pos2(px, r.top()), pos2(px, r.bottom())], Stroke::new(0.5, grid_c));
        if x.abs() > stx * 1e-6 && px > r.left() + 14.0 && px < r.right() - 14.0 {
            draw::text(&p, pos2(px, ax_y + 3.0), Align2::CENTER_TOP, &fmt_tick(x, stx), lab_f.clone(), dim);
        }
        x += stx;
    }
    let mut y = (v.y0 / sty).ceil() * sty;
    while y <= v.y1 {
        let py = sy(y);
        p.line_segment([pos2(r.left(), py), pos2(r.right(), py)], Stroke::new(0.5, grid_c));
        if y.abs() > sty * 1e-6 && py > r.top() + 30.0 && py < r.bottom() - 10.0 {
            draw::text(&p, pos2(ax_x + 4.0, py), Align2::LEFT_CENTER, &fmt_tick(y, sty), lab_f.clone(), dim);
        }
        y += sty;
    }
    p.line_segment([pos2(r.left(), sy(0.0)), pos2(r.right(), sy(0.0))], Stroke::new(1.0, axis_c));
    p.line_segment([pos2(sx(0.0), r.top()), pos2(sx(0.0), r.bottom())], Stroke::new(1.0, axis_c));
    // curves
    let n = (r.width() as usize / 2).clamp(50, 800);
    let span = v.y1 - v.y0;
    for (gi, (line, label)) in graphs.iter().enumerate() {
        let first = if th.family == calc_core::theme::Family::Modern26 { col(0x3A82F7FF) } else { col(th.accent) };
        let c = if gi == 0 { first } else { col(CURVES[gi % CURVES.len()]) };
        let ys = notes::graph(&app.ov.note_text, *line, rad, v.x0, v.x1, n);
        let mut seg: Vec<egui::Pos2> = Vec::new();
        let mut prev: Option<f64> = None;
        let flush = |seg: &mut Vec<egui::Pos2>| {
            if seg.len() > 1 {
                p.add(Shape::line(std::mem::take(seg), Stroke::new(2.5, c)));
            }
            seg.clear();
        };
        for (i, yv) in ys.iter().enumerate() {
            let xv = v.x0 + (v.x1 - v.x0) * i as f64 / (n - 1) as f64;
            match yv {
                Some(yy) if yy.is_finite() && (yy - v.y0).abs() < span * 50.0 => {
                    if prev.is_some_and(|q| (yy - q).abs() > span * 3.0) {
                        flush(&mut seg);
                    }
                    seg.push(pos2(sx(xv), sy(*yy)));
                    prev = Some(*yy);
                }
                _ => {
                    flush(&mut seg);
                    prev = None;
                }
            }
        }
        flush(&mut seg);
        // legend
        let ly = r.top() + 16.0 + gi as f32 * 20.0;
        p.circle_filled(pos2(r.left() + 16.0, ly), 4.0, c);
        draw::text(&p, pos2(r.left() + 26.0, ly), Align2::LEFT_CENTER, label, fonts::ui(500, 12.0), col(th.panel_text));
    }
}
