//! Keydeck's other windows as data (dlgapi.rs lists them with Settings and the rest): those the
//! suite's Flutter app already draws in Letters' shapes (Fonts, Colors, Spelling, Versions,
//! passwords, Replace Fonts, Record Audio, Web Video, Keyboard Shortcuts, Chart Data, All Tabs),
//! and the rest as forms the front end draws row by row (Link, Go To Slide, Find & Replace,
//! Equation, Conditional Highlighting, Save Theme, Language & Region, Define Placeholder,
//! Warnings, Rename, Customize Presenter Display, Clear Recording, Substitutions, Arrange).
//! `set` takes "<window>.<key>" with the value, as the windows' buttons and fields send them.

use serde_json::{Value, json};

use crate::app::App;

fn label(text: impl Into<String>) -> Value {
    json!({"t": "label", "text": text.into()})
}

fn dim(text: impl Into<String>) -> Value {
    json!({"t": "label", "text": text.into(), "dim": true})
}

fn buttons(items: &[(&str, &str, bool)]) -> Value {
    json!({"t": "buttons", "items": items.iter().map(|(l, k, on)| json!([l, k, on])).collect::<Vec<_>>()})
}

fn form(id: &str, title: &str, rows: Vec<Value>) -> Value {
    json!({"id": id, "title": title, "form": rows})
}

fn col(c: kn_model::Color) -> Value {
    json!([c.r, c.g, c.b, 1.0])
}

/// The windows open now.
pub fn state(app: &mut App) -> Vec<Value> {
    let mut out = Vec::new();
    if app.link_dialog.open {
        let d = &app.link_dialog;
        let mut rows = vec![json!({"t": "seg", "key": "kind", "options": ["Webpage", "Email", "Phone Number", "Slide"], "value": d.kind})];
        if d.kind == 3 {
            rows.push(json!({"t": "radio", "key": "slideKind", "options": ["Next slide", "Previous slide", "First slide", "Last slide", "Slide", "Exit Slideshow", "Last slide viewed"], "value": d.slide_kind}));
            if d.slide_kind == 4 {
                rows.push(json!({"t": "number", "key": "slideNo", "label": "Slide", "value": d.slide_no, "min": 1, "max": app.slides.len().max(1)}));
            }
        } else {
            let hint = ["https://example.com", "name@example.com", "+1 555 123 4567"][d.kind.min(2)];
            rows.push(json!({"t": "text", "key": "text", "value": d.text, "hint": hint, "enter": "done", "focus": true}));
        }
        rows.push(dim("Links work when you play the slideshow."));
        rows.push(buttons(&[("Remove Link", "remove", true), ("Done", "done", true)]));
        out.push(form("slideLink", "Link", rows));
    }
    if app.show_goto {
        out.push(form("goToSlide", "Go to Slide", vec![
            json!({"t": "text", "key": "text", "value": app.goto_buf, "hint": format!("Slide (1–{})", app.slides.len()), "enter": "done", "focus": true, "width": 160}),
            buttons(&[("Go", "done", true)]),
        ]));
    }
    if app.find.open {
        let f = &app.find;
        let mut rows = vec![
            json!({"t": "text", "key": "query", "value": f.query, "hint": "Find", "enter": "next", "focus": true}),
            json!({"t": "text", "key": "replace", "value": f.replace, "hint": "Replace"}),
            json!({"t": "check", "key": "case", "label": "Match Case", "value": f.match_case}),
            json!({"t": "check", "key": "words", "label": "Whole Words", "value": f.whole_words}),
        ];
        if let Some(c) = crate::find::counter(app) {
            rows.push(dim(c));
        }
        rows.push(buttons(&[("◀", "prev", true), ("▶", "next", true), ("Replace", "replace", true), ("Replace All", "all", true)]));
        out.push(form("find", "Find & Replace", rows));
    }
    if app.equation.open {
        let mut rows = vec![
            label("Enter a LaTeX equation:"),
            json!({"t": "text", "key": "text", "value": app.equation.source, "hint": "\\frac{a}{b} = \\sqrt{x^2 + y^2}", "rows": 3, "focus": true, "width": 400}),
        ];
        if let Some(e) = &app.equation.error {
            rows.push(label(e.clone()));
        }
        rows.push(dim("Typeset with pdflatex"));
        rows.push(buttons(&[("Insert", "done", !app.equation.source.trim().is_empty())]));
        out.push(form("equationTex", "Equation", rows));
    }
    if app.cond_win.open {
        let w = &app.cond_win;
        let range = crate::tableops::range(app);
        let what = match range {
            Some((_, r0, c0, r1, c1)) => format!("Rule for {} selected cell{}", (r1 - r0 + 1) * (c1 - c0 + 1), if (r0, c0) == (r1, c1) { "" } else { "s" }),
            None => "Select table cells first.".into(),
        };
        let ok = range.is_some() && w.value.trim().replace(',', "").parse::<f64>().is_ok();
        out.push(form("condRule", "Conditional Highlighting", vec![
            label(what),
            json!({"t": "choice", "key": "cmp", "label": "Number", "options": kn_model::tablecond::Cmp::ALL.iter().map(|c| c.name()).collect::<Vec<_>>(), "value": w.cmp}),
            json!({"t": "text", "key": "value", "value": w.value, "width": 100}),
            json!({"t": "choice", "key": "look", "label": "Style", "options": kn_model::tablecond::LOOKS.iter().map(|l| l.name).collect::<Vec<_>>(), "value": w.look}),
            buttons(&[("Add Rule", "add", ok)]),
        ]));
    }
    if let Some(name) = &app.dialogs.save_theme {
        out.push(form("saveTheme", "Save Theme", vec![
            label("Theme name:"),
            json!({"t": "text", "key": "text", "value": name, "enter": "done", "focus": true}),
            buttons(&[("Cancel", "close", true), ("Add to Theme Chooser", "done", !name.trim().is_empty())]),
        ]));
    }
    if app.dialogs.language {
        let cur = crate::versions::language(app);
        let pick = crate::versions::LANGUAGES.iter().position(|(c, _)| *c == cur).unwrap_or(0);
        out.push(form("language", "Language & Region", vec![
            label("Language of this presentation (spelling, quotes, number and date formats):"),
            json!({"t": "radio", "key": "pick", "options": crate::versions::LANGUAGES.iter().map(|(_, n)| *n).collect::<Vec<_>>(), "value": pick}),
        ]));
    }
    if let Some((_, tag, display, media)) = &app.dialogs.define_placeholder {
        out.push(json!({"id": "placeholder", "title": if *media { "Define as Media Placeholder" } else { "Define as Text Placeholder" }, "closable": false, "form": [
            label("Tag:"),
            {"t": "text", "key": "tag", "value": tag, "focus": true},
            label("Display name:"),
            {"t": "text", "key": "display", "value": display},
            buttons(&[("Cancel", "close", true), ("Define", "done", !tag.trim().is_empty())]),
        ]}));
    }
    if app.warnings.open {
        let w = &app.warnings;
        let mut rows: Vec<Value> = if w.list.is_empty() { vec![dim("There are no warnings for this presentation.")] } else { w.list.iter().map(|l| label(format!("\u{26a0}  {l}"))).collect() };
        if w.missing_fonts {
            rows.push(buttons(&[("Replace Fonts\u{2026}", "replace", true)]));
        }
        out.push(form("warnings", "Document Warnings", rows));
    }
    if let Some(name) = &app.fileops.rename {
        out.push(form("rename", "Rename", vec![
            json!({"t": "text", "key": "text", "value": name, "enter": "done", "focus": true, "width": 280}),
            buttons(&[("Rename", "done", true)]),
        ]));
    }
    if app.show_pd_custom {
        let s = &app.settings;
        out.push(form("pdCustom", "Customize Presenter Display", vec![
            json!({"t": "check", "key": "pd_current", "label": "Current Slide", "value": s.pd_current}),
            json!({"t": "check", "key": "pd_next", "label": "Next Slide", "value": s.pd_next}),
            json!({"t": "check", "key": "pd_notes", "label": "Presenter Notes", "value": s.pd_notes}),
            json!({"t": "check", "key": "pd_clock", "label": "Clock", "value": s.pd_clock}),
            json!({"t": "check", "key": "pd_timer", "label": "Timer", "value": s.pd_timer}),
            json!({"t": "check", "key": "pd_ready", "label": "Ready to Advance indicator", "value": s.pd_ready}),
        ]));
    }
    if app.confirm_clear_recording {
        out.push(json!({"id": "clearRecording", "title": "Clear Recording", "closable": false, "form": [
            label("Are you sure you want to clear the recording?"),
            buttons(&[("Cancel", "close", true), ("Clear", "done", true)]),
        ]}));
    }
    if app.spell_win.substitutions_open {
        let s = &app.settings;
        let mut rows = vec![
            json!({"t": "check", "key": "smart_quotes", "label": "Smart Quotes", "value": s.smart_quotes}),
            json!({"t": "check", "key": "smart_dashes", "label": "Smart Dashes", "value": s.smart_dashes}),
            json!({"t": "check", "key": "text_replacement", "label": "Text Replacement", "value": s.text_replacement}),
            json!({"t": "check", "key": "smart_links", "label": "Smart Links", "value": s.smart_links}),
            json!({"t": "check", "key": "smart_phone_links", "label": "Smart Phone Number Links", "value": s.smart_phone_links}),
            json!({"t": "section", "text": "Text replacements"}),
        ];
        rows.extend(crate::textedit::REPLACEMENTS.iter().map(|(from, to)| label(format!("{from}    →    {to}"))));
        out.push(form("substitutions", "Substitutions", rows));
    }
    if app.tools.arrange {
        let groups: [&[(&str, &str)]; 5] = ARRANGE;
        let rows = groups.iter().map(|g| json!({"t": "buttons", "items": g.iter().map(|(l, c)| json!([l, c, crate::menus::enabled(app, c)])).collect::<Vec<_>>()})).collect();
        out.push(form("arrange", "Arrange", rows));
    }
    if let Some((id, buf)) = &app.open_comment {
        if let Some(info) = app.doc.as_ref().and_then(|d| d.comment(*id)) {
            let who = if info.author.is_empty() { "Author".to_string() } else { info.author.clone() };
            let when = info.date.map(kn_model::comments::format_date).unwrap_or_default();
            out.push(form("slideComment", "Comment", vec![
                json!({"t": "label", "text": format!("{who}   {when}"), "bold": true}),
                json!({"t": "text", "key": "text", "value": buf, "rows": 4, "focus": true, "width": 280}),
                buttons(&[("Delete", "delete", true), ("Done", "close", true)]),
            ]));
        }
    }
    if app.show_adjust_image {
        let img = app.sel.first().copied().filter(|id| app.scene(app.cur).is_some_and(|sc| sc.drawables.iter().any(|d| d.id == *id && matches!(d.kind, kn_model::DrawableKind::Image(_)))));
        let mut rows = Vec::new();
        match img {
            None => rows.push(dim("Select a picture to adjust it.")),
            Some(id) => {
                let a = app.doc.as_ref().map(|d| d.image_adjustments(id)).unwrap_or_default();
                for (label, key, v, lo) in [("Exposure", "exposure", a.exposure, -100.0), ("Contrast", "contrast", a.contrast, -100.0), ("Saturation", "saturation", a.saturation, -100.0), ("Highlights", "highlights", a.highlights, -100.0), ("Shadows", "shadows", a.shadows, -100.0), ("Sharpness", "sharpness", a.sharpness, 0.0), ("De-noise", "denoise", a.denoise, 0.0), ("Temperature", "temperature", a.temperature, -100.0), ("Tint", "tint", a.tint, -100.0)] {
                    rows.push(json!({"t": "number", "key": key, "label": label, "value": (v * 100.0).round(), "min": lo, "max": 100, "step": 5, "suffix": " %"}));
                }
                rows.push(json!({"t": "section", "text": "Levels"}));
                rows.push(json!({"t": "number", "key": "bottom_level", "label": "Black", "value": (a.bottom_level * 100.0).round(), "min": 0, "max": 99, "step": 1, "suffix": " %"}));
                rows.push(json!({"t": "number", "key": "top_level", "label": "White", "value": (a.top_level * 100.0).round(), "min": 1, "max": 100, "step": 1, "suffix": " %"}));
                rows.push(json!({"t": "check", "key": "enhance", "label": "Enhance", "value": a.enhance}));
                rows.push(buttons(&[("Reset Image", "reset", !a.is_default())]));
            }
        }
        out.push(form("adjustImage", "Adjust Image", rows));
    }
    if app.show_object_list {
        let objs: Vec<(kn_format::ObjId, String, bool)> = app.scene(app.cur).map(|sc| sc.drawables.iter().rev().filter(|d| !d.from_template).map(|d| (d.id, crate::objectlist::label(d), d.locked)).collect()).unwrap_or_default();
        let items: Vec<String> = objs.iter().map(|(_, l, locked)| if *locked { format!("🔒 {l}") } else { l.clone() }).collect();
        let cur = objs.iter().position(|o| app.sel.first() == Some(&o.0));
        out.push(form("objectList", "Object List", vec![
            json!({"t": "list", "key": "pick", "items": items, "value": cur, "height": 300, "width": 260}),
            buttons(&[("Lock", "lock", cur.is_some()), ("Unlock", "unlock", cur.is_some())]),
        ]));
    }
    if app.insp.show_build_order {
        out.push(build_order(app));
    }
    // Letters' windows
    if app.tools.fonts {
        let cur = crate::editing::current_run(app);
        if app.font_names.is_none() {
            app.font_names = Some(crate::editing::font_names(app));
        }
        let st = cur.clone().unwrap_or_default();
        let family = kn_render::fonts::parse_ps_name(&st.font_name).0;
        out.push(json!({
            "id": "fonts", "title": "Fonts", "can": cur.is_some(), "query": app.tools.font_query, "names": app.font_names.clone().unwrap_or_default(),
            "family": family, "bold": st.bold, "italic": st.italic, "underline": st.underline, "strike": st.strike, "size": st.size, "color": col(st.color),
        }));
    }
    if app.tools.colors {
        let c = app.tools.color.unwrap_or(egui::Color32::from_rgb(0x1e, 0x88, 0xe5));
        out.push(json!({"id": "colors", "title": "Colors", "color": c.to_srgba_unmultiplied(), "saved": app.settings.saved_colors, "any": !app.sel.is_empty() || app.canvas.is_editing_text()}));
    }
    if app.spell_win.open {
        let w = &app.spell_win;
        let no_dicts = kn_ui::spell::dicts().is_some_and(|d| d.is_empty());
        out.push(json!({"id": "spelling", "title": "Spelling and Grammar", "done": w.word.is_none(), "noDicts": no_dicts, "word": w.word.clone().unwrap_or_default(), "guesses": w.guesses, "pick": w.pick}));
    }
    if app.dialogs.versions {
        let rows: Vec<Value> = app.doc.as_ref().and_then(|d| d.path.clone()).map(|p| crate::versions::list(&p)).unwrap_or_default().iter().map(|(p, secs)| json!([crate::versions::when(*secs), p.to_string_lossy()])).collect();
        out.push(json!({"id": "versions", "title": "All Versions", "versions": rows}));
    }
    if let Some(p) = &app.dialogs.password_prompt {
        let name = p.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        out.push(json!({"id": "password", "title": "Password", "file": name, "hint": p.hint, "wrong": !p.typed.is_empty()}));
    }
    if app.dialogs.set_password.is_some() {
        out.push(json!({"id": "setPassword", "title": "Set Password", "has": app.doc.as_ref().is_some_and(|d| d.password.is_some())}));
    }
    if let Some(r) = &app.replace_fonts {
        let rows: Vec<Value> = r.rows.iter().map(|(n, inst, to)| json!([n, inst, to])).collect();
        out.push(json!({"id": "replaceFonts", "title": "Replace Fonts", "rows": rows, "families": app.renderer.fonts.menu_families()}));
    }
    if app.media.record_open {
        out.push(json!({"id": "record", "title": "Record Audio", "seconds": 0}));
    }
    if let Some(url) = &app.webmedia.url_sheet {
        out.push(json!({"id": "webVideo", "title": "Add Web Video", "text": url, "ok": kn_ui::media::web::video_link(url.trim())}));
    }
    if app.show_help {
        let rows: Vec<Value> = crate::settings::SHORTCUTS.iter().map(|(what, keys)| json!([what, keys])).collect();
        out.push(json!({"id": "shortcuts", "title": "Keyboard Shortcuts", "sections": [["Keydeck", rows]]}));
    }
    if let Some(ed) = &app.chart_edit {
        let mut v = ed.api_state();
        v["id"] = json!("chartData");
        v["title"] = json!("Chart Data");
        out.push(v);
    }
    if app.show_all_tabs {
        let names: Vec<String> = (0..app.tabs.len()).map(|i| app.tab_name(i)).collect();
        out.push(json!({"id": "allTabs", "title": "All Tabs", "tabs": names, "active": app.active_tab}));
    }
    out
}

/// View > Show Build Order (inspector.rs build_order_window): the steps (object and effect,
/// a step that starts by itself marked "With" or "After"), the chosen step's Start and Delay.
fn build_order(app: &mut App) -> Value {
    use kn_model::buildedit::effect_name;
    let Some(scene) = app.scene(app.cur) else { return form("buildOrder", "Build Order", vec![dim("No slide")]) };
    let n = scene.build_order.len();
    let sel = app.insp.build_order_sel.min(n.saturating_sub(1));
    let items: Vec<String> = scene
        .build_order
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let b = scene.builds.iter().find(|b| b.id == step.build);
            let d = b.and_then(|b| scene.drawables.iter().find(|d| d.id == b.drawable));
            let object = d
                .map(|d| match &d.kind {
                    kn_model::DrawableKind::Shape(s) => s.text.as_ref().map(|t| t.text.trim().lines().next().unwrap_or("").chars().take(26).collect::<String>()).filter(|t| !t.is_empty()).unwrap_or_else(|| "Shape".into()),
                    kn_model::DrawableKind::Image(_) => "Image".into(),
                    kn_model::DrawableKind::Movie(m) => if m.audio_only { "Audio" } else { "Movie" }.into(),
                    kn_model::DrawableKind::Table(_) => "Table".into(),
                    kn_model::DrawableKind::Chart(_) => "Chart".into(),
                    kn_model::DrawableKind::Group(_) => "Group".into(),
                    _ => "Object".into(),
                })
                .unwrap_or_default();
            let effect = b.and_then(|b| effect_name(&b.effect, &b.kind).map(str::to_string)).or_else(|| b.map(|b| crate::inspector::pretty_effect(&b.effect))).unwrap_or_default();
            let join = if i > 0 && step.automatic { if step.with_previous { "  ⋯ with" } else { "  ↳ after" } } else { "" };
            format!("{}. {object} — {effect}{join}", i + 1)
        })
        .collect();
    if n == 0 {
        return form("buildOrder", "Build Order", vec![dim("No builds on this slide")]);
    }
    let step = &scene.build_order[sel];
    let start = match (step.automatic, step.with_previous) {
        (false, _) => 0,
        (true, true) => 1,
        (true, false) => 2,
    };
    let starts = if sel == 0 { vec!["On Click".to_string(), "With Transition".into(), "After Transition".into()] } else { vec!["On Click".to_string(), format!("With Build {sel}"), format!("After Build {sel}")] };
    form("buildOrder", "Build Order", vec![
        json!({"t": "list", "key": "select", "items": items, "value": sel, "height": 240}),
        buttons(&[("Move Up", "up", sel > 0), ("Move Down", "down", sel + 1 < n)]),
        json!({"t": "choice", "key": "start", "label": "Start", "options": starts, "value": start}),
        json!({"t": "number", "key": "delay", "label": "Delay", "value": step.delay, "min": 0, "max": 60, "step": 0.1, "decimals": 2, "suffix": " s"}),
        buttons(&[("Preview", "preview", true)]),
    ])
}

/// View > Show Arrange Tools: its rows of buttons (label, menu command).
const ARRANGE: [&[(&str, &str)]; 5] = [
    &[("Front", "Bring to Front"), ("Forward", "Bring Forward"), ("Backward", "Send Backward"), ("Back", "Send to Back")],
    &[("Left", "Align Objects > Left"), ("Center", "Align Objects > Center"), ("Right", "Align Objects > Right")],
    &[("Top", "Align Objects > Top"), ("Middle", "Align Objects > Middle"), ("Bottom", "Align Objects > Bottom")],
    &[("Distribute H", "Distribute Objects > Horizontally"), ("Distribute V", "Distribute Objects > Vertically"), ("Flip H", "Flip Horizontally"), ("Flip V", "Flip Vertically")],
    &[("Group", "Group"), ("Ungroup", "Ungroup"), ("Lock", "Lock"), ("Unlock", "Unlock")],
];

/// A window's field or button; false when the window isn't one of these.
pub fn set(app: &mut App, ctx: &egui::Context, win: &str, key: &str, v: &Value) -> bool {
    let s = v.as_str().map(str::to_string).unwrap_or_else(|| v.get("text").and_then(Value::as_str).unwrap_or_default().to_string());
    let n = v.as_f64().unwrap_or(0.0);
    let b = v.as_bool().unwrap_or(false);
    match (win, key) {
        ("slideLink", "kind") => app.link_dialog.kind = (n as usize).min(3),
        ("slideLink", "slideKind") => app.link_dialog.slide_kind = (n as usize).min(6),
        ("slideLink", "slideNo") => app.link_dialog.slide_no = (n as usize).max(1),
        ("slideLink", "text") => app.link_dialog.text = s,
        ("slideLink", "remove") => crate::links::apply_link(app, None, None),
        ("slideLink", "done") => {
            if let (Some(url), target) = app.link_dialog.result() {
                crate::links::apply_link(app, url, target);
            }
        }
        ("slideLink", _) => app.link_dialog.open = false,
        ("goToSlide", "text") => app.goto_buf = s,
        ("goToSlide", "done") => {
            if let Ok(n) = app.goto_buf.trim().parse::<usize>() {
                app.go_to(n.saturating_sub(1).min(app.slides.len().saturating_sub(1)));
            }
            app.show_goto = false;
        }
        ("goToSlide", _) => app.show_goto = false,
        ("find", "query") => app.find.query = s,
        ("find", "replace") if v.is_string() => app.find.replace = s,
        ("find", "case") => app.find.match_case = b,
        ("find", "words") => app.find.whole_words = b,
        ("find", "next" | "prev" | "replace" | "all") => crate::find::act(app, key),
        ("find", _) => app.find.open = false,
        ("equationTex", "text") => app.equation.source = s,
        ("equationTex", "done") => crate::equation::insert_equation(app),
        ("equationTex", _) => app.equation.open = false,
        ("condRule", "cmp") => app.cond_win.cmp = (n as usize).min(kn_model::tablecond::Cmp::ALL.len() - 1),
        ("condRule", "look") => app.cond_win.look = (n as usize).min(kn_model::tablecond::LOOKS.len() - 1),
        ("condRule", "value") => app.cond_win.value = s,
        ("condRule", "add") => crate::condwin::add_rule(app),
        ("condRule", _) => app.cond_win.open = false,
        ("saveTheme", "text") => app.dialogs.save_theme = Some(s),
        ("saveTheme", "done") => {
            if let Some(name) = app.dialogs.save_theme.take().filter(|n| !n.trim().is_empty()) {
                crate::versions::save_theme(app, name.trim());
            }
        }
        ("saveTheme", _) => app.dialogs.save_theme = None,
        ("language", "pick") => {
            if let Some((code, _)) = crate::versions::LANGUAGES.get(n as usize) {
                crate::versions::set_language(app, code);
            }
        }
        ("language", _) => app.dialogs.language = false,
        ("placeholder", "tag" | "display") => {
            if let Some(p) = app.dialogs.define_placeholder.as_mut() {
                if key == "tag" { p.1 = s } else { p.2 = s }
            }
        }
        ("placeholder", "done") => {
            if let Some((id, tag, display, _)) = app.dialogs.define_placeholder.take() {
                crate::versions::define_placeholder(app, id, &tag, &display);
            }
        }
        ("placeholder", _) => app.dialogs.define_placeholder = None,
        ("warnings", "replace") => {
            app.warnings.open = false;
            crate::replacefonts::open(app);
        }
        ("warnings", _) => app.warnings.open = false,
        ("rename", "text") => app.fileops.rename = Some(s),
        ("rename", "done") => {
            let buf = app.fileops.rename.take().unwrap_or_default();
            let ext = app.doc.as_ref().and_then(|d| d.path.as_ref()).and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "key".into());
            let name = buf.trim().trim_end_matches(&format!(".{ext}")).to_string();
            if let Some(dir) = app.doc.as_ref().and_then(|d| d.path.as_ref()).and_then(|p| p.parent()).map(|p| p.to_path_buf()).filter(|_| !name.is_empty() && !name.contains('/')) {
                crate::filemenu::relocate(app, ctx, dir.join(format!("{name}.{ext}")));
            }
        }
        ("rename", _) => app.fileops.rename = None,
        ("pdCustom", "close") => app.show_pd_custom = false,
        ("pdCustom", k) => {
            let st = &mut app.settings;
            match k {
                "pd_current" => st.pd_current = b,
                "pd_next" => st.pd_next = b,
                "pd_notes" => st.pd_notes = b,
                "pd_clock" => st.pd_clock = b,
                "pd_timer" => st.pd_timer = b,
                "pd_ready" => st.pd_ready = b,
                _ => {}
            }
            st.save();
        }
        ("clearRecording", "done") => {
            app.confirm_clear_recording = false;
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Clear Recording");
                doc.clear_recording(&mut app.hist);
                doc.end(&mut app.hist);
            }
            app.notify("Recording cleared");
        }
        ("clearRecording", _) => app.confirm_clear_recording = false,
        ("substitutions", "close") => app.spell_win.substitutions_open = false,
        ("substitutions", k) => {
            let st = &mut app.settings;
            match k {
                "smart_quotes" => st.smart_quotes = b,
                "smart_dashes" => st.smart_dashes = b,
                "text_replacement" => st.text_replacement = b,
                "smart_links" => st.smart_links = b,
                "smart_phone_links" => st.smart_phone_links = b,
                _ => {}
            }
            st.save();
        }
        ("buildOrder", k) => {
            use kn_model::buildedit::BuildStart;
            let Some(scene) = app.scene(app.cur) else { return true };
            let Some(slide) = app.current_slide().map(|s| s.slide) else { return true };
            let n_steps = scene.build_order.len();
            let sel = app.insp.build_order_sel.min(n_steps.saturating_sub(1));
            let chunk = app.doc.as_ref().and_then(|d| d.file.obj(slide)).map(|s| s.ref_ids("buildChunks")).unwrap_or_default().get(sel).copied();
            match k {
                "select" => {
                    let i = (n as usize).min(n_steps.saturating_sub(1));
                    app.insp.build_order_sel = i;
                    let d = scene.build_order.get(i).and_then(|st| scene.builds.iter().find(|b| b.id == st.build)).map(|b| b.drawable);
                    if let Some(d) = d {
                        app.sel = vec![d];
                    }
                }
                "up" | "down" => {
                    let to = if k == "up" { sel.saturating_sub(1) } else { (sel + 1).min(n_steps.saturating_sub(1)) };
                    if let Some(doc) = app.doc.as_mut() {
                        doc.begin(&mut app.hist, "Reorder Builds");
                        doc.move_build_step(&mut app.hist, slide, sel, to);
                        doc.end(&mut app.hist);
                    }
                    app.insp.build_order_sel = to;
                    app.bump(slide);
                }
                "start" | "delay" => {
                    let Some(step) = scene.build_order.get(sel) else { return true };
                    let cur = match (step.automatic, step.with_previous) {
                        (false, _) => BuildStart::OnClick,
                        (true, true) => BuildStart::WithPrevious,
                        (true, false) => BuildStart::AfterPrevious,
                    };
                    let (start, delay) = if k == "start" { ([BuildStart::OnClick, BuildStart::WithPrevious, BuildStart::AfterPrevious][(n as usize).min(2)], step.delay) } else { (cur, n.clamp(0.0, 60.0)) };
                    if let (Some(cid), Some(doc)) = (chunk, app.doc.as_mut()) {
                        doc.begin_merge(&mut app.hist, "Build Start");
                        doc.set_build_start_mode(&mut app.hist, cid, start, delay);
                        doc.end(&mut app.hist);
                    }
                    app.bump(slide);
                }
                "preview" => {
                    app.insp.show_build_order = false;
                    app.start_slideshow(app.cur);
                }
                _ => app.insp.show_build_order = false,
            }
        }
        ("slideComment", k) => {
            let Some((id, buf)) = app.open_comment.take() else { return true };
            let slide = app.current_slide().map(|s| s.slide);
            let info = app.doc.as_ref().and_then(|d| d.comment(id));
            match k {
                "text" => app.open_comment = Some((id, s)),
                "delete" => {
                    if let Some(doc) = app.doc.as_mut() {
                        doc.begin(&mut app.hist, "Delete Comment");
                        doc.delete_comment(&mut app.hist, id);
                        doc.end(&mut app.hist);
                    }
                }
                _ => {
                    if let (Some(info), Some(doc)) = (info, app.doc.as_mut()) {
                        if buf != info.text {
                            doc.begin(&mut app.hist, "Edit Comment");
                            doc.set_comment_text(&mut app.hist, info.id, &buf);
                            doc.end(&mut app.hist);
                        }
                    }
                }
            }
            if let Some(sl) = slide {
                app.bump(sl);
            }
        }
        ("adjustImage", "close") => app.show_adjust_image = false,
        ("adjustImage", k) => {
            let Some(id) = app.sel.first().copied() else { return true };
            let mut a = app.doc.as_ref().map(|d| d.image_adjustments(id)).unwrap_or_default();
            let x = (n / 100.0) as f32;
            match k {
                "exposure" => a.exposure = x,
                "contrast" => a.contrast = x,
                "saturation" => a.saturation = x,
                "highlights" => a.highlights = x,
                "shadows" => a.shadows = x,
                "sharpness" => a.sharpness = x,
                "denoise" => a.denoise = x,
                "temperature" => a.temperature = x,
                "tint" => a.tint = x,
                "bottom_level" => a.bottom_level = x.min(a.top_level - 0.01),
                "top_level" => a.top_level = x.max(a.bottom_level + 0.01),
                "enhance" => a.enhance = b,
                "reset" => a = Default::default(),
                _ => return true,
            }
            crate::imageadj::set(app, id, a);
        }
        ("objectList", "close") => app.show_object_list = false,
        ("objectList", k) => {
            let objs: Vec<kn_format::ObjId> = app.scene(app.cur).map(|sc| sc.drawables.iter().rev().filter(|d| !d.from_template).map(|d| d.id).collect()).unwrap_or_default();
            match k {
                "pick" => {
                    if let Some(id) = objs.get(n as usize) {
                        app.sel = vec![*id];
                    }
                }
                "lock" | "unlock" => app.command(ctx, if k == "lock" { "Lock" } else { "Unlock" }),
                _ => {}
            }
        }
        ("arrange", "close") => app.tools.arrange = false,
        ("arrange", cmd) => app.command(ctx, cmd),
        ("fonts", k) => {
            use kn_model::format::TextProp;
            match k {
                "query" => app.tools.font_query = s,
                "family" => {
                    let ps = app.renderer.fonts.ps_name_for_family(&s).unwrap_or(s);
                    crate::editing::set_text(app, "Font", TextProp::Font(ps));
                }
                "bold" => crate::editing::set_text(app, "Bold", TextProp::Bold(b)),
                "italic" => crate::editing::set_text(app, "Italic", TextProp::Italic(b)),
                "underline" => crate::editing::set_text(app, "Underline", TextProp::Underline(b)),
                "strike" => crate::editing::set_text(app, "Strikethrough", TextProp::Strike(b)),
                "size" => crate::editing::set_text(app, "Font Size", TextProp::Size((n as f32).clamp(1.0, 1638.0))),
                "color" => {
                    let a = v.as_array().cloned().unwrap_or_default();
                    let f = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
                    crate::editing::set_text(app, "Text Color", TextProp::Color(kn_model::Color { r: f(0), g: f(1), b: f(2), a: 1.0 }));
                }
                _ => app.tools.fonts = false,
            }
        }
        ("colors", k) => {
            let c = v.as_array().map(|a| {
                let f = |i: usize| a.get(i).and_then(Value::as_u64).unwrap_or(255) as u8;
                egui::Color32::from_rgba_unmultiplied(f(0), f(1), f(2), f(3))
            });
            match k {
                "color" => app.tools.color = c.or(app.tools.color),
                "apply" => {
                    let c = c.or(app.tools.color).unwrap_or(egui::Color32::WHITE);
                    let colr = kn_model::Color { r: c.r() as f32 / 255.0, g: c.g() as f32 / 255.0, b: c.b() as f32 / 255.0, a: c.a() as f32 / 255.0 };
                    if app.canvas.is_editing_text() {
                        crate::editing::set_text(app, "Text Color", kn_model::format::TextProp::Color(colr));
                    } else {
                        let f = kn_model::format::FillSpec::Color(colr);
                        crate::editing::apply(app, "Fill", |doc, h, id| doc.set_fill(h, id, &f));
                    }
                }
                "saved" => {
                    app.settings.saved_colors = s;
                    app.settings.save();
                }
                _ => app.tools.colors = false,
            }
        }
        ("spelling", "pick") => {
            let w = &mut app.spell_win;
            w.pick = n as usize;
            if let Some(g) = w.guesses.get(w.pick) {
                w.change_to = g.clone();
            }
        }
        ("spelling", "change" | "next" | "ignore" | "learn") => crate::spellwin::spell_act(app, key),
        ("spelling", _) => app.spell_win.open = false,
        ("versions", "restore") => crate::versions::restore_version(app, std::path::Path::new(&s)),
        ("versions", _) => app.dialogs.versions = false,
        ("password", "done") => {
            if let Some(p) = app.dialogs.password_prompt.take() {
                app.open_path_with(p.path.clone(), Some((s, p.hint.clone())));
            }
        }
        ("password", _) => app.dialogs.password_prompt = None,
        ("setPassword", k) => {
            app.dialogs.set_password = None;
            let f = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
            match k {
                "remove" => crate::versions::set_password(app, "", ""),
                "done" => crate::versions::set_password(app, &f("password"), &f("hint")),
                _ => {}
            }
        }
        ("replaceFonts", "pick") => {
            let i = v.get("i").and_then(Value::as_u64).unwrap_or(0) as usize;
            let to = v.get("to").and_then(Value::as_str).map(str::to_string);
            if let Some(row) = app.replace_fonts.as_mut().and_then(|r| r.rows.get_mut(i)) {
                row.2 = to;
            }
        }
        ("replaceFonts", "replace") => {
            if let Some(st) = app.replace_fonts.take() {
                crate::replacefonts::replace(app, &st);
            }
        }
        ("replaceFonts", _) => app.replace_fonts = None,
        ("record", _) => app.media.record_open = false,
        ("webVideo", "edit") => app.webmedia.url_sheet = Some(s),
        ("webVideo", "done") => {
            app.webmedia.url_sheet = None;
            crate::webmedia::start_fetch(app, &s);
        }
        ("webVideo", _) => {
            app.webmedia.url_sheet = None;
            app.webmedia.replacing = None;
        }
        ("shortcuts", _) => app.show_help = false,
        ("chartData", "close") => app.chart_edit = None,
        ("chartData", _) => {
            if let Some(mut ed) = app.chart_edit.take() {
                let act = ed.api_action(key, v);
                if act != kn_ui::chartdata::DataAction::None {
                    let slide = app.current_slide().map(|s| s.slide);
                    if let Some(doc) = app.doc.as_mut() {
                        doc.begin(&mut app.hist, act.undo_name());
                        act.apply(doc, &mut app.hist, ed.chart);
                        doc.end(&mut app.hist);
                    }
                    if let Some(sl) = slide {
                        app.bump(sl);
                    }
                    let c = app.scene(app.cur).and_then(|sc| sc.drawables.into_iter().find(|d| d.id == ed.chart)).and_then(|d| match d.kind {
                        kn_model::DrawableKind::Chart(c) => Some(c),
                        _ => None,
                    });
                    if let Some(d) = app.doc.as_ref() {
                        ed.refresh(d, c.as_ref());
                    }
                }
                app.chart_edit = Some(ed);
            }
        }
        ("allTabs", k) => {
            if k == "pick" {
                app.select_tab(ctx, n as usize);
            }
            app.show_all_tabs = false;
        }
        _ => return false,
    }
    true
}
