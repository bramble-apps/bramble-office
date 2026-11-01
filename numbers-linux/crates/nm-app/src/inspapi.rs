//! The inspector as data, for front ends that draw it themselves (the Flutter app, ffi.rs):
//! `state` says which pane shows and every control's value, as the desktop inspector would
//! show them now; `set` does what changing one of them does there (the same edits as
//! inspector.rs, tblinsp.rs, objinsp.rs). Keys are "<pane>.<control>". The object panes are the
//! suite's (Letters' inspapi, on the same kn-model), so the front ends draw them alike.

use nm_core::kn_format::ObjId;
use nm_core::kn_model::{self, Color};
use serde_json::{Value, json};

use crate::app::{App, InspTab};
use crate::canvas::commit;

/// Which pane the inspector shows, and its values.
pub fn state(app: &mut App) -> Value {
    if let Some(id) = app.canvas.obj.filter(|o| app.table(*o).is_none()) {
        return object_state(app, id, app.canvas.editing.is_some());
    }
    let Some(sel) = app.sel else { return sheet_state(app) };
    if app.insp_tab == InspTab::Organize {
        return json!({ "pane": "organize", "data": crate::api::organize_state(app, sel.table) });
    }
    cells_state(app, sel.table)
}

/// Changes a control: `key` as `state` names it, `v` its new value (or what was picked).
pub fn set(app: &mut App, ctx: &egui::Context, key: &str, v: &Value) {
    let (pane, k) = key.split_once('.').unwrap_or((key, ""));
    match pane {
        "obj" => set_object(app, k, v),
        "style" => set_style(app, k, v),
        "arrange" => set_arrange(app, ctx, k, v),
        "image" => set_image(app, k, v),
        "chart" => set_chart(app, k, v),
        "table" => set_table(app, k, v),
        "media" => set_media(app, k, v),
        "cell" => set_cell(app, k, v),
        "celltext" => set_cell_text(app, k, v),
        "objtext" => set_obj_text(app, ctx, k, v),
        "sheet" => set_sheet(app, k, v),
        "organize" => crate::api::organize_set(app, k, v),
        _ => {}
    }
}

// ------------------------------------------------------------------ the sheet (nothing selected)

/// Numbers' Sheet inspector: its name, its tab colour, Duplicate / Hide / Delete.
fn sheet_state(app: &App) -> Value {
    let Some(s) = app.sheets().get(app.sheet).cloned() else { return json!({ "pane": "nothing" }) };
    let color = app.wb.as_ref().and_then(|w| nm_core::sheets::tab_color(w.file(), s.id));
    let visible = app.sheets().iter().filter(|s| !s.hidden).count();
    json!({ "pane": "sheet", "name": s.name, "tabColor": color.map(col), "visible": visible })
}

fn set_sheet(app: &mut App, k: &str, v: &Value) {
    let i = app.sheet;
    let Some(id) = app.sheets().get(i).map(|s| s.id) else { return };
    match k {
        "name" => {
            let n = v.as_str().unwrap_or("").trim().to_string();
            if !n.is_empty() {
                app.edit("Rename Sheet", |d, h| nm_core::sheets::rename_sheet(d, h, id, &n));
            }
        }
        "tabColor" => {
            let c = to_col(v);
            if let Some(Err(e)) = app.edit("Change Tab Color", |d, h| nm_core::sheets::set_tab_color(d, h, id, c)) {
                app.notify(format!("Tab Color: {e}"));
            }
        }
        "duplicate" => app.duplicate_sheet(i),
        "hide" => app.hide_sheet(i),
        "delete" => {
            if app.sheets().len() > 1 {
                app.edit("Delete Sheet", |d, h| nm_core::sheets::delete_sheet(d, h, i));
                app.sel = None;
                app.sheet = usize::MAX;
                let n = app.sheets().len();
                app.go_to_sheet(i.min(n.saturating_sub(1)));
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ cells' Text (inspector.rs cell_text)

fn cell_text_state(app: &mut App) -> Value {
    let Some(sel) = app.sel else { return Value::Null };
    let Some(st) = app.table(sel.table).and_then(|(_, td)| td.cell_styles.get((sel.cursor.0 * td.cols + sel.cursor.1) as usize).cloned()) else { return Value::Null };
    if app.font_names.is_none() {
        app.font_names = Some(crate::editing::font_names(app));
    }
    let run = &st.text;
    let para = &st.para;
    let align = match st.align {
        kn_model::Align::Left => 0,
        kn_model::Align::Center => 1,
        kn_model::Align::Right => 2,
        kn_model::Align::Justify => 3,
        kn_model::Align::Natural => 4,
    };
    let valign = match st.valign {
        kn_model::VAlign::Top => 0,
        kn_model::VAlign::Middle => 1,
        kn_model::VAlign::Bottom => 2,
    };
    let lines = match para.line_spacing {
        kn_model::LineSpacing::Relative(a) => a,
        _ => 1.0,
    };
    json!({
        "family": kn_render::fonts::parse_ps_name(&run.font_name).0,
        "fonts": app.font_names.clone().unwrap_or_default(),
        "size": run.size, "bold": run.bold, "italic": run.italic, "underline": run.underline, "strike": run.strike,
        "color": col(run.color),
        "align": align, "valign": valign, "wrap": st.wrap,
        "inset": st.padding[0],
        "first": para.first_line_indent, "left": para.left_indent, "right": para.right_indent,
        "lines": lines, "before": para.space_before, "after": para.space_after,
    })
}

fn set_cell_text(app: &mut App, k: &str, v: &Value) {
    use nm_core::kn_model::format::TextProp;
    use nm_core::kn_model::tablecells::CellAlign;
    let Some(sel) = app.sel else { return };
    let t = sel.table;
    let b = app.with_merges(t, sel.block());
    let block = (app.mrow(t, b.0).unwrap_or(b.0), b.1, app.mrow(t, b.2).unwrap_or(b.2), b.3);
    let run = app.table(t).and_then(|(_, td)| td.cell_styles.get((sel.cursor.0 * td.cols + sel.cursor.1) as usize).map(|s| s.text.clone())).unwrap_or_default();
    commit(app);
    let n = num(v);
    let prop: Option<(&str, TextProp)> = match k {
        "family" => v.as_str().map(|f| ("Font", TextProp::Font(f.replace(' ', "")))),
        "size" => Some(("Font Size", TextProp::Size(n.clamp(1.0, 1000.0).round()))),
        "bold" => Some(("Bold", TextProp::Bold(!run.bold))),
        "italic" => Some(("Italic", TextProp::Italic(!run.italic))),
        "underline" => Some(("Underline", TextProp::Underline(!run.underline))),
        "strike" => Some(("Strikethrough", TextProp::Strike(!run.strike))),
        "color" => to_col(v).map(|c| ("Text Color", TextProp::Color(c))),
        "first" => Some(("First Line Indent", TextProp::FirstIndent(n))),
        "left" => Some(("Left Indent", TextProp::LeftIndent(n.max(0.0)))),
        "right" => Some(("Right Indent", TextProp::RightIndent(n.max(0.0)))),
        "lines" => Some(("Line Spacing", TextProp::LineSpacing(n.clamp(0.5, 10.0)))),
        "before" => Some(("Space Before", TextProp::SpaceBefore(n.max(0.0)))),
        "after" => Some(("Space After", TextProp::SpaceAfter(n.max(0.0)))),
        _ => None,
    };
    let res: Option<Result<(), String>> = if let Some((label, p)) = prop {
        app.edit(label, |d, h| d.set_cells_text_prop(h, t, block, &p).map(|_| ()).map_err(|e| e.to_string()))
    } else {
        match k {
            "align" => {
                let a = [CellAlign::Left, CellAlign::Center, CellAlign::Right, CellAlign::Justify, CellAlign::Auto][(v.as_u64().unwrap_or(0) as usize).min(4)];
                app.edit("Alignment", |d, h| d.set_cells_alignment(h, t, block, a).map(|_| ()).map_err(|e| e.to_string()))
            }
            "valign" => {
                let a = [kn_model::VAlign::Top, kn_model::VAlign::Middle, kn_model::VAlign::Bottom][(v.as_u64().unwrap_or(0) as usize).min(2)];
                app.edit("Alignment", |d, h| d.set_cells_valign(h, t, block, a).map(|_| ()).map_err(|e| e.to_string()))
            }
            "wrap" => {
                let w = on_of(v);
                app.edit("Wrap Text", |d, h| d.set_cells_wrap(h, t, block, w).map(|_| ()).map_err(|e| e.to_string()))
            }
            "inset" => {
                let p = [n.clamp(0.0, 200.0); 4];
                app.edit("Text Inset", |d, h| d.set_cells_padding(h, t, block, p).map(|_| ()).map_err(|e| e.to_string()))
            }
            _ => None,
        }
    };
    if let Some(Err(e)) = res {
        app.notify(e);
    }
}

fn col(c: Color) -> Value {
    json!([c.r, c.g, c.b, c.a])
}

fn to_col(v: &Value) -> Option<Color> {
    let a = v.as_array()?;
    let f = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(1.0) as f32;
    Some(Color { r: f(0), g: f(1), b: f(2), a: f(3) })
}

fn num(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}


fn object_tabs(d: &kn_model::Drawable) -> Vec<&'static str> {
    use kn_model::DrawableKind;
    match &d.kind {
        DrawableKind::Shape(s) if s.is_line => vec!["Style", "Arrange"],
        DrawableKind::Shape(_) => vec!["Style", "Text", "Arrange"],
        DrawableKind::Image(im) if im.gallery.len() > 1 => vec!["Gallery", "Arrange"],
        DrawableKind::Image(im) if im.web_video.is_some() => vec!["Web Video", "Arrange"],
        DrawableKind::Image(_) => vec!["Style", "Image", "Arrange"],
        DrawableKind::Movie(m) if m.audio_only => vec!["Audio", "Arrange"],
        DrawableKind::Movie(_) => vec!["Style", "Movie", "Arrange"],
        DrawableKind::Chart(_) => vec!["Chart", "Axis", "Series", "Arrange"],
        DrawableKind::Table(_) => vec!["Table", "Arrange"],
        _ => vec!["Arrange"],
    }
}

fn object_state(app: &mut App, id: ObjId, text_first: bool) -> Value {
    let Some(d) = app.drawables.iter().find(|d| d.id == id).cloned() else { return json!({ "pane": "nothing" }) };
    let tabs = object_tabs(&d);
    if text_first && app.obj_tab == 0 && tabs.contains(&"Text") {
        app.obj_tab = 1;
    }
    let tab = app.obj_tab.min(tabs.len() - 1);
    let mut out = json!({ "pane": "object", "tabs": tabs, "tab": tab, "kind": tabs[tab] });
    let body = match tabs[tab] {
        "Style" => style_state(app, &d),
        "Text" => obj_text_state(app),
        "Arrange" => arrange_state(app, &d),
        "Image" => image_state(app, &d),
        "Chart" | "Axis" | "Series" => chart_state(app, &d, tabs[tab]),
        "Table" => table_state(app, &d),
        "Movie" | "Audio" => movie_state(app, &d),
        "Gallery" => gallery_state(app, &d),
        "Web Video" => web_video_state(&d),
        _ => Value::Null,
    };
    out["data"] = body;
    out
}

fn set_object(app: &mut App, k: &str, v: &Value) {
    use crate::app::InspTab;
    match k {
        "tab" => app.obj_tab = v.as_u64().unwrap_or(0) as usize,
        // the cells' tabs: Table / Cell / Text / Arrange
        "cellTab" => app.insp_tab = [InspTab::Table, InspTab::Cell, InspTab::Text, InspTab::Arrange][(v.as_u64().unwrap_or(0) as usize).min(3)],
        _ => {}
    }
}

/// Table cells selected: Table / Cell / Text / Arrange (inspector.rs; Text shows the Cell pane).
fn cells_state(app: &mut App, table: ObjId) -> Value {
    use crate::app::InspTab;
    let order = [InspTab::Table, InspTab::Cell, InspTab::Text, InspTab::Arrange];
    let tab = order.iter().position(|t| *t == app.insp_tab).unwrap_or(0);
    let Some(d) = app.table(table).map(|(d, _)| d.clone()) else { return json!({ "pane": "nothing" }) };
    let (kind, data) = match tab {
        0 => ("Table", table_state(app, &d)),
        2 => ("CellText", cell_text_state(app)),
        3 => ("Arrange", arrange_state(app, &d)),
        _ => ("Cell", cell_state(app, &d)),
    };
    json!({ "pane": "object", "cells": true, "tabs": ["Table", "Cell", "Text", "Arrange"], "tab": tab, "kind": kind, "data": data })
}

// ------------------------------------------------------------------ Table (tblinsp table_panel)

fn table_state(app: &mut App, d: &kn_model::Drawable) -> Value {
    use kn_model::tablelook::Gridline;
    let kn_model::DrawableKind::Table(t) = &d.kind else { return Value::Null };
    let id = d.id;
    let Some(doc) = app.wb.as_ref().map(|w| &w.doc) else { return Value::Null };
    let presets: Vec<Value> = nm_core::styles::table_style_presets(doc)
        .into_iter()
        .map(|(p, n)| {
            let l = doc.table_preset_look(n);
            json!({ "id": p, "headerRow": l.header_row.map(col), "headerCol": l.header_col.map(col), "footer": l.footer.map(col), "body": l.body.map(col), "band": l.band.map(col), "line": l.line.map(col), "dashed": l.dashed })
        })
        .collect();
    let caption = doc.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).is_some_and(|dm| dm.bool("caption_hidden") == Some(false) && dm.ref_id("caption").is_some_and(|c| doc.file.type_name(c) == "TSA.CaptionInfoArchive"));
    let (r0, c0, _, _) = match crate::tblinsp::range(app).filter(|r| r.0 == id) {
        Some((_, a, b, c, d)) => (a, b, c, d),
        None => (0, 0, t.rows.saturating_sub(1), t.cols.saturating_sub(1)),
    };
    let grid: Vec<bool> = [Gridline::BodyRows, Gridline::BodyColumns, Gridline::HeaderRow, Gridline::HeaderColumn, Gridline::Footer].iter().map(|g| doc.table_gridline(id, *g)).collect();
    let band = match doc.table_band_fill(id) {
        Some(kn_model::Fill::Color(c)) => c,
        _ => Color { r: 0.9, g: 0.9, b: 0.92, a: 1.0 },
    };
    json!({
        "presets": presets,
        "title": doc.table_name_shown(id), "caption": caption,
        "headerRows": t.header_rows, "headerCols": t.header_cols, "footerRows": t.footer_rows,
        "rows": t.rows, "cols": t.cols,
        "fontSize": doc.table_font_size(id),
        "outline": stroke_json(&doc.table_outline(id)),
        "gridlines": grid,
        "banded": doc.table_banded(id), "bandColor": col(band),
        "height": t.row_heights.get(r0 as usize).copied().unwrap_or(30.0),
        "width": t.col_widths.get(c0 as usize).copied().unwrap_or(30.0),
    })
}

fn set_table(app: &mut App, k: &str, v: &Value) {
    use kn_model::tablelook::Gridline;
    use kn_model::tablerows::{Axis, HeaderKind};
    let id = match app.sel {
        Some(s) => s.table,
        None => match app.canvas.obj {
            Some(i) => i,
            None => return,
        },
    };
    let Some(kn_model::DrawableKind::Table(t)) = app.drawables.iter().find(|d| d.id == id).map(|d| d.kind.clone()) else { return };
    let (r0, c0, r1, c1) = match crate::tblinsp::range(app).filter(|r| r.0 == id) {
        Some((_, a, b, c, d)) => (a, b, c, d),
        None => (0, 0, t.rows.saturating_sub(1), t.cols.saturating_sub(1)),
    };
    let on = on_of(v);
    let slide = app.current_slide().map(|s| s.slide);
    let caption_slide = slide;
    let Some(doc) = app.wb.as_mut().map(|w| &mut w.doc) else { return };
    doc.begin(&mut app.hist, "Table");
    let h = &mut app.hist;
    match k {
        "preset" => {
            if let Some(p) = v.as_u64() {
                doc.apply_table_style_preset(h, id, p as ObjId);
            }
        }
        "savePreset" => {
            let _ = doc.save_table_style_preset(h, id);
        }
        "title" => {
            doc.set_table_name_shown(h, id, on);
        }
        "caption" => {
            if let Some(s) = caption_slide {
                doc.set_caption(h, s, id, false, on);
            }
        }
        "headerRows" | "headerCols" | "footerRows" => {
            let kind = match k {
                "headerRows" => HeaderKind::Rows,
                "headerCols" => HeaderKind::Columns,
                _ => HeaderKind::Footer,
            };
            let _ = doc.set_table_header_count(h, id, kind, num(v).clamp(0.0, 5.0) as u32);
        }
        "rows" | "cols" => {
            let (axis, cur) = if k == "rows" { (Axis::Row, t.rows) } else { (Axis::Column, t.cols) };
            let n = num(v).clamp(1.0, 999.0) as u32;
            for i in cur..n {
                let _ = doc.table_insert(h, id, axis, i, i - 1);
            }
            for i in (n..cur).rev() {
                let _ = doc.table_delete(h, id, axis, i);
            }
        }
        "fontBigger" => {
            doc.bump_table_font_size(h, id, 1.0);
        }
        "fontSmaller" => {
            doc.bump_table_font_size(h, id, -1.0);
        }
        "outline" => {
            let s = stroke_from(v);
            doc.set_table_outline(h, id, s.as_ref());
        }
        "gridline" => {
            let g = [Gridline::BodyRows, Gridline::BodyColumns, Gridline::HeaderRow, Gridline::HeaderColumn, Gridline::Footer][(v.as_u64().unwrap_or(0) as usize).min(4)];
            let cur = doc.table_gridline(id, g);
            doc.set_table_gridline(h, id, g, !cur);
        }
        "banded" => {
            doc.set_table_banded(h, id, on);
        }
        "bandColor" => {
            if let Some(c) = to_col(v) {
                doc.set_table_band_color(h, id, c);
            }
        }
        "height" | "width" => {
            let rows = k == "height";
            let (from, to) = if rows { (r0, r1) } else { (c0, c1) };
            let set: Vec<(u32, f32)> = (from..=to).map(|i| (i, num(v).clamp(8.0, 2000.0))).collect();
            doc.set_table_sizes(h, id, rows, &set);
        }
        "fitHeight" | "fitWidth" => {
            let rows = k == "fitHeight";
            let idx: Vec<u32> = if rows { (r0..=r1).collect() } else { (c0..=c1).collect() };
            doc.fit_table_to_content(h, id, rows, &idx);
        }
        "distributeRows" => {
            doc.distribute_table(h, id, true);
        }
        "distributeCols" => {
            doc.distribute_table(h, id, false);
        }
        _ => {}
    }
    doc.end(&mut app.hist);
    if let Some(s) = slide {
        app.bump(s);
    }
}

// ------------------------------------------------------------------ Movie / Audio / Gallery / Web Video (mediainsp)

fn movie_state(app: &mut App, d: &kn_model::Drawable) -> Value {
    let kn_model::DrawableKind::Movie(m) = &d.kind else { return Value::Null };
    let len = m.movie_data.and_then(|data| app.doc().and_then(|doc| kn_ui::media::cache_file(doc, data))).and_then(|(p, _)| kn_ui::media::duration(&p)).unwrap_or(0.0) as f32;
    json!({
        "audioOnly": m.audio_only, "volume": m.volume, "loop": m.loop_mode,
        "length": len, "start": m.start, "end": if m.end > 0.0 { m.end } else { len }, "poster": m.poster_time,
    })
}

fn gallery_state(app: &mut App, d: &kn_model::Drawable) -> Value {
    let kn_model::DrawableKind::Image(im) = &d.kind else { return Value::Null };
    let doc = app.doc();
    let caption = doc.and_then(|doc| doc.file.obj(d.id)).and_then(|m| m.find_super("TSD.DrawableArchive")).is_some_and(|dm| dm.bool("caption_hidden") == Some(false));
    let shown = im.gallery_shown.min(im.gallery.len().saturating_sub(1));
    json!({
        "caption": caption,
        "mode": if im.gallery_caption_mode == 2 { 2 } else { 1 },
        "shown": shown,
        "items": im.gallery.iter().enumerate().map(|(k, it)| json!({
            "data": it.data_id,
            "name": it.data_id.and_then(|data| doc.and_then(|doc| doc.data_name(data)).map(crate::objinsp::shown_file_name)).unwrap_or_else(|| format!("Image {}", k + 1)),
            "description": it.description,
        })).collect::<Vec<_>>(),
    })
}

fn web_video_state(d: &kn_model::Drawable) -> Value {
    let kn_model::DrawableKind::Image(im) = &d.kind else { return Value::Null };
    let Some(w) = &im.web_video else { return Value::Null };
    json!({ "title": w.title, "author": w.author, "url": w.url })
}

fn set_media(app: &mut App, k: &str, v: &Value) {
    use kn_model::imageedit::MovieProp;
    let Some(id) = app.canvas.obj else { return };
    let Some(d) = app.drawables.iter().find(|d| d.id == id).cloned() else { return };
    match (&d.kind, k) {
        (kn_model::DrawableKind::Movie(m), _) => {
            let audio = m.audio_only;
            let change = match k {
                "volume" => Some(MovieProp::Volume(num(v).clamp(0.0, 1.0))),
                "loop" => Some(MovieProp::Loop(v.as_u64().unwrap_or(0) as u32)),
                "trim" => {
                    let (a, b) = (v.get(0).map(num).unwrap_or(0.0), v.get(1).map(num).unwrap_or(0.0));
                    Some(MovieProp::Trim(a.min(b - 0.1).max(0.0), b))
                }
                "poster" => {
                    let t = num(v);
                    let frame = m.movie_data.and_then(|data| app.doc().and_then(|doc| kn_ui::media::cache_file(doc, data))).and_then(|(p, _)| kn_ui::media::frame_jpeg(&p, t as f64));
                    match frame {
                        Some(jpeg) => {
                            if let Some(Err(e)) = app.edit("Poster Frame", |doc, h| doc.set_movie_poster(h, id, t, jpeg)) {
                                app.notify(format!("Couldn't set the poster frame: {e}"));
                            }
                        }
                        None => app.notify("Couldn't read that frame (is ffmpeg installed?)"),
                    }
                    None
                }
                _ => None,
            };
            if let Some(c) = change {
                app.edit(if audio { "Audio" } else { "Movie" }, |doc, h| doc.set_movie_prop(h, id, &c));
            }
        }
        (kn_model::DrawableKind::Image(im), _) => {
            let shown = im.gallery_shown.min(im.gallery.len().saturating_sub(1));
            let mode = if im.gallery_caption_mode == 2 { 2u8 } else { 1 };
            match k {
                "caption" => {
                    let on = on_of(v);
                    app.edit("Gallery", |doc, h| doc.set_gallery_caption(h, id, on, mode));
                }
                "mode" => {
                    let m = v.as_u64().unwrap_or(1) as u8;
                    app.edit("Gallery", |doc, h| doc.set_gallery_caption(h, id, true, m));
                }
                "show" => show_image(app, id, v.as_u64().unwrap_or(0) as usize),
                "add" => {
                    let pics: Vec<(Vec<u8>, String)> = v.as_array().map(|a| a.iter().filter_map(|p| {
                        let p = std::path::Path::new(p.as_str()?);
                        Some((std::fs::read(p).ok()?, p.file_name()?.to_string_lossy().to_string()))
                    }).collect()).unwrap_or_default();
                    if !pics.is_empty() {
                        if let Some(Err(e)) = app.edit("Add Images", |doc, h| doc.make_gallery(h, id, pics)) {
                            app.notify(format!("Couldn't add the pictures: {e}"));
                        }
                    }
                }
                "remove" => {
                    let n = im.gallery.len();
                    app.edit("Gallery", |doc, h| {
                        doc.remove_gallery_image(h, id, shown);
                    });
                    show_image(app, id, shown.min(n.saturating_sub(2)));
                }
                "description" => {
                    if let Some(item) = im.gallery.get(shown).map(|it| it.id) {
                        let text = v.as_str().unwrap_or("").to_string();
                        app.edit("Gallery", |doc, h| doc.set_gallery_description(h, item, &text));
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ Cell (tblinsp cell_panel)

fn format_json(f: &kn_model::tablefmt::CellFormat) -> Value {
    use kn_model::tablefmt::CellFormat as F;
    match f {
        F::Automatic => json!({ "kind": 0 }),
        F::Number { decimals, thousands, negative } => json!({ "kind": 1, "decimals": decimals, "thousands": thousands, "negative": negative }),
        F::Currency { code, decimals, thousands, negative, accounting } => json!({ "kind": 2, "code": code, "decimals": decimals, "thousands": thousands, "negative": negative, "accounting": accounting }),
        F::Percent { decimals, thousands, negative } => json!({ "kind": 3, "decimals": decimals, "thousands": thousands, "negative": negative }),
        F::Fraction { accuracy } => json!({ "kind": 4, "accuracy": accuracy }),
        F::Base { base, places, minus } => json!({ "kind": 5, "base": base, "places": places, "minus": minus }),
        F::Scientific { decimals } => json!({ "kind": 6, "decimals": decimals }),
        F::Text => json!({ "kind": 7 }),
        F::DateTime { pattern } => {
            let d = kn_model::tablefmt::DATE_PATTERNS.iter().filter(|p| pattern.starts_with(*p)).max_by_key(|p| p.len()).copied();
            let rest = d.map(|d| pattern[d.len()..].trim()).unwrap_or(pattern.as_str());
            let t = kn_model::tablefmt::TIME_PATTERNS.iter().find(|p| **p == rest || rest.replace('\u{202f}', " ") == **p).copied();
            json!({ "kind": 8, "date": d, "time": t })
        }
        F::Duration { style, largest, smallest, automatic } => json!({ "kind": 9, "style": style, "largest": largest, "smallest": smallest, "automatic": automatic }),
        F::Custom { .. } => json!({ "kind": -1 }),
    }
}

fn format_from(v: &Value) -> kn_model::tablefmt::CellFormat {
    use kn_model::tablefmt::CellFormat as F;
    let u = |k: &str| v.get(k).and_then(Value::as_u64).map(|x| x as u32);
    let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    let dec = || v.get("decimals").and_then(Value::as_u64).map(|x| x as u32);
    match v.get("kind").and_then(Value::as_i64).unwrap_or(0) {
        1 => F::Number { decimals: dec(), thousands: b("thousands"), negative: u("negative").unwrap_or(0) },
        2 => F::Currency { code: v.get("code").and_then(Value::as_str).unwrap_or("USD").to_string(), decimals: dec(), thousands: b("thousands"), negative: u("negative").unwrap_or(0), accounting: b("accounting") },
        3 => F::Percent { decimals: dec(), thousands: b("thousands"), negative: u("negative").unwrap_or(0) },
        4 => F::Fraction { accuracy: u("accuracy").unwrap_or(u32::MAX - 2) },
        5 => F::Base { base: u("base").unwrap_or(2), places: u("places").unwrap_or(0), minus: v.get("minus").and_then(Value::as_bool).unwrap_or(true) },
        6 => F::Scientific { decimals: dec() },
        7 => F::Text,
        8 => {
            let (d, t) = (v.get("date").and_then(Value::as_str), v.get("time").and_then(Value::as_str));
            F::DateTime {
                pattern: match (d, t) {
                    (Some(d), Some(t)) => format!("{d} {t}"),
                    (Some(d), None) => d.to_string(),
                    (None, Some(t)) => t.to_string(),
                    (None, None) => "M/d/yy".to_string(),
                },
            }
        }
        9 => {
            let (lg, sm) = (u("largest").unwrap_or(1), u("smallest").unwrap_or(32));
            F::Duration { style: u("style").unwrap_or(1), largest: lg, smallest: sm.max(lg), automatic: v.get("automatic").and_then(Value::as_bool).unwrap_or(true) }
        }
        _ => F::Automatic,
    }
}

fn cell_block(app: &App, id: ObjId, t: &kn_model::TableData) -> (u32, u32, u32, u32) {
    match crate::tblinsp::range(app).filter(|r| r.0 == id) {
        Some((_, r0, c0, r1, c1)) => (r0, c0, r1, c1),
        None => match crate::tblinsp::table_cursor(app).filter(|c| c.0 == id) {
            Some((_, r, c)) => (r, c, r, c),
            None => (0, 0, t.rows.saturating_sub(1), t.cols.saturating_sub(1)),
        },
    }
}

thread_local! {
    /// The Border section's chosen edges and line (kept while the pane is open).
    static BORDER: std::cell::RefCell<(Option<u8>, Option<kn_model::format::StrokeSpec>)> = std::cell::RefCell::new((None, Some(kn_model::format::StrokeSpec { color: Color::BLACK, width: 1.0, dash: kn_model::format::Dash::Solid })));
}

fn cell_state(app: &mut App, d: &kn_model::Drawable) -> Value {
    let kn_model::DrawableKind::Table(t) = &d.kind else { return Value::Null };
    let id = d.id;
    let (r, c, _, _) = cell_block(app, id, t);
    let doc = app.wb.as_ref().map(|w| &w.doc);
    let cur = doc.map(|doc| doc.cell_format(id, r, c)).unwrap_or(kn_model::tablefmt::CellFormat::Automatic);
    let fill = t.cell_styles.get((r * t.cols + c) as usize).map(|s| s.fill.clone()).unwrap_or(kn_model::Fill::None);
    let sample = 789_332_889.0;
    let (edges, line) = BORDER.with(|b| b.borrow().clone());
    json!({
        "format": format_json(&cur),
        "currencies": kn_model::tablefmt::CURRENCIES,
        "fractions": kn_model::tablefmt::FRACTION_ACCURACIES.iter().map(|(v, n)| json!([v, n])).collect::<Vec<_>>(),
        "dates": kn_model::tablefmt::DATE_PATTERNS.iter().map(|p| json!([p, kn_model::table::format_date(sample, p)])).collect::<Vec<_>>(),
        "times": kn_model::tablefmt::TIME_PATTERNS.iter().map(|p| json!([p, kn_model::table::format_date(sample, p)])).collect::<Vec<_>>(),
        "fill": fill_json(&fill, doc),
        "edges": edges,
        "line": line.map(|s| json!({ "color": col(s.color), "width": s.width, "dash": format!("{:?}", s.dash) })),
    })
}

fn set_cell(app: &mut App, k: &str, v: &Value) {
    use kn_model::tablefmt::BorderEdges as E;
    let Some(id) = app.sel.map(|s| s.table).or(app.canvas.obj) else { return };
    let Some(kn_model::DrawableKind::Table(t)) = app.drawables.iter().find(|d| d.id == id).map(|d| d.kind.clone()) else { return };
    let block = cell_block(app, id, &t);
    let edges_of = |i: u8| [E::All, E::Inside, E::Outside, E::Left, E::Center, E::Right, E::Top, E::Middle, E::Bottom][(i as usize).min(8)];
    let mut res: Result<(), String> = Ok(());
    let slide = app.current_slide().map(|s| s.slide);
    let mut run = |app: &mut App, name: &str, f: &dyn Fn(&mut kn_model::Document, &mut kn_model::edit::History) -> Result<(), String>| {
        if let Some(doc) = app.wb.as_mut().map(|w| &mut w.doc) {
            doc.begin(&mut app.hist, name);
            res = f(doc, &mut app.hist);
            doc.end(&mut app.hist);
        }
    };
    match k {
        "format" => {
            let f = format_from(v);
            run(app, "Data Format", &|doc, h| doc.set_cells_format(h, id, block, &f).map(|_| ()).map_err(|e| e.to_string()));
        }
        "fill" => {
            if let Some(spec) = fill_from(v, app.wb.as_mut().map(|w| &mut w.doc)) {
                run(app, "Fill", &|doc, h| doc.set_cells_fill(h, id, block, Some(&spec)).map(|_| ()).map_err(|e| e.to_string()));
            }
        }
        "conditional" => crate::menus::run(app, &egui::Context::default(), "Conditional Highlighting…"),
        "edges" | "line" => {
            // pick the edges, then their line (tblinsp: no edges yet takes all of them)
            let (mut edges, mut line) = BORDER.with(|b| b.borrow().clone());
            if k == "edges" {
                edges = v.as_u64().map(|x| x as u8);
            } else {
                line = stroke_from(v);
                if edges.is_none() {
                    edges = Some(0);
                }
            }
            BORDER.with(|b| *b.borrow_mut() = (edges, line.clone()));
            if let Some(e) = edges.map(edges_of) {
                let l = line.clone();
                run(app, "Border", &|doc, h| doc.set_cells_border(h, id, block, e, l.as_ref()).map_err(|e| e.to_string()));
                app.notify(if line.is_none() { format!("Borders removed ({e:?})") } else { format!("Border set ({e:?})") });
            }
        }
        _ => {}
    }
    if let Err(e) = res {
        app.notify(e);
    }
    if let Some(s) = slide {
        app.bump(s);
    }
}

// ------------------------------------------------------------------ Arrange (objinsp arrange, inspector arrange_extra)

fn arrange_state(app: &mut App, d: &kn_model::Drawable) -> Value {
    let selected: Vec<ObjId> = app.selected_objs();
    let count = selected.len().max(1);
    let g = app.canvas.tiles.effective(d);
    json!({
        "count": count,
        "locked": d.locked,
        "group": count == 1 && matches!(d.kind, kn_model::DrawableKind::Group(_)),
        "w": g.w, "h": g.h, "x": g.x, "y": g.y, "angle": g.angle,
        "wordProcessing": false, "inline": false, "wrap": 0, "wrapMargin": 0.0,
    })
}

fn set_arrange(app: &mut App, ctx: &egui::Context, k: &str, v: &Value) {
    let Some(id) = app.canvas.obj else { return };
    let Some(d) = app.drawables.iter().find(|d| d.id == id).cloned() else { return };
    match k {
        "command" => {
            if let Some(c) = v.as_str() {
                app.command(ctx, c);
            }
        }
        "geometry" => {
            // {w | h | x | y | angle | vflip | hflip, constrain}
            let g0 = app.canvas.tiles.effective(&d);
            let mut g = g0;
            let f = |k: &str| v.get(k).map(num);
            if let Some(x) = f("w") {
                g.w = x.clamp(1.0, 100000.0);
            }
            if let Some(x) = f("h") {
                g.h = x.clamp(1.0, 100000.0);
            }
            if let Some(x) = f("x") {
                g.x = x.clamp(-100000.0, 100000.0);
            }
            if let Some(x) = f("y") {
                g.y = x.clamp(-100000.0, 100000.0);
            }
            if let Some(x) = f("angle") {
                g.angle = x.rem_euclid(360.0);
            }
            if v.get("vflip").is_some() {
                g.vflip = !g.vflip;
            }
            if v.get("hflip").is_some() {
                g.hflip = !g.hflip;
            }
            if v.get("constrain").and_then(Value::as_bool) == Some(true) && g0.w > 0.0 && g0.h > 0.0 {
                if g.w != g0.w {
                    g.h = g0.h * g.w / g0.w;
                } else if g.h != g0.h {
                    g.w = g0.w * g.h / g0.h;
                }
            }
            if g == g0 {
                return;
            }
            let slide = app.current_slide().map(|s| s.slide);
            let is_table = matches!(d.kind, kn_model::DrawableKind::Table(_));
            let merge = v.get("merge").and_then(Value::as_bool).unwrap_or(false);
            if let Some(doc) = app.wb.as_mut().map(|w| &mut w.doc) {
                if merge {
                    doc.begin_merge(&mut app.hist, "Change Geometry");
                } else {
                    doc.begin(&mut app.hist, "Change Geometry");
                }
                if is_table && (g.w != g0.w || g.h != g0.h) {
                    doc.scale_table_to(&mut app.hist, id, g.w, g.h);
                }
                doc.set_frame(&mut app.hist, id, kn_model::Geometry { auto_width: g0.auto_width && g.w == g0.w, auto_height: g0.auto_height && g.h == g0.h, ..g });
                doc.end(&mut app.hist);
            }
            if let Some(s) = slide {
                app.bump(s);
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ Format > Image (objinsp image_panel)

fn image_state(app: &mut App, d: &kn_model::Drawable) -> Value {
    let kn_model::DrawableKind::Image(i) = &d.kind else { return Value::Null };
    let doc = app.wb.as_ref().map(|w| &w.doc);
    let name = i.data_id.and_then(|did| doc.and_then(|doc| doc.data_name(did)).map(crate::objinsp::shown_file_name)).unwrap_or_else(|| "Image".into());
    let a = doc.map(|doc| doc.image_adjustments(d.id)).unwrap_or_default();
    json!({
        "name": name,
        "editingMask": app.mask_edit == Some(d.id),
        "masked": i.mask.is_some(),
        "exposure": a.exposure, "saturation": a.saturation, "enhance": a.enhance,
        "equation": i.equation,
        "description": d.alt_text.clone().unwrap_or_default(),
    })
}

fn set_image(app: &mut App, k: &str, v: &Value) {
    let Some(id) = app.canvas.obj else { return };
    let ctx = egui::Context::default();
    match k {
        "command" => {
            if let Some(c) = v.as_str() {
                app.command(&ctx, c);
            }
        }
        "replace" => {
            if let Some(p) = v.as_str() {
                app.replace_image(id, std::path::Path::new(p));
            }
        }
        "exposure" | "saturation" | "enhance" | "reset" => {
            let mut a = app.wb.as_ref().map(|w| w.doc.image_adjustments(id)).unwrap_or_default();
            match k {
                "exposure" => a.exposure = num(v).clamp(-100.0, 100.0) / 100.0,
                "saturation" => a.saturation = num(v).clamp(-100.0, 100.0) / 100.0,
                "enhance" => a.enhance = !a.enhance,
                _ => {
                    if a.is_default() {
                        return;
                    }
                    a = Default::default();
                }
            }
            crate::imageadj::set(app, id, a);
        }
        "description" => {
            let text = v.as_str().unwrap_or("").to_string();
            let slide = app.current_slide().map(|s| s.slide);
            if let Some(doc) = app.wb.as_mut().map(|w| &mut w.doc) {
                doc.begin(&mut app.hist, "Description");
                doc.set_description(&mut app.hist, id, &text);
                doc.end(&mut app.hist);
            }
            if let Some(s) = slide {
                app.bump(s);
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ Chart / Axis / Series (kn_ui chartpanes)

thread_local! {
    /// The Axis and Series panes' chosen axis (0 Y, 1 X, 2 Y2) and series.
    static CHART: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

/// (format kind 0 automatic / 1 number / 2 percentage / 3 currency, decimals) of a stored format.
fn read_format(f: Option<&nm_core::kn_format::Msg>) -> (usize, u32) {
    let Some(f) = f else { return (0, 0) };
    let dec = f.u32("decimal_places").filter(|d| *d < 200).unwrap_or(0);
    let kind = match f.u32("format_type") {
        Some(257) => 3,
        Some(258) => 2,
        Some(256) if f.u32("decimal_places").is_some_and(|d| d < 200) => 1,
        _ => 0,
    };
    (kind, dec)
}

fn format_of(kind: u64, decimals: u32) -> kn_model::tablefmt::CellFormat {
    use kn_model::tablefmt::CellFormat;
    match kind {
        1 => CellFormat::Number { decimals: Some(decimals), thousands: true, negative: 0 },
        2 => CellFormat::Percent { decimals: Some(decimals), thousands: false, negative: 0 },
        3 => CellFormat::Currency { code: "USD".into(), decimals: Some(decimals), thousands: true, negative: 0, accounting: false },
        _ => CellFormat::Automatic,
    }
}

fn chart_state(app: &mut App, d: &kn_model::Drawable, tab: &str) -> Value {
    use kn_model::SeriesKind;
    let kn_model::DrawableKind::Chart(c) = &d.kind else { return Value::Null };
    let Some(doc) = app.wb.as_ref().map(|w| &w.doc) else { return Value::Null };
    let (axis_i, series_i) = CHART.with(|s| s.get());
    match tab {
        "Chart" => {
            let presets: Vec<Value> = doc.chart_presets().iter().map(|p| json!({ "id": p, "colors": doc.chart_preset_colors(*p).iter().map(|c| col(*c)).collect::<Vec<_>>() })).collect();
            let bg = c.background.as_ref().and_then(|f| match f {
                kn_model::Fill::Color(c) => Some(*c),
                kn_model::Fill::Gradient { stops, .. } => stops.first().map(|s| s.1),
                _ => None,
            });
            json!({
                "summary": format!("{} series × {} categories", c.series.len(), c.categories.len()),
                "presets": presets, "preset": doc.chart_preset(d.id),
                "font": c.value_label_style.font_name,
                "types": kn_model::chartdata::CHART_TYPES.iter().map(|t| json!([t.0, t.1])).collect::<Vec<_>>(),
                "type": c.chart_type,
                "legend": c.show_legend, "title": c.title,
                "border": c.show_border, "borderStroke": c.border.as_ref().map(|s| json!({ "color": col(s.color), "width": s.width })),
                "background": bg.map(col),
                "bars": c.series.iter().any(|s| matches!(s.kind, SeriesKind::Column | SeriesKind::Bar)),
                "cornerRadius": c.corner_radius * 100.0, "outerOnly": c.corner_outer_only,
                "barGap": c.bar_gap * 100.0, "setGap": c.set_gap * 100.0,
                "donut": c.chart_type.to_ascii_lowercase().contains("donut"), "innerRadius": c.inner_radius * 100.0,
            })
        }
        "Axis" => {
            let two = c.value_axes.len() > 1 && c.chart_type.starts_with("twoAxis");
            let mut names = vec!["Value (Y)", "Category (X)"];
            if two {
                names.push("Value (Y2)");
            }
            let ai = axis_i.min(names.len() - 1);
            let xy = c.chart_type.starts_with("scatter") || c.chart_type.starts_with("bubble");
            let axis = match ai {
                1 => c.category_axis.clone(),
                2 => c.value_axes.get(1).cloned().unwrap_or_default(),
                _ => c.value_axes.first().cloned().unwrap_or_default(),
            };
            let is_value = ai != 1 || xy;
            let (fk, fd) = read_format(axis.number_format.as_ref());
            json!({
                "names": names, "axis": ai, "isValue": is_value,
                "title": axis.title.is_some(), "labels": axis.show_labels, "line": axis.show_line,
                "angle": axis.label_angle, "formatKind": fk, "decimals": fd,
                "log": axis.log, "min": axis.min, "max": axis.max,
                "major": axis.intervals.unwrap_or(4), "minor": axis.minor_intervals.unwrap_or(2),
                "gridlines": axis.show_gridlines, "minorGridlines": axis.show_minor_gridlines,
                "majorTicks": axis.show_major_ticks, "minorTicks": axis.show_minor_ticks,
                "gridStroke": axis.gridline.as_ref().map(|s| json!({ "color": col(s.color), "width": s.width })),
            })
        }
        _ => {
            if c.series.is_empty() {
                return json!({ "empty": true });
            }
            let i = series_i.min(c.series.len() - 1);
            let s = &c.series[i];
            let (fk, fd) = read_format(s.value_format.as_ref());
            json!({
                "series": i, "names": c.series.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
                "valueLabels": c.series.iter().any(|s| s.show_value_labels),
                "namesShown": c.series.iter().any(|s| s.show_series_names),
                "pieOrBubble": matches!(s.kind, SeriesKind::Pie | SeriesKind::Bubble),
                "pie": s.kind == SeriesKind::Pie,
                "lineKind": matches!(s.kind, SeriesKind::Line | SeriesKind::Scatter | SeriesKind::Radar),
                "formatKind": fk, "decimals": fd,
                "color": col(s.color),
                "stroke": s.stroke.as_ref().map(|k| json!({ "color": col(k.color), "width": k.width })),
                "labelDistance": s.label_distance.unwrap_or(62.0),
                "explosion": s.explosion * 100.0,
            })
        }
    }
}

fn set_chart(app: &mut App, k: &str, v: &Value) {
    use kn_model::chartdata::ChartFlag;
    use kn_model::chartstyle::{ChartPart, ChartValue};
    use kn_model::format::{Dash, FillSpec, StrokeSpec};
    let Some(id) = app.canvas.obj else { return };
    let Some(kn_model::DrawableKind::Chart(c)) = app.drawables.iter().find(|d| d.id == id).map(|d| d.kind.clone()) else { return };
    let (axis_i, series_i) = CHART.with(|s| s.get());
    match k {
        "axisPick" => return CHART.with(|s| s.set((v.as_u64().unwrap_or(0) as usize, series_i))),
        "seriesPick" => return CHART.with(|s| s.set((axis_i, v.as_u64().unwrap_or(0) as usize))),
        "editData" => return crate::chartedit::open(app, id),
        _ => {}
    }
    let on = on_of(v);
    let stroke = |v: &Value| StrokeSpec { color: v.get("color").and_then(to_col).unwrap_or(Color::BLACK), width: v.get("width").map(num).unwrap_or(1.0), dash: Dash::Solid };
    let xy = c.chart_type.starts_with("scatter") || c.chart_type.starts_with("bubble");
    let (part, which, flag_idx) = match axis_i {
        1 if xy => (ChartPart::ValueAxis(1), "value", None),
        1 => (ChartPart::CategoryAxis, "category", None),
        2 => (ChartPart::ValueAxis(1), "value", Some(1)),
        _ => (ChartPart::ValueAxis(0), "value", Some(0)),
    };
    let prop = |name: &str| format!("tschchartaxis{which}{name}");
    let si = series_i.min(c.series.len().saturating_sub(1));
    let n = c.series.len();
    app.edit("Chart", |d, h| match k {
        "preset" => {
            if let Some(p) = v.as_u64() {
                d.apply_chart_preset(h, id, p as ObjId);
            }
        }
        "font" => {
            d.set_chart_font(h, id, v.as_str(), 1.0);
        }
        "fontScale" => {
            d.set_chart_font(h, id, None, num(v));
        }
        "type" => {
            if let Some(t) = v.as_str() {
                d.set_chart_type(h, id, t);
            }
        }
        "legend" => {
            d.set_chart_legend(h, id, on);
        }
        "title" => {
            let t = if v.is_boolean() { on.then(|| c.title.clone().unwrap_or_else(|| "Title".into())) } else { v.as_str().map(str::to_string) };
            d.set_chart_title(h, id, t.as_deref());
        }
        "border" => {
            d.set_chart_flag(h, id, ChartFlag::Border(on));
        }
        "borderStroke" => {
            d.set_chart_prop(h, id, ChartPart::Chart, "tschchartinfodefaultborderstroke", &ChartValue::Stroke(Some(stroke(v))));
        }
        "background" => {
            let f = if v.is_boolean() { if on { FillSpec::Color(Color::rgba(1.0, 1.0, 1.0, 1.0)) } else { FillSpec::None } } else { to_col(v).map(FillSpec::Color).unwrap_or(FillSpec::None) };
            d.set_chart_background(h, id, &f);
        }
        "cornerRadius" => {
            d.set_chart_prop(h, id, ChartPart::Chart, "tschchartinfodefaultroundedcornerradius", &ChartValue::Float(num(v).clamp(0.0, 100.0) / 100.0));
        }
        "outerOnly" => {
            d.set_chart_prop(h, id, ChartPart::Chart, "tschchartinfodefaultroundedcornerouterendonly", &ChartValue::Bool(on));
        }
        "barGap" | "setGap" => {
            let name = if k == "barGap" { "tschchartinfodefaultinterbargap" } else { "tschchartinfodefaultintersetgap" };
            d.set_chart_prop(h, id, ChartPart::Chart, name, &ChartValue::Float(num(v).clamp(0.0, 500.0)));
        }
        "innerRadius" => {
            d.set_chart_prop(h, id, ChartPart::Chart, "tschchartinfodefaultinnerradius", &ChartValue::Float(num(v).clamp(0.0, 95.0) / 100.0));
        }
        "axisTitle" | "axisLabels" | "axisLine" => {
            let p = match k {
                "axisTitle" => "showtitle",
                "axisLabels" => "showlabels",
                _ => "showaxis",
            };
            d.set_chart_flag(h, id, ChartFlag::Axis(if axis_i == 1 { None } else { flag_idx }, p, on));
        }
        "angle" => {
            d.set_chart_prop(h, id, part, &prop("labelsorientation"), &ChartValue::Float(num(v)));
        }
        "axisFormat" => {
            let m = format_of(v.get("kind").and_then(Value::as_u64).unwrap_or(0), v.get("decimals").and_then(Value::as_u64).unwrap_or(0) as u32).msg();
            d.set_axis_number_format(h, id, part, &m);
        }
        "log" => {
            d.set_chart_prop(h, id, part, "tschchartaxisvaluescale", &ChartValue::Int(if on { 2 } else { 1 }));
        }
        "min" | "max" => {
            let name = if k == "min" { "tschchartaxisdefaultusermin" } else { "tschchartaxisdefaultusermax" };
            d.set_chart_prop(h, id, part, name, &ChartValue::Number(v.as_f64().unwrap_or(0.0)));
        }
        "major" => {
            d.set_chart_prop(h, id, part, "tschchartaxisvaluenumberofmajorgridlines", &ChartValue::Int(num(v).clamp(1.0, 20.0) as i64));
        }
        "minor" => {
            d.set_chart_prop(h, id, part, "tschchartaxisvaluenumberofminorgridlines", &ChartValue::Int(num(v).clamp(1.0, 20.0) as i64));
        }
        "gridlines" | "minorGridlines" | "majorTicks" | "minorTicks" => {
            let p = match k {
                "gridlines" => "showmajorgridlines",
                "minorGridlines" => "showminorgridlines",
                "majorTicks" => "showmajortickmarks",
                _ => "showminortickmarks",
            };
            d.set_chart_prop(h, id, part, &prop(p), &ChartValue::Bool(on));
        }
        "gridStroke" => {
            d.set_chart_prop(h, id, part, &prop("majorgridlinestroke"), &ChartValue::Stroke(Some(stroke(v))));
        }
        "valueLabels" => {
            d.set_chart_flag(h, id, ChartFlag::ValueLabels(on));
        }
        "seriesNames" => {
            d.set_chart_flag(h, id, ChartFlag::SeriesNames(on));
        }
        "seriesFormat" => {
            let m = format_of(v.get("kind").and_then(Value::as_u64).unwrap_or(0), v.get("decimals").and_then(Value::as_u64).unwrap_or(0) as u32).msg();
            for k in 0..n {
                d.set_series_number_format(h, id, k, &m);
            }
        }
        "seriesFill" => {
            if let Some(cc) = to_col(v) {
                d.set_series_fill(h, id, si, &FillSpec::Color(cc));
            }
        }
        "seriesStroke" => {
            let s = (!v.is_null()).then(|| stroke(v));
            d.set_series_stroke(h, id, si, s.as_ref());
        }
        "labelDistance" => {
            let dist = num(v).clamp(0.0, 200.0);
            for k in 0..n {
                d.set_chart_prop(h, id, ChartPart::Series(k), "tschchartseriespielabelexplosion", &ChartValue::Float(dist));
                d.set_chart_prop(h, id, ChartPart::Series(k), "tschchartseriespie2_3labelexplosion", &ChartValue::Float(dist));
            }
        }
        "explosion" => {
            d.set_chart_prop(h, id, ChartPart::Series(si), "tschchartseriespiewedgeexplosion", &ChartValue::Float(num(v).clamp(0.0, 100.0) / 100.0));
        }
        _ => {}
    });
}

// ------------------------------------------------------------------ Format > Style (objinsp style)

fn graphic_preview(g: &kn_model::style::Graphic) -> Value {
    let fill = match &g.fill {
        kn_model::Fill::Color(c) => Some(col(*c)),
        kn_model::Fill::Gradient { stops, .. } if !stops.is_empty() => Some(col(stops[0].1)),
        _ => None,
    };
    json!({
        "fill": fill,
        "stroke": g.stroke.as_ref().map(|s| json!({ "color": col(s.color), "width": s.width })),
        "shadow": g.shadow.is_some(),
        "reflection": g.reflection.is_some(),
    })
}

fn style_state(app: &mut App, d: &kn_model::Drawable) -> Value {
    use kn_model::DrawableKind;
    let doc = app.wb.as_ref().map(|w| &w.doc);
    let mut presets = Vec::new();
    let mut preset_kind = "";
    let mut image_data = None;
    match &d.kind {
        DrawableKind::Shape(s) if !s.is_line => {
            preset_kind = if s.is_text_box { "Text Styles" } else { "Shape Styles" };
            presets = doc.map(|doc| doc.shape_style_presets(s.is_text_box)).unwrap_or_default();
        }
        DrawableKind::Image(im) if im.gallery.len() < 2 => {
            preset_kind = "Image Styles";
            presets = doc.map(|doc| doc.image_style_presets()).unwrap_or_default();
            image_data = im.data_id;
        }
        _ => {}
    }
    let previews: Vec<Value> = presets
        .iter()
        .map(|pid| {
            let g = doc.map(|doc| kn_model::style::graphic(&doc.file, Some(*pid)));
            let mut p = g.as_ref().map(graphic_preview).unwrap_or(Value::Null);
            p["id"] = json!(pid);
            p
        })
        .collect();
    let (fill, stroke, is_line, tail, head) = match &d.kind {
        DrawableKind::Shape(s) => (Some(fill_json(&s.fill, doc)), stroke_json(&s.stroke), s.is_line, s.tail.is_some(), s.head.is_some()),
        DrawableKind::Image(i) => (None, stroke_json(&i.stroke), false, false, false),
        _ => (None, Value::Null, false, false, false),
    };
    let has_border = matches!(&d.kind, DrawableKind::Shape(_) | DrawableKind::Image(_));
    let fill_swatch = match &d.kind {
        DrawableKind::Shape(s) => match &s.fill {
            kn_model::Fill::Color(c) => Some(col(*c)),
            kn_model::Fill::Gradient { stops, .. } => stops.first().map(|s| col(s.1)),
            kn_model::Fill::None => None,
            #[allow(unreachable_patterns)]
            _ => Some(json!([0.5, 0.5, 0.5, 1.0])),
        },
        _ => None,
    };
    let shown = |field: &str, hidden: &str| {
        doc.and_then(|doc| doc.file.obj(d.id)).and_then(|m| m.find_super("TSD.DrawableArchive")).is_some_and(|dm| dm.bool(hidden) == Some(false) && dm.ref_id(field).is_some_and(|c| doc.is_some_and(|doc| doc.file.type_name(c) == "TSA.CaptionInfoArchive")))
    };
    json!({
        "presetKind": preset_kind,
        "presets": previews,
        "imageData": image_data,
        "fill": fill,
        "fillSwatch": fill_swatch,
        "stroke": stroke,
        "hasBorder": has_border,
        "isLine": is_line,
        "tail": tail, "head": head,
        "shadow": d.shadow.as_ref().map(|s| json!({ "color": col(s.color), "angle": s.angle, "offset": s.offset, "blur": s.blur, "opacity": s.opacity, "kind": s.kind, "height": s.height, "curve": s.curve })),
        "reflection": d.reflection,
        "opacity": d.opacity,
        "title": shown("title", "title_hidden"),
        "caption": shown("caption", "caption_hidden"),
    })
}

fn set_style(app: &mut App, k: &str, v: &Value) {
    let Some(id) = app.canvas.obj else { return };
    let Some(d) = app.drawables.iter().find(|d| d.id == id).cloned() else { return };
    match k {
        "preset" => {
            if let Some(pid) = v.as_u64() {
                crate::editing::apply(app, "Style", |doc, h, id| doc.apply_shape_style(h, id, pid as ObjId));
            }
        }
        "fill" => {
            if let Some(f) = fill_from(v, app.wb.as_mut().map(|w| &mut w.doc)) {
                crate::editing::apply(app, "Fill", |doc, h, id| doc.set_fill(h, id, &f));
            }
        }
        "stroke" => {
            let s = stroke_from(v);
            crate::editing::apply(app, "Border", |doc, h, id| doc.set_stroke(h, id, s.as_ref()));
        }
        "ends" => {
            let (tail, head) = (v.get(0).and_then(Value::as_bool).unwrap_or(false), v.get(1).and_then(Value::as_bool).unwrap_or(false));
            let end = kn_model::insert::arrow_line_end();
            crate::editing::apply(app, "Arrows", |doc, h, id| doc.set_line_ends(h, id, tail.then_some(&end), head.then_some(&end)));
        }
        "shadowKind" => {
            // No Shadow / Drop / Contact / Curved: the object's own settings if the kind stays, else Keynote's defaults
            let k = v.as_u64().unwrap_or(0) as u8;
            let defaults = |kind: u8| kn_model::Shadow { color: Color::BLACK, angle: 315.0, offset: if kind == 0 { 5.0 } else { 0.0 }, blur: if kind == 1 { 6.0 } else { 10.0 }, opacity: if kind == 0 { 0.75 } else { 0.5 }, kind, height: 0.2, curve: 0.6 };
            let spec = (k > 0).then(|| match d.shadow.clone() {
                Some(s) if s.kind == k - 1 => s,
                _ => defaults(k - 1),
            });
            crate::editing::apply(app, "Shadow", |doc, h, id| doc.set_shadow_spec(h, id, spec.as_ref()));
        }
        "shadow" => {
            let Some(mut s) = d.shadow.clone() else { return };
            let f = |k: &str| v.get(k).map(num);
            if let Some(x) = f("blur") {
                s.blur = x.clamp(0.0, 100.0);
            }
            if let Some(x) = f("offset") {
                s.offset = x.clamp(0.0, 100.0);
            }
            if let Some(x) = f("opacity") {
                s.opacity = x.clamp(0.0, 100.0) / 100.0;
            }
            if let Some(x) = f("angle") {
                s.angle = x.clamp(0.0, 359.0);
            }
            if let Some(x) = f("perspective") {
                s.height = x.clamp(1.0, 90.0).to_radians().sin();
            }
            if let Some(x) = f("curve") {
                s.curve = x.clamp(0.0, 100.0) / 100.0;
            }
            if let Some(c) = v.get("color").and_then(to_col) {
                s.color = c;
            }
            crate::editing::apply(app, "Shadow", |doc, h, id| doc.set_shadow_spec(h, id, Some(&s)));
        }
        "reflection" => {
            let a = if v.is_boolean() { if on_of(v) { 0.5 } else { 0.0 } } else { (num(v) / 100.0).max(0.01) };
            crate::editing::apply(app, "Reflection", |doc, h, id| doc.set_reflection_amount(h, id, a));
        }
        "opacity" => {
            let op = num(v).clamp(0.0, 100.0) / 100.0;
            crate::editing::apply(app, "Opacity", |doc, h, id| doc.set_opacity(h, id, op));
        }
        "title" | "caption" => {
            let title = k == "title";
            let on = on_of(v);
            if let Some(slide) = app.current_slide().map(|s| s.slide) {
                if let Some(doc) = app.wb.as_mut().map(|w| &mut w.doc) {
                    doc.begin(&mut app.hist, if title { "Title" } else { "Caption" });
                    doc.set_caption(&mut app.hist, slide, id, title, on);
                    doc.end(&mut app.hist);
                }
                app.bump(slide);
            }
        }
        _ => {}
    }
}

fn on_of(v: &Value) -> bool {
    v.as_bool().unwrap_or(false)
}

// ------------------------------------------------------------------ fills and borders

/// A fill as the Fill editor shows it (objinsp fill_editor): {kind 0 none, 1 colour, 2
/// gradient, 3 image, 4 advanced image, 5 advanced gradient, …its values}.
pub(crate) fn fill_json(fill: &kn_model::Fill, doc: Option<&kn_model::Document>) -> Value {
    use kn_model::format::FillSpec;
    match crate::editing::fill_spec(fill) {
        FillSpec::None => json!({ "kind": 0 }),
        FillSpec::Color(c) => json!({ "kind": 1, "color": col(c) }),
        FillSpec::Gradient { from, to, angle } => json!({ "kind": 2, "from": col(from), "to": col(to), "angle": angle }),
        FillSpec::Image { data_id, technique, size, reference, tint } => {
            let natural = doc.and_then(|d| d.data(data_id)).and_then(|b| image::ImageReader::new(std::io::Cursor::new(b.to_vec())).with_guessed_format().ok()?.into_dimensions().ok());
            json!({
                "kind": if tint.is_some() { 4 } else { 3 },
                "data": data_id, "technique": format!("{technique:?}"), "size": size.map(|s| json!([s.0, s.1])),
                "reference": reference.map(col), "tint": tint.map(col), "natural": natural.map(|n| json!([n.0, n.1])),
            })
        }
        FillSpec::Advanced { stops, radial, angle } => json!({ "kind": 5, "stops": stops.iter().map(|s| json!([s.0, col(s.1), s.2])).collect::<Vec<_>>(), "radial": radial, "angle": angle }),
    }
}

/// The fill the Fill editor sent back ({kind, …}); an image "path" is added to the document.
pub(crate) fn fill_from(v: &Value, doc: Option<&mut kn_model::Document>) -> Option<kn_model::format::FillSpec> {
    use kn_model::format::FillSpec;
    use kn_model::ImageFillTechnique as T;
    let c = |k: &str| v.get(k).and_then(to_col);
    Some(match v.get("kind")?.as_u64()? {
        0 => FillSpec::None,
        1 => FillSpec::Color(c("color")?),
        2 => FillSpec::Gradient { from: c("from")?, to: c("to")?, angle: num(v.get("angle")?) },
        3 | 4 => {
            let (mut data_id, mut size) = (v.get("data").and_then(Value::as_u64).unwrap_or(0), v.get("size").and_then(Value::as_array).map(|a| (num(&a[0]), num(&a[1]))));
            let mut reference = c("reference");
            if let Some(path) = v.get("path").and_then(Value::as_str) {
                let bytes = std::fs::read(path).ok()?;
                let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok()?.into_dimensions().ok()?;
                let name = std::path::Path::new(path).file_name()?.to_string_lossy().to_string();
                data_id = doc?.file.add_image_data(bytes, &name).ok()?;
                size = Some((w as f32, h as f32));
                reference = None;
            }
            let technique = match v.get("technique").and_then(Value::as_str).unwrap_or("ScaleToFill") {
                "Natural" => T::Natural,
                "Stretch" => T::Stretch,
                "Tile" => T::Tile,
                "ScaleToFit" => T::ScaleToFit,
                _ => T::ScaleToFill,
            };
            FillSpec::Image { data_id, technique, size, reference, tint: c("tint") }
        }
        _ => FillSpec::Advanced {
            stops: v.get("stops")?.as_array()?.iter().filter_map(|s| Some((num(s.get(0)?), to_col(s.get(1)?)?, num(s.get(2)?)))).collect(),
            radial: v.get("radial").and_then(Value::as_bool).unwrap_or(false),
            angle: v.get("angle").map(num).unwrap_or(270.0),
        },
    })
}

/// A border as the Border editor shows it: null (none) or {color, width, dash}.
pub(crate) fn stroke_json(s: &Option<kn_model::Stroke>) -> Value {
    match s {
        None => Value::Null,
        Some(s) => json!({ "color": col(s.color), "width": s.width, "dash": if s.dash.is_empty() { "Solid" } else if s.dash[0] < 0.5 { "Dotted" } else { "Dashed" } }),
    }
}

pub(crate) fn stroke_from(v: &Value) -> Option<kn_model::format::StrokeSpec> {
    use kn_model::format::Dash;
    if v.is_null() {
        return None;
    }
    Some(kn_model::format::StrokeSpec {
        color: v.get("color").and_then(to_col).unwrap_or(Color::BLACK),
        width: v.get("width").map(num).unwrap_or(1.0),
        dash: match v.get("dash").and_then(Value::as_str) {
            Some("Dashed") => Dash::Dashed,
            Some("Dotted") => Dash::Dotted,
            _ => Dash::Solid,
        },
    })
}

// ------------------------------------------------------------------ Document

/// A gallery's shown picture (its arrows): the sheet shows it, the document doesn't change.
fn show_image(app: &mut App, id: ObjId, k: usize) {
    app.gallery_pos.insert(id, k);
    app.version += 1;
}

/// A text box's or shape's Text tab: its font and paragraph, as the cells' Text pane shows them.
fn obj_text_state(app: &mut App) -> Value {
    let Some(run) = crate::menus::current_char(app) else { return Value::Null };
    if app.font_names.is_none() {
        app.font_names = Some(crate::editing::font_names(app));
    }
    json!({
        "family": kn_render::fonts::parse_ps_name(&run.font_name).0,
        "fonts": app.font_names.clone().unwrap_or_default(),
        "size": run.size, "bold": run.bold, "italic": run.italic, "underline": run.underline, "strike": run.strike,
        "color": col(run.color),
        "object": true,
    })
}

fn set_obj_text(app: &mut App, ctx: &egui::Context, k: &str, v: &Value) {
    use nm_core::kn_model::format::TextProp;
    let run = crate::menus::current_char(app).unwrap_or_default();
    let n = num(v);
    match k {
        "family" => {
            if let Some(f) = v.as_str() {
                crate::editing::set_text(app, "Font", TextProp::Font(f.replace(' ', "")));
            }
        }
        "size" => crate::editing::set_text(app, "Font Size", TextProp::Size(n.clamp(1.0, 1000.0).round())),
        "bold" => crate::editing::set_text(app, "Bold", TextProp::Bold(!run.bold)),
        "italic" => crate::editing::set_text(app, "Italic", TextProp::Italic(!run.italic)),
        "underline" => crate::editing::set_text(app, "Underline", TextProp::Underline(!run.underline)),
        "strike" => crate::editing::set_text(app, "Strikethrough", TextProp::Strike(!run.strike)),
        "color" => {
            if let Some(c) = to_col(v) {
                crate::editing::set_text(app, "Text Color", TextProp::Color(c));
            }
        }
        "align" => {
            let name = ["Text > Align Left", "Text > Align Center", "Text > Align Right", "Text > Justify"][(v.as_u64().unwrap_or(0) as usize).min(3)];
            crate::menus::run(app, ctx, name);
        }
        _ => {}
    }
}
