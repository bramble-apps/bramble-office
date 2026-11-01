//! The desktop's small windows as data (app.rs dialogs, settings.rs, the toolbar's Customize
//! sheet), for front ends that draw them (the suite's windows.dart): `state` lists the open ones,
//! `set` presses their buttons and changes their fields (keys "<window>.<what>").

use serde_json::{json, Value};

use crate::app::App;

pub fn state(app: &App) -> Value {
    let mut out: Vec<Value> = Vec::new();
    if let Some(c) = chooser_state(app) {
        out.push(c);
    }
    if let Some((path, hint, _, wrong)) = &app.password {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        out.push(json!({"id": "password", "title": "Password", "file": name, "hint": hint, "wrong": wrong}));
    }
    if app.set_password.is_some() {
        out.push(json!({"id": "setPassword", "title": "Set Password", "has": app.wb.as_ref().is_some_and(|w| w.doc.password.is_some())}));
    }
    if let Some((_, _, _, text, author)) = &app.comment_dialog {
        out.push(json!({"id": "comment", "title": "Add Comment", "author": author, "text": text}));
    }
    if app.tb.customizing {
        // View > Customize Toolbar… (toolbar.rs sheet): every item, which are in the toolbar,
        // the default set and the display mode
        let in_bar = crate::toolbar::items(app);
        let items: Vec<Value> = crate::toolbar::ITEMS
            .iter()
            .map(|d| json!({"id": d.id, "label": d.label, "tip": d.tip, "icon": format!("{:?}", d.icon), "used": !crate::toolbar::is_space(d.id) && in_bar.iter().any(|x| x == d.id)}))
            .collect();
        let defaults: Vec<&str> = crate::toolbar::DEFAULT.iter().copied().filter(|id| !crate::toolbar::is_space(id)).collect();
        out.push(json!({"id": "customize", "title": "", "items": items, "defaults": defaults, "mode": app.settings.toolbar_mode}));
    }
    if app.show_settings {
        out.push(settings_state(app));
    }
    if app.show_about {
        out.push(json!({"id": "about", "title": "About Values", "app": "Values", "line": "Reads and writes Numbers (.numbers) spreadsheets and Excel workbooks.", "version": env!("CARGO_PKG_VERSION")}));
    }
    if app.hub.open {
        let shape = |s: &kn_ui::contenthub::AppShape| {
            use kn_ui::contenthub::Glyph;
            let glyph = match &s.glyph {
                Glyph::Text => json!("text"),
                Glyph::Line { arrow } => json!(if *arrow { "arrow" } else { "line" }),
                Glyph::Curve => json!("curve"),
                Glyph::Label => json!("label"),
                Glyph::Path { path, w, h } => kn_ui::contenthub::api::outline(path, *w, *h),
            };
            json!({"key": s.key, "name": s.name, "glyph": glyph})
        };
        let (basic, mine) = crate::chrome::hub_shapes();
        out.push(json!({"id": "hub", "title": "Content Hub", "tab": app.hub.tab_index(), "basic": basic.iter().map(shape).collect::<Vec<_>>(), "mine": mine.iter().map(shape).collect::<Vec<_>>()}));
    }
    if let Some(ed) = &app.chart_edit {
        let mut v = ed.api_state();
        v["id"] = json!("chartData");
        v["title"] = json!("Chart Data");
        out.push(v);
    }
    if app.show_colors {
        let any = app.sel.is_some() || app.canvas.obj.is_some();
        out.push(json!({"id": "colors", "title": "Colors", "color": app.colors_pick.to_srgba_unmultiplied(), "saved": app.settings.saved_colors, "any": any, "apply": "Fill Selection"}));
    }
    if app.print_setup_open {
        if let Some(p) = print_state(app) {
            out.push(p);
        }
    }
    if app.show_shortcuts {
        out.push(json!({"id": "shortcuts", "title": "Keyboard Shortcuts", "sections": shortcut_sections()}));
    }
    if let Some(le) = &app.link_edit {
        // Format > Add Link (links.rs window): Numbers' link popover
        let sheets: Vec<String> = app.sheets().iter().map(|s| s.name.clone()).collect();
        out.push(json!({
            "id": "valuesLink", "title": "", "kinds": crate::links::KINDS, "kind": le.kind, "addr": le.addr, "subject": le.subject,
            "display": le.display, "hasText": le.text.is_some(), "sheet": le.sheet, "sheets": sheets, "existing": le.existing,
        }));
    }
    if app.stocks.open {
        use nm_core::kn_model::formula::STOCK_ARGS;
        let attrs: Vec<Value> = STOCK_ARGS.iter().map(|(_, n, id)| json!([n, id.is_some()])).collect();
        out.push(json!({"id": "stock", "title": "Stock Quote", "symbol": app.stocks.symbol, "attr": app.stocks.attr, "attrs": attrs, "busy": app.stocks.busy()}));
    }
    if let Some(sp) = &app.spelling {
        let no_dicts = kn_ui::spell::dicts().is_some_and(|d| d.is_empty());
        out.push(json!({"id": "spelling", "title": "Spelling and Grammar", "done": sp.done, "noDicts": no_dicts, "word": sp.word, "guesses": sp.guesses, "pick": sp.pick, "doneText": "No more misspelled words in this sheet."}));
    }
    if app.show_versions {
        let all = app.doc().and_then(|d| d.path.clone()).map(|p| crate::fileops::list(&p)).unwrap_or_default();
        let rows: Vec<Value> = all.iter().map(|(p, secs)| json!([crate::fileops::when(*secs), p.to_string_lossy()])).collect();
        out.push(json!({"id": "versions", "title": "All Versions", "versions": rows, "empty": "No earlier versions yet: a version is kept each time the spreadsheet is saved over."}));
    }
    if app.show_fonts {
        let cur = crate::fonts::current(app);
        let st = cur.clone().unwrap_or_default();
        let family = kn_render::fonts::parse_ps_name(&st.font_name).0;
        let c = st.color;
        out.push(json!({
            "id": "fonts", "title": "Fonts", "can": cur.is_some(), "query": app.font_query,
            "names": app.font_names.clone().unwrap_or_else(|| crate::editing::font_names(app)), "family": family,
            "bold": st.bold, "italic": st.italic, "underline": st.underline, "strike": st.strike, "size": st.size,
            "color": [c.r, c.g, c.b, 1.0], "hint": "Select cells or a text box to change its font.",
        }));
    }
    if let Some(r) = &app.replace_fonts {
        let rows: Vec<Value> = r.rows.iter().map(|(n, inst, to)| json!([n, inst, to])).collect();
        out.push(json!({"id": "replaceFonts", "title": "Replace Fonts", "rows": rows, "families": app.canvas.tiles.font_families()}));
    }
    if let Some(list) = &app.warnings {
        out.push(json!({"id": "warnings", "title": "Document Warnings", "list": list, "missingFonts": list.iter().any(|w| w.starts_with("The font"))}));
    }
    if let Some(name) = &app.templates.saving {
        out.push(json!({"id": "saveTemplate", "title": "Save as Template", "text": name}));
    }
    if let Some((lang, region)) = &app.language_edit {
        use crate::templates::{LANGUAGES, REGIONS};
        out.push(json!({
            "id": "language", "title": "Language & Region", "lang": lang, "region": region,
            "langs": LANGUAGES.iter().map(|(c, n)| json!([c, n])).collect::<Vec<_>>(), "regions": REGIONS.iter().map(|(c, n)| json!([c, n])).collect::<Vec<_>>(),
            "sample": crate::templates::language_sample(region),
        }));
    }
    if app.equation.open {
        out.push(json!({
            "id": "equation", "title": "Equation", "text": app.equation.source, "preview": crate::equation::latex_to_text(&app.equation.source),
            "error": app.equation.error, "done": if app.equation.editing.is_some() { "Update" } else { "Insert" },
        }));
    }
    if app.media.record_open {
        out.push(json!({"id": "record", "title": "Record Audio", "seconds": crate::media::recording_secs(app)}));
    }
    if let Some(url) = &app.media.url_sheet {
        out.push(json!({"id": "webVideo", "title": "Add Web Video", "text": url, "ok": kn_ui::media::web::video_link(url.trim())}));
    }
    if let Some(music) = app.media.open {
        let files = BROWSER.with(|b| b.borrow().clone());
        out.push(json!({
            "id": "mediaBrowser", "title": if music { "Music" } else { "Photos and Videos" }, "music": music, "query": app.media.query,
            "files": files.iter().map(|p| json!([p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), p.to_string_lossy()])).collect::<Vec<_>>(),
            "empty": if music { "No songs in ~/Music" } else { "No pictures or movies in ~/Pictures or ~/Videos" },
        }));
    }
    if let Some((name, dir)) = &app.doc_title {
        // the title popover (chrome.rs title_bar): the name, where it is, its format
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let mut places: Vec<std::path::PathBuf> = dir.iter().cloned().collect();
        for sub in ["Documents", "Desktop", ""] {
            if let Some(h) = &home {
                let p = if sub.is_empty() { h.clone() } else { h.join(sub) };
                if p.is_dir() && !places.contains(&p) {
                    places.push(p);
                }
            }
        }
        let show = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string());
        let f = DOC_FORMAT.with(|f| f.get());
        let note = match f {
            0 => "Saves as an Excel workbook. Only what Excel can hold can be added.",
            1 => "Saves as an Excel workbook that also keeps every Values feature for Values (other apps see the Excel part).",
            _ => "Saves as a Numbers spreadsheet.",
        };
        out.push(json!({
            "id": "docTitle", "title": "", "name": name, "dir": dir.as_ref().map(|d| d.to_string_lossy().into_owned()),
            "dirName": dir.as_deref().map(show), "places": places.iter().map(|p| json!([show(p), p.to_string_lossy()])).collect::<Vec<_>>(),
            "format": f, "formats": ["Excel workbook", "Excel + Values features", "Numbers spreadsheet"], "was": crate::chrome::doc_format(app), "note": note,
        }));
    }
    Value::Array(out)
}

thread_local! {
    /// The title popover's format pick (chrome.rs keeps it in egui's memory).
    static DOC_FORMAT: std::cell::Cell<u8> = const { std::cell::Cell::new(2) };
    /// The media browser's files (listed when it opens or its search changes).
    static BROWSER: std::cell::RefCell<Vec<std::path::PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The media browser's list, again (after it opens or its search changes).
pub fn list_browser(app: &mut App) {
    if let Some(music) = app.media.open {
        let files = crate::media::browser_files(app, music);
        BROWSER.with(|b| *b.borrow_mut() = files);
    }
}

/// The menus' shortcuts by menu, then the sheet's own keys.
fn shortcut_sections() -> Vec<Value> {
    use crate::menus::Item;
    fn walk(items: &[Item], parent: &str, out: &mut Vec<(String, String)>) {
        for it in items {
            if !it.children.is_empty() {
                walk(&it.children, &it.label, out);
            } else if let Some(s) = &it.shortcut {
                out.push((crate::menus::command_name(parent, &it.label), s.clone()));
            }
        }
    }
    let mut out = Vec::new();
    for m in crate::menus::menus() {
        let mut rows = Vec::new();
        walk(&m.children, &m.label, &mut rows);
        if !rows.is_empty() {
            out.push(json!([m.label, rows]));
        }
    }
    let rows: Vec<(String, String)> = [
        ("Return", "Enter the cell and go down"),
        ("Tab / Shift+Tab", "Enter the cell and go right / left"),
        ("Arrows", "Move the selection (Shift: extend it)"),
        ("Alt+Arrows", "Add a row or column there"),
        ("Typing", "Replace the selected cell"),
        ("F2", "Edit the selected cell"),
        ("Escape", "Leave the cell as it was"),
    ]
    .iter()
    .map(|(k, w)| (w.to_string(), k.to_string()))
    .collect();
    out.push(json!(["Cells", rows]));
    out
}

fn text(v: &Value) -> String {
    v.get("text").and_then(Value::as_str).unwrap_or_default().to_string()
}

/// A window's button: "<window>.done" with what was typed ({"text": …}), "<window>.cancel",
/// or a window's own.
pub fn set(app: &mut App, ctx: &egui::Context, key: &str, v: &Value) {
    let (win, button) = key.split_once('.').unwrap_or((key, ""));
    let cancel = button == "cancel";
    match win {
        "password" => {
            if cancel {
                app.password = None;
                return;
            }
            let Some((path, hint, _, _)) = app.password.clone() else { return };
            match nm_core::kn_model::Document::open_with_password(path.as_path(), &text(v), &hint) {
                Ok(doc) => {
                    app.password = None;
                    app.load(nm_core::Workbook { doc });
                }
                Err(_) => {
                    if let Some(p) = &mut app.password {
                        p.3 = true;
                    }
                }
            }
        }
        "setPassword" => {
            let choice = match button {
                "remove" => Some(None),
                "done" => {
                    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
                    Some(Some((s("password"), s("hint"))))
                }
                _ => None,
            };
            app.set_password = None;
            if let (Some(c), Some(w)) = (choice, &mut app.wb) {
                w.doc.password = c;
                w.doc.dirty = true;
                app.notify("The password applies from the next save.");
            }
        }
        "comment" => {
            if let Some((t, r, c, _, author)) = app.comment_dialog.take() {
                let body = text(v).trim().to_string();
                let author = v.get("author").and_then(Value::as_str).map(str::to_string).unwrap_or(author);
                if !cancel && !body.is_empty() {
                    if let Some(Err(e)) = app.edit("Add Comment", |d, h| nm_core::comments::add_cell_comment(d, h, t, r, c, &body, author.trim())) {
                        app.notify(format!("Couldn't add the comment: {e}"));
                    }
                }
            }
        }
        "about" => app.show_about = false,
        "chooser" => chooser_set(app, button, v),
        "hub" => {
            use kn_ui::contenthub::Picked;
            let key = v.as_str().unwrap_or_default().to_string();
            match button {
                "app" => crate::chrome::insert_from_hub(app, Picked::App(key)),
                "remove" => crate::chrome::insert_from_hub(app, Picked::Remove(key)),
                "lib" => {
                    if let Some(s) = nm_core::kn_model::shapelib::shapes().get(v.as_u64().unwrap_or(u64::MAX) as usize) {
                        crate::chrome::insert_from_hub(app, Picked::Library(s));
                    }
                }
                "image" => {
                    let p = v["path"].as_str().unwrap_or_default();
                    if let Ok(bytes) = std::fs::read(p) {
                        crate::media::insert_picture(app, bytes, v["name"].as_str().unwrap_or("picture.jpg"));
                    }
                }
                _ => app.hub.open = false,
            }
        }
        "chartData" => match button {
            "close" => app.chart_edit = None,
            _ => {
                if let Some(mut ed) = app.chart_edit.take() {
                    let act = ed.api_action(button, v);
                    if act != kn_ui::chartdata::DataAction::None {
                        let id = ed.chart;
                        app.edit(act.undo_name(), |d, h| act.apply(d, h, id));
                        let c = app.drawables.iter().find(|d| d.id == id).and_then(|d| match &d.kind {
                            nm_core::kn_model::DrawableKind::Chart(c) => Some(c.clone()),
                            _ => None,
                        });
                        if let Some(d) = app.doc() {
                            ed.refresh(d, c.as_ref());
                        }
                    }
                    app.chart_edit = Some(ed);
                }
            }
        },
        "colors" => {
            let c = v.as_array().map(|a| {
                let f = |i: usize| a.get(i).and_then(Value::as_u64).unwrap_or(255) as u8;
                egui::Color32::from_rgba_unmultiplied(f(0), f(1), f(2), f(3))
            });
            match button {
                "color" => app.colors_pick = c.unwrap_or(app.colors_pick),
                "apply" => {
                    let c = c.unwrap_or(app.colors_pick);
                    app.fill_selection(c);
                }
                "saved" => {
                    app.settings.saved_colors = v.as_str().unwrap_or_default().to_string();
                    app.settings.save();
                    app.color_state.saved.clear();
                }
                _ => app.show_colors = false,
            }
        }
        "printSetup" => print_set(app, button, v),
        "shortcuts" => app.show_shortcuts = false,
        "settings" => set_setting(app, ctx, button, v),
        // the toolbar's items as dragged into place ({"items": [ids]}), the default set, the mode
        "valuesLink" => {
            let Some(mut le) = app.link_edit.clone() else { return };
            let st = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
            if let Some(k) = st("kind") {
                le.kind = k;
            }
            if let Some(a) = st("addr") {
                le.addr = a;
            }
            if let Some(a) = st("subject") {
                le.subject = a;
            }
            if let Some(a) = st("display") {
                le.display = a;
            }
            if let Some(i) = v.get("sheet").and_then(Value::as_u64) {
                le.sheet = i as usize;
            }
            match button {
                "remove" => crate::links::finish(app, le, true, false),
                // Add Link / Open Link / Compose Email / Call / Go to Sheet
                "go" => {
                    let follow = le.existing || le.kind != "Webpage";
                    crate::links::finish(app, le, false, follow);
                }
                "done" => crate::links::finish(app, le, false, false),
                "cancel" => app.link_edit = None,
                _ => app.link_edit = Some(le),
            }
        }
        "stock" => match button {
            "edit" => {
                if let Some(t) = v.get("symbol").and_then(Value::as_str) {
                    app.stocks.symbol = t.to_string();
                }
                if let Some(i) = v.get("attr").and_then(Value::as_u64) {
                    app.stocks.attr = (i as usize).min(nm_core::kn_model::formula::STOCK_ARGS.len() - 1);
                }
            }
            "insert" => crate::stocks::insert_quote(app, None),
            "refresh" => crate::stocks::refresh(app, None),
            _ => app.stocks.open = false,
        },
        "spelling" => {
            let word = app.spelling.as_ref().map(|s| s.word.clone()).unwrap_or_default();
            match button {
                "pick" => {
                    if let Some(s) = app.spelling.as_mut() {
                        s.pick = v.as_u64().unwrap_or(0) as usize;
                    }
                }
                "change" => crate::spelling::change(app),
                "next" => crate::spelling::find_next(app),
                "ignore" => {
                    kn_ui::spell::ignore(&word);
                    crate::spelling::find_next(app);
                }
                "learn" => {
                    kn_ui::spell::learn(&word);
                    crate::spelling::find_next(app);
                }
                _ => app.spelling = None,
            }
        }
        "versions" => match button {
            "restore" => {
                if let Some(path) = app.doc().and_then(|d| d.path.clone()) {
                    let v = std::path::PathBuf::from(v.as_str().unwrap_or_default());
                    crate::fileops::restore_version(app, &path, &v);
                }
            }
            _ => app.show_versions = false,
        },
        "fonts" => {
            use nm_core::kn_model::format::TextProp;
            let b = v.as_bool().unwrap_or(false);
            let n = v.as_f64().unwrap_or(0.0) as f32;
            match button {
                "query" => app.font_query = v.as_str().unwrap_or_default().to_string(),
                "family" => {
                    let f = v.as_str().unwrap_or_default();
                    let ps = app.canvas.tiles.ps_name_for_family(f).unwrap_or_else(|| f.replace(' ', ""));
                    crate::editing::set_text(app, "Font", TextProp::Font(ps));
                }
                "bold" => crate::editing::set_text(app, "Bold", TextProp::Bold(b)),
                "italic" => crate::editing::set_text(app, "Italic", TextProp::Italic(b)),
                "underline" => crate::editing::set_text(app, "Underline", TextProp::Underline(b)),
                "strike" => crate::editing::set_text(app, "Strikethrough", TextProp::Strike(b)),
                "size" => crate::editing::set_text(app, "Font Size", TextProp::Size(n.clamp(1.0, 1638.0))),
                "color" => {
                    let a = v.as_array().cloned().unwrap_or_default();
                    let f = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
                    crate::editing::set_text(app, "Text Color", TextProp::Color(nm_core::kn_model::Color { r: f(0), g: f(1), b: f(2), a: 1.0 }));
                }
                _ => app.show_fonts = false,
            }
        }
        "replaceFonts" => match button {
            "pick" => {
                let i = v.get("i").and_then(Value::as_u64).unwrap_or(0) as usize;
                let to = v.get("to").and_then(Value::as_str).map(str::to_string);
                if let Some(row) = app.replace_fonts.as_mut().and_then(|r| r.rows.get_mut(i)) {
                    row.2 = to;
                }
            }
            "replace" => {
                if let Some(st) = app.replace_fonts.take() {
                    crate::fonts::replace(app, &st);
                }
            }
            _ => app.replace_fonts = None,
        },
        "warnings" => match button {
            "replace" => {
                app.warnings = None;
                crate::fonts::open_replace(app);
            }
            _ => app.warnings = None,
        },
        "saveTemplate" => match button {
            "edit" => app.templates.saving = Some(text(v)),
            "add" => {
                app.templates.saving = None;
                crate::templates::add_to_chooser(app, &text(v));
            }
            // Save…: the template's bytes to the file the front end picked ({"path": …})
            "save" => {
                app.templates.saving = None;
                let path = v.get("path").and_then(Value::as_str).unwrap_or_default();
                match crate::templates::template_bytes(app) {
                    Some(b) if !path.is_empty() => {
                        if let Err(e) = std::fs::write(path, b) {
                            app.notify(format!("Couldn't save the template: {e}"));
                        }
                    }
                    _ => {}
                }
            }
            _ => app.templates.saving = None,
        },
        "language" => {
            let Some((mut l, mut r)) = app.language_edit.clone() else { return };
            if let Some(x) = v.get("lang").and_then(Value::as_str) {
                l = x.to_string();
            }
            if let Some(x) = v.get("region").and_then(Value::as_str) {
                r = x.to_string();
            }
            match button {
                "ok" => {
                    app.language_edit = None;
                    crate::templates::set_language(app, &l, &r);
                }
                "cancel" | "close" => app.language_edit = None,
                _ => app.language_edit = Some((l, r)),
            }
        }
        "equation" => match button {
            "edit" => {
                app.equation.source = text(v);
                app.equation.error = None;
            }
            "done" => {
                app.equation.source = text(v);
                crate::equation::insert_now(app);
            }
            _ => app.equation = Default::default(),
        },
        "record" => match button {
            "start" => crate::media::record_start(app),
            "stop" => {
                crate::media::record_stop(app, true);
                app.media.record_open = false;
            }
            _ => {
                crate::media::record_stop(app, false);
                app.media.record_open = false;
            }
        },
        "webVideo" => match button {
            "edit" => app.media.url_sheet = Some(text(v)),
            "done" => crate::media::web_go(app, &text(v)),
            _ => app.media.url_sheet = None,
        },
        "mediaBrowser" => match button {
            "query" => {
                crate::media::set_query(app, v.as_str().unwrap_or_default());
                list_browser(app);
            }
            "pick" => crate::media::insert_path(app, std::path::PathBuf::from(v.as_str().unwrap_or_default())),
            _ => app.media.open = None,
        },
        "docTitle" => {
            let name = v.get("name").and_then(Value::as_str).map(str::to_string);
            let dir = v.get("dir").and_then(Value::as_str).map(std::path::PathBuf::from);
            match button {
                "open" => {
                    if app.doc_title.is_some() {
                        app.doc_title = None;
                    } else if app.wb.is_some() {
                        let f = crate::chrome::title_open(app);
                        DOC_FORMAT.with(|c| c.set(f));
                    }
                }
                "edit" => {
                    if let Some((n, d)) = app.doc_title.as_mut() {
                        if let Some(name) = name {
                            *n = name;
                        }
                        if dir.is_some() {
                            *d = dir;
                        }
                    }
                }
                "format" => DOC_FORMAT.with(|c| c.set(v.get("pick").and_then(Value::as_u64).unwrap_or(2).min(2) as u8)),
                "apply" => {
                    if let Some((n, Some(d))) = app.doc_title.take() {
                        crate::chrome::title_apply(app, &n, &d, DOC_FORMAT.with(|c| c.get()));
                    }
                }
                _ => app.doc_title = None,
            }
        }
        "customize" => match button {
            "items" => {
                let list: Vec<String> = v["items"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                crate::toolbar::set_items(app, &list);
            }
            "defaults" => {
                let list: Vec<String> = crate::toolbar::DEFAULT.iter().map(|s| s.to_string()).collect();
                crate::toolbar::set_items(app, &list);
            }
            "mode" => {
                app.settings.toolbar_mode = (v.as_u64().unwrap_or(1) as u8).clamp(1, 3);
                app.settings.save();
            }
            "open" => app.tb.customizing = true,
            _ => app.tb.customizing = false,
        },
        _ => {}
    }
}

fn settings_state(app: &App) -> Value {
    use kn_ui::theme;
    let (l, s) = (&app.look, &app.settings);
    let themes: Vec<Value> = theme::COLOR_THEMES
        .iter()
        .map(|&(id, name, main, accent)| {
            // Default is Values' own green
            let (main, accent) = if id == "default" { (theme::default_accent(), theme::default_accent()) } else { (main, accent) };
            json!({"id": id, "name": if id == "default" { "Numbers Green" } else { name }, "main": main, "accent": accent})
        })
        .collect();
    let theme_name = theme::COLOR_THEMES.iter().skip(1).find(|t| t.0 == l.color_theme).map(|t| t.1).unwrap_or("Numbers Green");
    json!({
        "id": "settings",
        "title": "Settings",
        "app": "Values",
        "android": cfg!(target_os = "android"),
        "windows": cfg!(target_os = "windows"),
        "themes": themes,
        "themeName": theme_name,
        "scales": std::iter::once(0).chain(kn_ui::scale::CHOICES).map(|p| json!([p, if p == 0 { "Default".to_string() } else { format!("{p}%") }])).collect::<Vec<_>>(),
        "layouts": kn_ui::form::CHOICES.iter().map(|(v, n)| json!([v, n])).collect::<Vec<_>>(),
        "zooms": crate::chrome::ZOOMS,
        "dark": l.dark,
        "color_theme": l.color_theme,
        "liquid_glass": l.liquid_glass,
        "translucent_panes": l.translucent_panes,
        "classic_look": l.classic_look,
        "system_title_bar": l.system_title_bar,
        "ui_scale": s.ui_scale,
        "ui_layout": s.ui_layout,
        "show_size_while_moving": s.show_size_while_moving,
        "formula_suggestions": s.formula_suggestions,
        "autocomplete_cells": s.autocomplete_cells,
        "default_zoom": s.default_zoom,
        "author": s.author,
        "ruler_units": s.ruler_units,
        "guides_center": s.guides_center,
        "guides_edges": s.guides_edges,
        "guides_spacing": s.guides_spacing,
        "guides_sizing": s.guides_sizing,
        "smart_quotes": s.smart_quotes,
        "smart_dashes": s.smart_dashes,
        "capitalize": s.capitalize,
        "text_replacement": s.text_replacement,
        "spell_check": s.spell_check,
        "grammar": s.grammar,
        "autocorrect": s.autocorrect,
    })
}

/// A setting changed in the Settings window ("settings.<field>"), saved as the desktop does;
/// a look change re-styles at once.
fn set_setting(app: &mut App, ctx: &egui::Context, field: &str, v: &Value) {
    let b = v.as_bool().unwrap_or(false);
    let st = v.as_str().unwrap_or_default().to_string();
    let n = v.as_f64().unwrap_or(0.0);
    let (look0, s0) = (app.look.clone(), app.settings.clone());
    let (l, s) = (&mut app.look, &mut app.settings);
    match field {
        "close" => app.show_settings = false,
        "dark" => l.dark = b,
        "color_theme" => l.color_theme = st,
        "liquid_glass" => l.liquid_glass = b,
        "translucent_panes" => l.translucent_panes = b,
        "classic_look" => l.classic_look = b,
        "system_title_bar" => l.system_title_bar = b,
        "ui_scale" => s.ui_scale = n as u32,
        "ui_layout" => s.ui_layout = n as u8,
        "show_size_while_moving" => s.show_size_while_moving = b,
        "formula_suggestions" => s.formula_suggestions = b,
        "autocomplete_cells" => s.autocomplete_cells = b,
        "default_zoom" => s.default_zoom = n as f32,
        "author" => s.author = st,
        "ruler_units" => s.ruler_units = (n as u8).min(2),
        "guides_center" => s.guides_center = b,
        "guides_edges" => s.guides_edges = b,
        "guides_spacing" => s.guides_spacing = b,
        "guides_sizing" => s.guides_sizing = b,
        "smart_quotes" => s.smart_quotes = b,
        "smart_dashes" => s.smart_dashes = b,
        "capitalize" => s.capitalize = b,
        "text_replacement" => s.text_replacement = b,
        "spell_check" => s.spell_check = b,
        "grammar" => s.grammar = b,
        "autocorrect" => s.autocorrect = b,
        _ => {}
    }
    if app.look != look0 {
        app.look.apply(ctx);
        app.look.save();
        // (the tiles' text doesn't change, but the chrome's colours do)
        app.version += 1;
    }
    if app.settings != s0 {
        app.settings.save();
    }
}

// ------------------------------------------------------------------ the template chooser (templates.rs)

/// A recent spreadsheet's thumbnail: the preview.jpg a Numbers file holds, copied once (on a
/// thread) into the cache; None meanwhile and for files with none.
fn thumbnail(path: &std::path::Path) -> Option<std::path::PathBuf> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, meta.len(), meta.modified().ok()).hash(&mut h);
    let cache = std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))?;
    let file = cache.join("numdeck").join("previews").join(format!("{:016x}.jpg", h.finish()));
    if file.exists() {
        return Some(file);
    }
    static MAKING: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());
    let mut m = MAKING.lock().unwrap_or_else(|e| e.into_inner());
    if !m.iter().any(|p| p == path) {
        m.push(path.to_path_buf());
        let p = path.to_path_buf();
        std::thread::spawn(move || {
            let bytes = (|| {
                let k = nm_core::kn_format::KeyFile::open(&p).ok()?;
                k.entry_bytes(k.entry("preview.jpg")?).map(|b| b.into_owned())
            })();
            if let Some(b) = bytes {
                let _ = std::fs::create_dir_all(file.parent().unwrap_or(std::path::Path::new(".")));
                let _ = std::fs::write(&file, &b);
            }
        });
    }
    None
}

fn chooser_state(app: &App) -> Option<Value> {
    if !app.templates.chooser {
        return None;
    }
    let mut templates: Vec<String> = crate::templates::STARTERS.iter().map(|(n, _)| n.to_string()).collect();
    templates.extend(crate::templates::mine().into_iter().map(|(n, _)| n));
    let recent: Vec<Value> = app
        .recent()
        .iter()
        .map(|p| json!({ "path": p.to_string_lossy(), "name": p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), "thumb": thumbnail(p).map(|f| f.to_string_lossy().into_owned()) }))
        .collect();
    let [r, g, b, a] = kn_ui::theme::CANVAS_BG().to_srgba_unmultiplied();
    Some(json!({
        "id": "chooser",
        "title": "Choose a Template",
        "app": "Values",
        "section": "Templates",
        "all": "All Templates",
        "hint": "or open a .numbers or .xlsx file",
        "templates": templates,
        "styles": crate::templates::STARTERS.iter().map(|(_, s)| s).collect::<Vec<_>>(),
        "default": "Blank",
        "tile": [176.0, 132.0],
        "cancel": true,
        "recent": recent,
        "backdrop": (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
    }))
}

/// "create" (a template's index), "open" (a recent file's path), "browse", "cancel".
fn chooser_set(app: &mut App, button: &str, v: &Value) {
    app.templates.chooser = false;
    match button {
        "create" => {
            let i = v.as_u64().unwrap_or(0) as usize;
            let n = crate::templates::STARTERS.len();
            if i < n {
                let style = crate::templates::STARTERS[i].1;
                app.new_document_with(|doc| crate::templates::with_table_style(doc, style));
            } else if let Some((_, f)) = crate::templates::mine().get(i - n) {
                crate::templates::new_from(app, f);
            }
        }
        "open" => app.open(std::path::Path::new(v.as_str().unwrap_or_default())),
        _ => {}
    }
}

// ------------------------------------------------------------------ Print Setup (fileops.rs print_setup)

const PAPERS: [(&str, (f32, f32)); 5] = [("US Letter", (612.0, 792.0)), ("US Legal", (612.0, 1008.0)), ("A4", (595.0, 842.0)), ("A3", (842.0, 1191.0)), ("Tabloid", (792.0, 1224.0))];

fn print_state(app: &App) -> Option<Value> {
    let sheet = app.sheets().get(app.sheet).cloned()?;
    let p = nm_core::sheets::page_setup(app.doc()?, sheet.id);
    let pages = app.wb.as_ref().map(|w| crate::pdf::page_count(w, &sheet)).unwrap_or(1);
    let paper = PAPERS.iter().position(|x| (x.1 .0 - p.paper.0.min(p.paper.1)).abs() < 1.0 && (x.1 .1 - p.paper.0.max(p.paper.1)).abs() < 1.0);
    Some(json!({
        "id": "printSetup", "title": "Print Setup",
        "sheet": sheet.name, "pages": pages,
        "papers": PAPERS.iter().map(|x| x.0).collect::<Vec<_>>(), "paper": paper,
        "portrait": p.portrait, "autofit": p.autofit, "scale": p.scale, "downFirst": p.down_first,
        "margins": p.margins, "pageNumbers": p.page_numbers, "startPage": p.start_page,
        "repeatHeaders": p.repeat_headers, "backgrounds": p.backgrounds, "comments": p.comments,
        "android": cfg!(target_os = "android"),
    }))
}

/// A Print Setup field ("paper", "portrait", "autofit", "scale", "downFirst", "margin" [i, pt],
/// "pageNumbers", "startPage" (null: from 1), "repeatHeaders", "backgrounds", "comments"), or
/// "print" (the printer here; a front end on Android prints the exported PDF itself), "close".
fn print_set(app: &mut App, button: &str, v: &Value) {
    let Some(sheet) = app.sheets().get(app.sheet).cloned() else { return };
    let Some(before) = app.doc().map(|d| nm_core::sheets::page_setup(d, sheet.id)) else { return };
    let mut p = before.clone();
    let b = v.as_bool().unwrap_or(false);
    let n = v.as_f64().unwrap_or(0.0) as f32;
    match button {
        "paper" => {
            if let Some(x) = PAPERS.get(v.as_u64().unwrap_or(0) as usize) {
                p.paper = x.1;
            }
        }
        "portrait" => p.portrait = b,
        "autofit" => p.autofit = b,
        "scale" => p.scale = (n / 100.0).clamp(0.1, 4.0),
        "downFirst" => p.down_first = b,
        "margin" => {
            let i = v.get(0).and_then(Value::as_u64).unwrap_or(0) as usize;
            if i < 4 {
                p.margins[i] = v.get(1).and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, 144.0) as f32;
            }
        }
        "pageNumbers" => p.page_numbers = b,
        "startPage" => p.start_page = v.as_i64().map(|x| x.clamp(1, 9999) as i32),
        "repeatHeaders" => p.repeat_headers = b,
        "backgrounds" => p.backgrounds = b,
        "comments" => p.comments = b,
        "print" => {
            app.print_setup_open = false;
            crate::fileops::print_now(app);
        }
        _ => app.print_setup_open = false,
    }
    if p != before {
        app.merge_next_edit = true;
        app.edit("Print Setup", |d, h| nm_core::sheets::set_page_setup(d, h, sheet.id, &p));
        app.merge_next_edit = false;
    }
}
