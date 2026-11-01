//! The inspector as data, for front ends that draw it themselves (the Flutter app, ffi.rs):
//! `state` says which pane shows and every control's value, as the desktop inspector would
//! show them now; `set` does what changing one of them does there (the same edits as
//! inspector.rs, textinsp.rs, listcap.rs…). Keys are "<pane>.<control>".

use pg_core::kn_format::ObjId;
use pg_core::kn_model::{self, Color};
use serde_json::{Value, json};

use crate::app::{App, InspMode, Owner};
use crate::text::CharChange;
use crate::textinsp::{apply_char, apply_para, char_style, lookup, para_chain, target, Target};

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

/// Which pane the inspector shows, and its values.
pub fn state(app: &mut App) -> Value {
    if app.insp_mode == InspMode::Document {
        return document_state(app);
    }
    if let Some(sel) = app.sel {
        return cells_state(app, sel.table);
    }
    if app.cursor.is_some() && app.canvas.obj.is_none_or(|o| app.canvas.editing == Some(o)) {
        if let Some(Owner::Drawable(id)) = app.cursor.map(|c| c.owner) {
            return object_state(app, id, true);
        }
        return json!({ "pane": "text", "tabs": ["Text"], "tab": 0, "text": text_state(app) });
    }
    if let Some(id) = app.canvas.obj {
        return object_state(app, id, false);
    }
    json!({ "pane": "nothing" })
}

/// Changes a control: `key` as `state` names it, `v` its new value (or what was picked).
pub fn set(app: &mut App, key: &str, v: &Value) {
    let (pane, k) = key.split_once('.').unwrap_or((key, ""));
    match pane {
        "text" => set_text(app, k, v),
        "doc" => set_document(app, k, v),
        "obj" => set_object(app, k, v),
        "style" => set_style(app, k, v),
        "arrange" => set_arrange(app, k, v),
        "image" => set_image(app, k, v),
        "chart" => set_chart(app, k, v),
        "table" => set_table(app, k, v),
        "media" => set_media(app, k, v),
        "cell" => set_cell(app, k, v),
        _ => {}
    }
}

// ------------------------------------------------------------------ Format > Text

fn text_state(app: &mut App) -> Value {
    let Some(t) = target(app) else { return Value::Null };
    let doc = app.doc();
    // the paragraph style button
    let (named, name, over) = doc.map(|d| pg_core::edit::named_style(d, pg_core::edit::para_style_at(d, t.storage, t.a))).unwrap_or((None, "Body".into(), false));
    let order = ["Title", "Subtitle", "Heading", "Heading 2", "Heading 3", "Heading Red", "Body", "Caption", "Header & Footer", "Footnote", "Label", "Label Dark", "Quote", "Attribution"];
    let mut styles = doc.map(|d| d.paragraph_styles()).unwrap_or_default();
    styles.sort_by_key(|(_, n)| order.iter().position(|o| o == n).unwrap_or(100));
    let para_styles: Vec<Value> = styles
        .iter()
        .map(|(id, n)| {
            let cs = doc.map(|d| kn_model::style::char_style(&d.file, Some(*id), None)).unwrap_or_default();
            json!({ "id": id, "name": n, "size": cs.size, "bold": cs.bold, "italic": cs.italic })
        })
        .collect();
    let cs = char_style(app, &t);
    let chain = para_chain(app, &t);
    let family = kn_render::fonts::parse_ps_name(&cs.font_name).0;
    if app.font_names.is_none() {
        app.font_names = Some(crate::editing::font_names(app));
    }
    let doc = app.doc();
    let char_styles: Vec<Value> = doc.map(|d| d.character_styles()).unwrap_or_default().iter().filter(|(_, n)| n != "None").map(|(id, n)| json!({ "id": id, "name": n })).collect();
    let char_cur = doc
        .and_then(|d| pg_core::edit::char_style_at(d, t.storage, if t.a < t.b { t.a + 1 } else { t.a }))
        .and_then(|id| doc.map(|d| pg_core::edit::named_style(d, Some(id)).1))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "None".into());
    let align = match lookup(&chain, |p| p.enum_name("alignment")).unwrap_or("TATvalue0") {
        "TATvalue2" => 1,
        "TATvalue1" => 2,
        "TATvalue3" => 3,
        _ => 0,
    };
    let ls = lookup(&chain, |p| p.msg("line_spacing").filter(|m| !m.fields.is_empty()).cloned());
    let mode = ls.as_ref().and_then(|m| m.enum_name("mode")).unwrap_or("kRelativeLineSpacing").to_string();
    let amount = ls.as_ref().and_then(|m| m.f32("amount")).unwrap_or(1.0);
    // bullets & lists
    let (list, level) = doc.map(|d| pg_core::edit::list_at(d, t.storage, t.a)).unwrap_or((None, 0));
    let list_name = list.and_then(|id| doc.map(|d| pg_core::edit::named_style(d, Some(id)).1)).unwrap_or_else(|| "None".into());
    let lists: Vec<Value> = doc.map(pg_core::edit::list_styles).unwrap_or_default().iter().map(|(id, n)| json!({ "id": id, "name": n, "marker": doc.map(|d| pg_core::edit::list_marker(d, *id)).unwrap_or_default() })).collect();
    let lv = list.and_then(|l| doc.map(|d| pg_core::edit::list_level(d, l, level)));
    // drop caps
    let dc_styles = doc.map(pg_core::edit::drop_cap_styles).unwrap_or_default();
    let dc_cur = doc.and_then(|d| pg_core::edit::drop_cap_at(d, t.storage, t.a));
    let dc = |id: ObjId| doc.map(|d| drop_cap_json(&pg_core::edit::drop_cap_settings(d, id)));
    // layout
    let body = t.owner == Owner::Body;
    let (count, gap) = app.wb.as_ref().map(|w| w.columns()).unwrap_or((1, 0.05));
    let tw = app.layout.as_ref().map(|l| l.setup.text_width()).unwrap_or(468.0);
    let stroke = lookup(&chain, |p| p.msg("stroke").and_then(kn_model::style::stroke));
    let positions = lookup(&chain, |p| p.u64("border_positions")).unwrap_or(0);
    let border = if positions == 0 || stroke.is_none() { 0 } else if positions == 15 { 2 } else { 1 };
    let following = lookup(&chain, |p| p.ref_id("following_style"));
    let fname = following.and_then(|f| doc.map(|d| pg_core::edit::named_style(d, Some(f)).1)).unwrap_or_else(|| "Same".into());
    json!({
        "styleName": if over { format!("{name}*") } else { name.clone() },
        "styleOver": over,
        "styleNamed": named,
        "paraStyles": para_styles,
        "subTab": app.text_tab.min(2),
        "family": family,
        "fonts": app.font_names.clone().unwrap_or_default(),
        "bold": cs.bold, "italic": cs.italic, "underline": cs.underline, "strike": cs.strike,
        "size": cs.size,
        "tracking": cs.tracking * 100.0,
        "baselineShift": cs.baseline_shift,
        "script": cs.script,
        "caps": format!("{:?}", cs.caps),
        "background": cs.background.map(col),
        "charStyle": char_cur,
        "charStyles": char_styles,
        "color": col(cs.color),
        "align": align,
        "spacingMode": mode,
        "spacingAmount": amount,
        "spaceBefore": lookup(&chain, |p| p.f32("space_before")).unwrap_or(0.0),
        "spaceAfter": lookup(&chain, |p| p.f32("space_after")).unwrap_or(0.0),
        "list": list_name,
        "listOn": list.is_some(),
        "lists": lists,
        "listLevel": lv.map(|l| json!({ "kind": l.kind, "string": l.string, "number": l.number, "indent": l.indent, "textIndent": l.text_indent, "scale": l.scale, "tiered": l.tiered })),
        "bullets": crate::listcap::BULLETS,
        "numberFormats": crate::listcap::NUMBER_FORMATS.iter().map(|(k, n)| json!([k, n])).collect::<Vec<_>>(),
        "dropCap": dc_cur.and_then(dc),
        "dropCapOn": dc_cur.is_some(),
        "dropCapStyles": dc_styles.iter().filter_map(|(id, _)| dc(*id).map(|s| json!({ "id": id, "settings": s }))).collect::<Vec<_>>(),
        "body": body,
        "columns": count,
        "gutter": gap * tw,
        "firstIndent": lookup(&chain, |p| p.f32("first_line_indent")).unwrap_or(0.0),
        "leftIndent": lookup(&chain, |p| p.f32("left_indent")).unwrap_or(0.0),
        "rightIndent": lookup(&chain, |p| p.f32("right_indent")).unwrap_or(0.0),
        "tabStops": lookup(&chain, |p| p.f32("default_tab_stops")).unwrap_or(36.0),
        "border": border,
        "borderColor": stroke.as_ref().map(|s| col(s.color)),
        "borderWidth": stroke.as_ref().map(|s| s.width),
        "paraBackground": lookup(&chain, |p| p.msg("fill").map(kn_model::style::color)).map(col),
        "keepLines": lookup(&chain, |p| p.bool("keep_lines_together")).unwrap_or(false),
        "keepNext": lookup(&chain, |p| p.bool("keep_with_next")).unwrap_or(false),
        "pageBreak": lookup(&chain, |p| p.bool("page_break_before")).unwrap_or(false),
        "widow": lookup(&chain, |p| p.bool("widow_control")).unwrap_or(true),
        "hyphenate": lookup(&chain, |p| p.bool("hyphenate")).unwrap_or(true),
        "ligatures": app.wb.as_ref().map(|w| w.setup().ligatures).unwrap_or(true),
        "following": fname,
        "followingNone": following.is_none(),
    })
}

fn drop_cap_json(s: &pg_core::edit::DropCapSettings) -> Value {
    json!({
        "lines": s.lines, "raised": s.raised, "chars": s.chars, "shape": s.shape,
        "fill": s.fill.map(col), "cornerRadius": s.corner_radius, "scale": s.scale, "wrap": s.wrap,
        "color": s.color.map(col), "bold": s.bold,
    })
}

/// A character property over the target, as textinsp's stepper rows do it (a caret: its word).
fn char_prop(app: &mut App, label: &str, f: impl Fn(&mut pg_core::kn_format::Msg) + 'static) {
    let Some(t) = target(app) else { return };
    let st = t.storage;
    app.edit(label, |d, h| {
        pg_core::edit::set_char_props(d, h, st, t.a, t.b.max(t.a + 1), &f);
    });
}

fn para_stroke(app: &mut App, label: &str, c: Color, w: f32) {
    apply_para(app, label, move |pp| {
        pp.remove("stroke");
        pp.push_msg("stroke", pg_core::edit::para_stroke_msg(&pg_core::kn_model::format::StrokeSpec { color: c, width: w, dash: pg_core::kn_model::format::Dash::Solid }));
    });
}

fn set_text(app: &mut App, k: &str, v: &Value) {
    let Some(t) = target(app) else { return };
    let st = t.storage;
    let id = || v.as_u64().map(|x| x as ObjId);
    let on = v.as_bool().unwrap_or(false);
    match k {
        "subTab" => app.text_tab = v.as_u64().unwrap_or(0) as usize,
        "paraStyle" => {
            if let Some(s) = id() {
                app.edit("Paragraph Style", |d, h| pg_core::edit::set_para_style(d, h, st, t.a, t.b, s));
            }
        }
        "updateStyle" => {
            if let Some(cur) = app.doc().and_then(|d| pg_core::edit::para_style_at(d, st, t.a)) {
                app.edit("Update Style", |d, h| pg_core::edit::update_named_style(d, h, cur));
            }
        }
        "revertStyle" => {
            if let Some(named) = app.doc().and_then(|d| pg_core::edit::named_style(d, pg_core::edit::para_style_at(d, st, t.a)).0) {
                app.edit("Paragraph Style", |d, h| pg_core::edit::set_para_style(d, h, st, t.a, t.b, named));
            }
        }
        "newStyle" => {
            let name = app.doc().map(|d| pg_core::edit::named_style(d, pg_core::edit::para_style_at(d, st, t.a)).1).unwrap_or_else(|| "Body".into());
            app.new_style = Some(format!("{name} 2"));
        }
        "family" => {
            if let Some(f) = v.as_str() {
                let ps = app.canvas.tiles.ps_name_for_family(f).unwrap_or_else(|| f.replace(' ', ""));
                apply_char(app, "Font", CharChange::Font(ps));
            }
        }
        "typeface" => {
            let cs = char_style(app, &t);
            let (b, i) = match v.as_str().unwrap_or("Regular") {
                "Bold Italic" => (true, true),
                "Bold" => (true, false),
                "Italic" => (false, true),
                _ => (false, false),
            };
            if b != cs.bold {
                apply_char(app, "Bold", CharChange::Bold(b));
            }
            if i != cs.italic {
                apply_char(app, "Italic", CharChange::Italic(i));
            }
        }
        "size" => apply_char(app, "Font Size", CharChange::Size(num(v).round().clamp(1.0, 1638.0))),
        "bold" => apply_char(app, "Bold", CharChange::Bold(on)),
        "italic" => apply_char(app, "Italic", CharChange::Italic(on)),
        "underline" => apply_char(app, "Underline", CharChange::Underline(on)),
        "strike" => apply_char(app, "Strikethrough", CharChange::Strike(on)),
        "color" => {
            if let Some(c) = to_col(v) {
                apply_char(app, "Text Color", CharChange::Color(c));
            }
        }
        "tracking" => {
            let x = num(v).round().clamp(-50.0, 200.0) / 100.0;
            char_prop(app, "Character Spacing", move |cp| cp.set_f32("tracking", x));
        }
        "baselineShift" => {
            let x = num(v).clamp(-100.0, 100.0);
            char_prop(app, "Baseline Shift", move |cp| cp.set_f32("baseline_shift", x));
        }
        "script" => apply_char(app, "Baseline", CharChange::Script(v.as_i64().unwrap_or(0) as i8)),
        "caps" => {
            let n = v.as_str().unwrap_or("None").to_string();
            char_prop(app, "Capitalization", move |cp| crate::text::set_caps(cp, &n));
        }
        "background" => {
            let c = to_col(v);
            char_prop(app, "Text Background", move |cp| {
                cp.remove("background_color");
                if let Some(c) = c.filter(|c| c.a > 0.0) {
                    cp.push_msg("background_color", kn_model::format::color_msg(c));
                }
            });
        }
        "charStyle" => {
            let style = id();
            if t.a < t.b {
                app.edit("Character Style", |d, h| d.set_character_style(h, st, t.a, t.b, style));
            }
        }
        "align" => {
            let s = ["TATvalue0", "TATvalue2", "TATvalue1", "TATvalue3"][(v.as_u64().unwrap_or(0) as usize).min(3)];
            apply_para(app, "Alignment", move |pp| pp.set_enum("alignment", s));
        }
        "indent" => {
            let delta = if v.as_i64().unwrap_or(1) < 0 { -1 } else { 1 };
            let in_list = app.layout.as_ref().and_then(|l| l.body.as_ref()).is_some_and(|b| b.tb.paragraphs.get(b.para_at_stored(t.a)).is_some_and(|p| p.label.is_some()));
            if in_list || t.owner != Owner::Body {
                app.edit("Indent", |d, h| pg_core::edit::change_level(d, h, st, t.a, t.b, delta));
            } else {
                let left = lookup(&para_chain(app, &t), |p| p.f32("left_indent")).unwrap_or(0.0);
                let x = (left + delta as f32 * 36.0).max(0.0);
                apply_para(app, "Indent", move |pp| pp.set_f32("left_indent", x));
            }
        }
        "spacingMode" | "spacingAmount" => {
            let chain = para_chain(app, &t);
            let ls = lookup(&chain, |p| p.msg("line_spacing").filter(|m| !m.fields.is_empty()).cloned());
            let mode = ls.as_ref().and_then(|m| m.enum_name("mode")).unwrap_or("kRelativeLineSpacing").to_string();
            let (m, x) = if k == "spacingMode" {
                let m = v.as_str().unwrap_or("kRelativeLineSpacing").to_string();
                let x = if m == "kRelativeLineSpacing" { 1.0 } else { char_style(app, &t).size * 1.2 };
                (m, x)
            } else {
                let rel = mode == "kRelativeLineSpacing";
                (mode, num(v).clamp(if rel { 0.1 } else { 0.0 }, if rel { 10.0 } else { 500.0 }))
            };
            apply_para(app, "Line Spacing", move |pp| {
                pp.remove("line_spacing");
                let ls = pp.msg_mut_or_create("line_spacing");
                ls.set_enum("mode", &m);
                ls.set_f32("amount", x);
            });
        }
        "spaceBefore" | "spaceAfter" => {
            let (label, field) = if k == "spaceBefore" { ("Before Paragraph", "space_before") } else { ("After Paragraph", "space_after") };
            let x = num(v).round().clamp(0.0, 1000.0);
            apply_para(app, label, move |pp| pp.set_f32(field, x));
        }
        "list" => {
            let l = id();
            app.edit("List", |d, h| pg_core::edit::set_list_style(d, h, st, t.a, t.b, l));
        }
        "listLevel" => {
            let (list, level) = app.doc().map(|d| pg_core::edit::list_at(d, st, t.a)).unwrap_or((None, 0));
            let Some(list) = list else { return };
            let Some(mut n) = app.doc().map(|d| pg_core::edit::list_level(d, list, level)) else { return };
            if let Some(x) = v.get("kind").and_then(Value::as_u64) {
                n.kind = x;
            }
            if let Some(x) = v.get("string").and_then(Value::as_str) {
                n.string = x.to_string();
            }
            if let Some(x) = v.get("number").and_then(Value::as_u64) {
                n.number = x;
            }
            if let Some(x) = v.get("tiered").and_then(Value::as_bool) {
                n.tiered = x;
            }
            if let Some(x) = v.get("scale") {
                n.scale = num(x).clamp(10.0, 400.0) / 100.0;
            }
            if let Some(x) = v.get("indent") {
                n.indent = num(x).clamp(0.0, 500.0);
            }
            if let Some(x) = v.get("textIndent") {
                n.text_indent = num(x).clamp(0.0, 500.0);
            }
            app.edit("List", |d, h| pg_core::edit::set_list_level(d, h, st, t.a, t.b, level, &|l| *l = n.clone()));
        }
        "dropCap" => {
            let p = if v.is_null() || v == &json!(false) { None } else if v == &json!(true) { app.doc().and_then(|d| pg_core::edit::drop_cap_styles(d).first().map(|p| p.0)) } else { id() };
            app.edit("Drop Cap", |d, h| pg_core::edit::set_drop_cap(d, h, st, t.a, t.b, p));
        }
        "dropCapSettings" => {
            let Some(cur) = app.doc().and_then(|d| pg_core::edit::drop_cap_at(d, st, t.a)) else { return };
            let Some(default) = app.doc().and_then(|d| pg_core::edit::drop_cap_styles(d).first().map(|p| p.0)) else { return };
            let Some(mut n) = app.doc().map(|d| pg_core::edit::drop_cap_settings(d, cur)) else { return };
            let u = |k: &str| v.get(k).and_then(Value::as_u64).map(|x| x as u32);
            if let Some(x) = u("lines") {
                n.lines = x.clamp(1, 10);
            }
            if let Some(x) = u("chars") {
                n.chars = x.clamp(1, 10);
            }
            if let Some(x) = u("raised") {
                n.raised = x.min(9);
            }
            if let Some(x) = v.get("scale") {
                n.scale = num(x).clamp(20.0, 200.0) / 100.0;
            }
            if let Some(x) = v.get("wrap").and_then(Value::as_u64) {
                n.wrap = x.min(2);
            }
            if let Some(x) = v.get("shape").and_then(Value::as_bool) {
                n.shape = x;
                if x && n.fill.is_none() {
                    n.fill = Some(Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 });
                }
            }
            if let Some(c) = v.get("fill").and_then(to_col) {
                n.fill = Some(c);
            }
            if let Some(x) = v.get("cornerRadius") {
                n.corner_radius = num(x).clamp(0.0, 50.0) / 100.0;
            }
            app.edit("Drop Cap", |d, h| pg_core::edit::set_drop_cap_settings(d, h, st, t.a, t.b, default, &|x| *x = n.clone()));
        }
        "columns" | "gutter" => {
            let (count, gap) = app.wb.as_ref().map(|w| w.columns()).unwrap_or((1, 0.05));
            let tw = app.layout.as_ref().map(|l| l.setup.text_width()).unwrap_or(468.0);
            let (n, g) = if k == "columns" { (num(v).clamp(1.0, 10.0) as u32, gap) } else { (count, num(v).clamp(0.0, 200.0) / tw) };
            app.edit_pages("Columns", |p, h| p.set_columns(h, n, g));
        }
        "firstIndent" | "leftIndent" | "rightIndent" => {
            let field = match k {
                "firstIndent" => "first_line_indent",
                "leftIndent" => "left_indent",
                _ => "right_indent",
            };
            let x = num(v).clamp(-500.0, 1000.0);
            apply_para(app, "Indent", move |pp| pp.set_f32(field, x));
        }
        "tabStops" => {
            let x = num(v).clamp(1.0, 500.0);
            apply_para(app, "Tabs", move |pp| pp.set_f32("default_tab_stops", x));
        }
        "border" => {
            let i = v.as_u64().unwrap_or(0);
            apply_para(app, "Paragraph Borders", move |pp| {
                if i == 0 {
                    pp.set_u64("border_positions", 0);
                } else {
                    pp.set_u64("border_positions", if i == 2 { 15 } else { 1 });
                    if pp.msg("stroke").is_none() {
                        let s = pg_core::kn_model::format::StrokeSpec { color: Color::BLACK, width: 1.0, dash: pg_core::kn_model::format::Dash::Solid };
                        pp.push_msg("stroke", pg_core::edit::para_stroke_msg(&s));
                    }
                }
            });
        }
        "borderColor" | "borderWidth" => {
            let chain = para_chain(app, &t);
            let Some(s) = lookup(&chain, |p| p.msg("stroke").and_then(kn_model::style::stroke)) else { return };
            if k == "borderColor" {
                if let Some(c) = to_col(v) {
                    para_stroke(app, "Border Color", c, s.width);
                }
            } else {
                para_stroke(app, "Border Width", s.color, num(v).clamp(0.5, 20.0));
            }
        }
        "paraBackground" => {
            let c = to_col(v);
            apply_para(app, "Paragraph Background", move |pp| {
                pp.remove("fill");
                if let Some(c) = c {
                    pp.push_msg("fill", kn_model::format::color_msg(c));
                }
            });
        }
        "keepLines" | "keepNext" | "pageBreak" | "widow" | "hyphenate" => {
            let (label, field) = match k {
                "keepLines" => ("Keep lines on same page", "keep_lines_together"),
                "keepNext" => ("Keep with next paragraph", "keep_with_next"),
                "pageBreak" => ("Start paragraph on a new page", "page_break_before"),
                "widow" => ("Prevent widow & orphan lines", "widow_control"),
                _ => ("Hyphenation", "hyphenate"),
            };
            apply_para(app, label, move |pp| pp.set_bool(field, on));
        }
        "ligatures" => {
            app.edit_pages("Ligatures", |p, h| {
                let mut s = p.setup();
                s.ligatures = on;
                p.set_setup(h, &s);
            });
        }
        "following" => {
            let f = id();
            if let Some(named) = app.doc().and_then(|d| pg_core::edit::named_style(d, pg_core::edit::para_style_at(d, st, t.a)).0) {
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
        _ => {}
    }
}

// ------------------------------------------------------------------ objects (inspector.rs object)

/// An object's tabs, as inspector.rs object() names them.
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
        "Text" => text_state(app),
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
    let presets: Vec<Value> = pg_core::styles::table_style_presets(doc)
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
    let len = crate::mediainsp::movie_length(app, d.id, m);
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
            "name": it.data_id.and_then(|data| doc.and_then(|doc| doc.data_name(data)).map(crate::mediainsp::shown_file_name)).unwrap_or_else(|| format!("Image {}", k + 1)),
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
                "show" => crate::mediainsp::show_image(app, id, v.as_u64().unwrap_or(0) as usize),
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
                    crate::mediainsp::show_image(app, id, shown.min(n.saturating_sub(2)));
                }
                "description" => {
                    if let Some(item) = im.gallery.get(shown).map(|it| it.id) {
                        let text = v.as_str().unwrap_or("").to_string();
                        app.edit("Gallery", |doc, h| doc.set_gallery_description(h, item, &text));
                    }
                }
                "replaceWebVideo" => {
                    if let Some(w) = &im.web_video {
                        crate::media::replace_web_video(id, &w.url);
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
        "conditional" => crate::condwin::show(),
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
    let (wp, inline, wrap, margin) = app.wb.as_ref().map(|w| {
        let (wrap, margin) = w.wrap_of(d.id);
        (w.is_word_processing(), w.inline_attachment(d.id).is_some(), wrap, margin)
    }).unwrap_or((false, false, 0, 0.0));
    json!({
        "count": count,
        "locked": d.locked,
        "group": count == 1 && matches!(d.kind, kn_model::DrawableKind::Group(_)),
        "w": g.w, "h": g.h, "x": g.x, "y": g.y, "angle": g.angle,
        "wordProcessing": wp, "inline": inline, "wrap": wrap, "wrapMargin": margin,
    })
}

fn set_arrange(app: &mut App, k: &str, v: &Value) {
    let Some(id) = app.canvas.obj else { return };
    let Some(d) = app.drawables.iter().find(|d| d.id == id).cloned() else { return };
    let ctx = egui::Context::default();
    match k {
        "command" => {
            if let Some(c) = v.as_str() {
                app.command(&ctx, c);
            }
        }
        "placement" => {
            let inline = on_of(v);
            let page = app.obj_page.get(&id).copied().unwrap_or(0);
            let at = match app.cursor {
                Some(c) if c.owner == Owner::Body => c.caret,
                _ => app.layout.as_ref().and_then(|l| l.pages.get(page)).and_then(|p| app.layout.as_ref()?.body.as_ref().map(|b| b.to_stored(p.start))).unwrap_or(0),
            };
            let place = if inline { pg_core::objects::Placement::Inline { at } } else { pg_core::objects::Placement::Floating { page, x: d.geom.x, y: d.geom.y } };
            app.edit_pages("Object Placement", |p, h| p.set_placement(h, id, place));
        }
        "wrap" => {
            let w = v.as_u64().unwrap_or(4);
            app.edit_pages("Text Wrap", |p, h| p.set_wrap(h, id, w));
        }
        "wrapMargin" => {
            let m = num(v).clamp(0.0, 200.0);
            app.edit_pages("Text Wrap", |p, h| p.set_wrap_margin(h, id, m));
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
fn read_format(f: Option<&pg_core::kn_format::Msg>) -> (usize, u32) {
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

fn units(app: &App) -> (&'static str, f32) {
    match app.settings.ruler_units {
        1 => ("in", 72.0),
        2 => ("pt", 1.0),
        3 => ("mm", 72.0 / 2.54 / 10.0),
        _ => ("cm", 72.0 / 2.54),
    }
}

fn document_state(app: &mut App) -> Value {
    let tab = app.doc_tab.min(2);
    let Some(w) = app.wb.as_ref() else { return json!({ "pane": "document", "tab": tab }) };
    let s = w.setup();
    let (unit, per) = units(app);
    let paper = pg_core::paper::find(&s.paper_id, s.width, s.height).map(|p| p.0).unwrap_or("Custom");
    let secs = w.sections();
    let page = app.current_page();
    let si = app.layout.as_ref().and_then(|l| l.pages.get(page)).map(|p| p.section).unwrap_or(0).min(secs.len().saturating_sub(1));
    let section = secs.get(si).map(|sec| {
        let fill = sec.background.clone().unwrap_or(kn_model::Fill::None);
        json!({
            "index": si, "count": secs.len(),
            "background": fill_json(&fill, app.doc()),
            "hideFirst": sec.first_page_hides_header_footer,
            "matchPrevious": sec.inherit_previous,
            "leftRight": sec.even_odd_different,
            "continue": sec.page_number_kind == 0,
            "start": sec.page_number_start,
        })
    });
    json!({
        "pane": "document",
        "tab": tab,
        "papers": pg_core::paper::PAPERS.iter().map(|p| p.0).collect::<Vec<_>>(),
        "paper": paper,
        "dimensions": pg_core::paper::dimension_text(s.width, s.height, unit),
        "landscape": s.landscape,
        "unit": unit,
        "per": per,
        "headers": s.headers, "footers": s.footers,
        "headerMargin": s.header_margin / per, "footerMargin": s.footer_margin / per,
        "body": s.body,
        "top": s.top / per, "bottom": s.bottom / per, "left": s.left / per, "right": s.right / per,
        "facing": s.facing_pages, "hyphenation": s.hyphenation, "ligatures": s.ligatures,
        "section": section,
        "wordProcessing": w.is_word_processing(),
        "bookmarks": w.bookmarks().iter().map(|(id, t, a, b)| json!({ "id": id, "text": t.trim(), "a": a, "b": b })).collect::<Vec<_>>(),
        "canBookmark": app.cursor.is_some_and(|c| c.owner == Owner::Body),
    })
}

fn set_document(app: &mut App, k: &str, v: &Value) {
    let on = v.as_bool().unwrap_or(false);
    if k == "tab" {
        app.doc_tab = v.as_u64().unwrap_or(0) as usize;
        return;
    }
    let Some(w) = app.wb.as_ref() else { return };
    let mut s = w.setup();
    let (_, per) = units(app);
    let mut changed = true;
    match k {
        "paper" => {
            let name = v.as_str().unwrap_or("");
            if let Some((_, id, pw, ph)) = pg_core::paper::PAPERS.iter().find(|p| p.0 == name) {
                s.paper_id = id.to_string();
                (s.width, s.height) = if s.landscape { (*ph, *pw) } else { (*pw, *ph) };
            }
        }
        "landscape" => {
            if on != s.landscape {
                s.landscape = on;
                (s.width, s.height) = (s.height, s.width);
            }
        }
        "headers" => s.headers = on,
        "footers" => s.footers = on,
        "headerMargin" => s.header_margin = num(v).clamp(0.0, 100.0) * per,
        "footerMargin" => s.footer_margin = num(v).clamp(0.0, 100.0) * per,
        "top" => s.top = num(v).clamp(0.0, 1000.0) * per,
        "bottom" => s.bottom = num(v).clamp(0.0, 1000.0) * per,
        "left" => s.left = num(v).clamp(0.0, 1000.0) * per,
        "right" => s.right = num(v).clamp(0.0, 1000.0) * per,
        "facing" => s.facing_pages = on,
        "hyphenation" => s.hyphenation = on,
        "ligatures" => s.ligatures = on,
        _ => changed = false,
    }
    if changed {
        app.edit_pages("Document Setup", |p, h| p.set_setup(h, &s));
        return;
    }
    let secs = w.sections();
    let page = app.current_page();
    let si = app.layout.as_ref().and_then(|l| l.pages.get(page)).map(|p| p.section).unwrap_or(0).min(secs.len().saturating_sub(1));
    let sid = secs.get(si).map(|x| x.id);
    match (k, sid) {
        ("body", _) => crate::menus::convert(app, !on),
        ("background", Some(id)) => {
            if let Some(f) = fill_from(v, app.wb.as_mut().map(|w| &mut w.doc)) {
                app.edit_pages("Background", |p, h| p.set_section_fill(h, id, &f));
            }
        }
        ("hideFirst", Some(id)) => {
            app.edit_pages("Headers & Footers", |p, h| p.set_section(h, id, |m| m.set_bool("section_template_first_page_hides_header_footer", on)));
        }
        ("matchPrevious", Some(id)) => {
            app.edit_pages("Headers & Footers", |p, h| p.set_section(h, id, |m| m.set_bool("inherit_previous_header_footer", on)));
        }
        ("leftRight", Some(id)) => {
            app.edit_pages("Headers & Footers", |p, h| p.set_section(h, id, |m| m.set_bool("section_template_even_odd_pages_different", on)));
        }
        ("continue", Some(id)) => {
            app.edit_pages("Page Numbering", |p, h| p.set_section(h, id, |m| m.set_u64("section_page_number_kind", if on { 0 } else { 1 })));
        }
        ("start", Some(id)) => {
            let n = num(v).clamp(1.0, 9999.0) as u64;
            app.edit_pages("Page Numbering", |p, h| {
                p.set_section(h, id, |m| {
                    m.set_u64("section_page_number_kind", 1);
                    m.set_u64("section_page_number_start", n);
                })
            });
        }
        ("newSection", _) => {
            let ctx = egui::Context::default();
            crate::menus::run(app, &ctx, "Section Break");
        }
        ("goBookmark", _) => {
            let (a, b) = (v.get("a").and_then(Value::as_u64).unwrap_or(0) as usize, v.get("b").and_then(Value::as_u64).unwrap_or(0) as usize);
            if let Some(st) = app.wb.as_ref().and_then(|w| w.body_storage()) {
                app.cursor = Some(crate::app::Cursor { storage: st, owner: Owner::Body, anchor: a, caret: b, goal_x: None });
                app.canvas.follow_caret = true;
                crate::canvas::deselect_objects(app);
            }
        }
        ("removeBookmark", _) => {
            if let Some(id) = v.as_u64() {
                app.edit_pages("Remove Bookmark", |p, h| p.remove_bookmark(h, id as ObjId));
            }
        }
        ("addBookmark", _) => {
            let ctx = egui::Context::default();
            crate::menus::run(app, &ctx, "Bookmark");
        }
        _ => {}
    }
}

#[allow(dead_code)]
fn unused(_t: Target) {}
