//! The windows as data, for front ends that draw them themselves (the Flutter app, ffi.rs):
//! `state` lists the open ones with what they show, as the desktop's egui windows (dialogs.rs,
//! app.rs dialogs) would; `set` is a button in one of them ("<window>.<button>").

use serde_json::{Value, json};

use crate::app::App;
use kn_ui::media;

/// The open windows, the one on top last.
pub fn state(app: &App) -> Value {
    let mut out: Vec<Value> = crate::chooserapi::state(app).into_iter().collect();
    if let Some((kind, text)) = &app.link_edit {
        let bookmarks: Vec<Value> = app.wb.as_ref().map(|w| w.bookmarks()).unwrap_or_default().into_iter().map(|(_, t, a, _)| json!([t.trim(), a.to_string()])).collect();
        out.push(json!({"id": "link", "title": format!("Link to {kind}"), "kind": kind, "text": text, "bookmarks": bookmarks}));
    }
    if let Some((hl, text)) = &app.comment_edit {
        out.push(json!({"id": "comment", "title": if hl.is_some() { "Edit Comment" } else { "Comment" }, "author": crate::text::author(app), "text": text}));
    }
    if let Some(name) = &app.new_style {
        out.push(json!({"id": "newStyle", "title": "Create Paragraph Style", "text": name}));
    }
    if let Some(src) = &app.equation {
        out.push(json!({"id": "equation", "title": "Equation", "text": src, "preview": crate::dialogs::latex_to_text(src)}));
    }
    if let Some((path, hint, _, wrong)) = &app.password {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        out.push(json!({"id": "password", "title": "Password", "file": name, "hint": hint, "wrong": wrong}));
    }
    if app.set_password.is_some() {
        out.push(json!({"id": "setPassword", "title": "Set Password", "has": app.wb.as_ref().is_some_and(|w| w.doc.password.is_some())}));
    }
    if app.go_to.is_some() {
        out.push(json!({"id": "goTo", "title": "Go to Page", "pages": app.page_count()}));
    }
    if let Some(sp) = &app.spelling {
        let no_dicts = kn_ui::spell::dicts().is_some_and(|d| d.is_empty());
        out.push(json!({"id": "spelling", "title": "Spelling and Grammar", "done": sp.done, "noDicts": no_dicts, "word": sp.word, "guesses": sp.guesses, "pick": sp.pick}));
    }
    if app.show_versions {
        let all = app.doc().and_then(|d| d.path.clone()).map(|p| crate::fileops::list(&p)).unwrap_or_default();
        let rows: Vec<Value> = all.iter().map(|(p, secs)| json!([crate::fileops::when(*secs), p.to_string_lossy()])).collect();
        out.push(json!({"id": "versions", "title": "All Versions", "versions": rows}));
    }
    if app.show_fonts {
        let cur = crate::fonts::current(app);
        let st = cur.clone().unwrap_or_default();
        let family = kn_render::fonts::parse_ps_name(&st.font_name).0;
        let c = st.color;
        out.push(json!({
            "id": "fonts", "title": "Fonts", "can": cur.is_some(), "query": app.font_query,
            "names": crate::editing::font_names(app), "family": family,
            "bold": st.bold, "italic": st.italic, "underline": st.underline, "strike": st.strike, "size": st.size,
            "color": [c.r, c.g, c.b, 1.0],
        }));
    }
    if let Some(r) = &app.replace_fonts {
        let rows: Vec<Value> = r.rows.iter().map(|(n, inst, to)| json!([n, inst, to])).collect();
        out.push(json!({"id": "replaceFonts", "title": "Replace Fonts", "rows": rows, "families": app.canvas.tiles.font_families()}));
    }
    if let Some(rec) = crate::media::record_state() {
        out.push(json!({"id": "record", "title": "Record Audio", "seconds": rec}));
    }
    if let (Some(url), _) = crate::media::web_state() {
        out.push(json!({"id": "webVideo", "title": "Add Web Video", "text": url, "ok": media::web::video_link(url.trim())}));
    }
    if app.tabs.closing.last().is_some_and(|&i| i < app.tabs.len()) {
        out.push(json!({"id": "unsaved", "title": "Unsaved changes", "text": format!("Do you want to save the changes to \u{201c}{}\u{201d}?", app.title())}));
    }
    if app.tabs.show_all {
        let names: Vec<String> = (0..app.tabs.len()).map(|i| crate::tabs::name(app, i)).collect();
        out.push(json!({"id": "allTabs", "title": "All Tabs", "tabs": names, "active": app.tabs.active}));
    }
    if app.show_shortcuts {
        let sections: Vec<Value> = crate::menus::shortcut_sections().into_iter().map(|(t, rows)| json!([t, rows])).collect();
        out.push(json!({"id": "shortcuts", "title": "Keyboard Shortcuts", "sections": sections}));
    }
    if let Some(c) = crate::condwin::api_state(app) {
        out.push(c);
    }
    if let Some(ed) = &app.chart_edit {
        let mut v = ed.api_state();
        v["id"] = json!("chartData");
        v["title"] = json!("Chart Data");
        out.push(v);
    }
    if app.show_colors {
        let any = app.cursor.is_some_and(|c| !c.is_empty()) || app.canvas.obj.is_some();
        out.push(json!({"id": "colors", "title": "Colors", "color": app.colors_pick.to_srgba_unmultiplied(), "saved": app.settings.saved_colors, "any": any}));
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
    if let Some((name, dir)) = &app.doc_title {
        // the title popover (chrome.rs title): the name, where it is, its format
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
        let cur = match &app.word {
            Some(w) if w.hybrid => 2,
            Some(_) => 1,
            None => 0,
        };
        let note = match &app.word {
            Some(w) if w.hybrid => "A Word file anyone can open, with the whole Letters document kept inside it.",
            Some(_) => "A plain Word file: what Word can't hold isn't kept (those features are greyed).",
            None => "A Pages document.",
        };
        out.push(json!({
            "id": "docTitle", "title": "", "name": name, "dir": dir.as_ref().map(|d| d.to_string_lossy().into_owned()),
            "dirName": dir.as_deref().map(show), "places": places.iter().map(|p| json!([show(p), p.to_string_lossy()])).collect::<Vec<_>>(),
            "format": cur, "note": note,
        }));
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
        out.push(json!({"id": "about", "title": "About Letters", "version": env!("CARGO_PKG_VERSION")}));
    }
    Value::Array(out)
}

fn text(v: &Value) -> String {
    v.get("text").and_then(Value::as_str).unwrap_or_default().to_string()
}

/// A window's button: "<window>.done" with what was typed ({"text": …}), "<window>.cancel",
/// or a window's own ("equation.edit" keeps the source for the preview).
pub fn set(app: &mut App, ctx: &egui::Context, key: &str, v: &Value) {
    let (win, button) = key.split_once('.').unwrap_or((key, ""));
    let cancel = button == "cancel";
    match win {
        "link" => {
            if let Some((kind, _)) = app.link_edit.take() {
                if !cancel {
                    crate::dialogs::link_done(app, &kind, &text(v));
                }
            }
        }
        "comment" => {
            if let Some((hl, _)) = app.comment_edit.take() {
                if !cancel {
                    crate::dialogs::comment_done(app, hl, &text(v));
                }
            }
        }
        "newStyle" => {
            if app.new_style.take().is_some() && !cancel {
                crate::dialogs::new_style_done(app, &text(v));
            }
        }
        "equation" => match button {
            "edit" => app.equation = Some(text(v)),
            _ => {
                app.equation = None;
                if cancel {
                    app.equation_target = None;
                } else {
                    crate::dialogs::equation_done(app, &text(v));
                }
            }
        },
        "password" => match button {
            "cancel" => app.password = None,
            _ => {
                if let Some(p) = &mut app.password {
                    p.2 = text(v);
                }
                app.password_open();
            }
        },
        "setPassword" => match button {
            "remove" => app.set_password_done(Some(None)),
            "done" => {
                let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
                app.set_password_done(Some(Some((s("password"), s("hint")))));
            }
            _ => app.set_password_done(None),
        },
        "goTo" => {
            app.go_to = None;
            if !cancel {
                if let Ok(p) = text(v).trim().parse::<usize>() {
                    let n = app.page_count();
                    crate::menus::go_to_page(app, p.clamp(1, n) - 1);
                }
            }
        }
        "about" => app.show_about = false,
        "chooser" => crate::chooserapi::set(app, button, v),
        "settings" => set_setting(app, ctx, button, v),
        // the toolbar's items as dragged into place ({"items": [ids]}), the default set, the mode
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
        "docTitle" => {
            let name = v.get("name").and_then(Value::as_str).map(str::to_string);
            let dir = v.get("dir").and_then(Value::as_str).map(std::path::PathBuf::from);
            match button {
                "open" => {
                    if app.doc_title.is_some() {
                        app.doc_title = None;
                    } else {
                        crate::chrome::title_open(app);
                    }
                }
                // what's typed so far (the name, the folder)
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
                "format" => {
                    if let Some((n, d)) = app.doc_title.clone() {
                        crate::chrome::title_format(app, v.get("pick").and_then(Value::as_u64).unwrap_or(0) as usize, n, d);
                    }
                }
                "apply" => {
                    if let Some((n, d)) = app.doc_title.take() {
                        crate::chrome::title_apply(app, &n, d);
                    }
                }
                _ => app.doc_title = None,
            }
        }
        // Content Hub picks (the front end searches and downloads; dlg.hub.image inserts a file)
        "hub" => {
            use kn_ui::contenthub::Picked;
            let key = v.as_str().unwrap_or_default().to_string();
            match button {
                "app" => crate::chrome::insert_from_hub(app, ctx, Picked::App(key)),
                "remove" => crate::chrome::insert_from_hub(app, ctx, Picked::Remove(key)),
                "lib" => {
                    if let Some(s) = pg_core::kn_model::shapelib::shapes().get(v.as_u64().unwrap_or(u64::MAX) as usize) {
                        crate::chrome::insert_from_hub(app, ctx, Picked::Library(s));
                    }
                }
                "image" => {
                    let p = v["path"].as_str().unwrap_or_default();
                    if let Ok(bytes) = std::fs::read(p) {
                        crate::insert::insert_image_bytes(app, bytes, v["name"].as_str().unwrap_or("picture.jpg"));
                    }
                }
                _ => app.hub.open = false,
            }
        }
        // View > Show Colors (the panel itself is the front end's; its saved colours are the app's)
        "colors" => {
            let c = v.as_array().map(|a| {
                let f = |i: usize| a.get(i).and_then(Value::as_u64).unwrap_or(255) as u8;
                egui::Color32::from_rgba_unmultiplied(f(0), f(1), f(2), f(3))
            });
            match button {
                "color" => app.colors_pick = c.unwrap_or(app.colors_pick),
                "apply" => crate::dialogs::apply_colors(app, c.unwrap_or(app.colors_pick)),
                "saved" => {
                    app.settings.saved_colors = v.as_str().unwrap_or_default().to_string();
                    app.settings.save();
                    app.color_state.saved.clear();
                }
                _ => app.show_colors = false,
            }
        }
        "shortcuts" => app.show_shortcuts = false,
        "condHighlight" => crate::condwin::api_set(app, button, v),
        "chartData" => match button {
            "close" => app.chart_edit = None,
            _ => {
                if let Some(mut ed) = app.chart_edit.take() {
                    let act = ed.api_action(button, v);
                    crate::chartedit::apply(app, &mut ed, act);
                    app.chart_edit = Some(ed);
                }
            }
        },
        "unsaved" => crate::tabs::closing_choice(app, match button {
            "dontSave" => 0,
            "save" => 2,
            _ => 1,
        }),
        "allTabs" => {
            if button == "pick" {
                crate::tabs::select(app, v.as_u64().unwrap_or(0) as usize);
            }
            app.tabs.show_all = false;
        }
        "record" => match button {
            "start" => crate::media::record_start(app),
            "stop" => {
                crate::media::record_stop(app, true);
                crate::media::record_close();
            }
            _ => {
                crate::media::record_stop(app, false);
                crate::media::record_close();
            }
        },
        "webVideo" => match button {
            "edit" => crate::media::web_edit(&text(v)),
            "done" => crate::media::web_go(&text(v), || {}),
            _ => crate::media::web_close(),
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
            use pg_core::kn_model::format::TextProp;
            let b = v.as_bool().unwrap_or(false);
            let n = v.as_f64().unwrap_or(0.0) as f32;
            match button {
                "query" => app.font_query = v.as_str().unwrap_or_default().to_string(),
                "family" => crate::editing::set_text(app, "Font", TextProp::Font(v.as_str().unwrap_or_default().replace(' ', ""))),
                "bold" => crate::editing::set_text(app, "Bold", TextProp::Bold(b)),
                "italic" => crate::editing::set_text(app, "Italic", TextProp::Italic(b)),
                "underline" => crate::editing::set_text(app, "Underline", TextProp::Underline(b)),
                "strike" => crate::editing::set_text(app, "Strikethrough", TextProp::Strike(b)),
                "size" => crate::editing::set_text(app, "Font Size", TextProp::Size(n.clamp(1.0, 1638.0))),
                "color" => {
                    let a = v.as_array().cloned().unwrap_or_default();
                    let f = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
                    crate::editing::set_text(app, "Text Color", TextProp::Color(pg_core::kn_model::Color { r: f(0), g: f(1), b: f(2), a: 1.0 }));
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
        _ => {}
    }
}

/// Letters > Settings…: the look and the settings, with the choices the drop-downs offer.
fn settings_state(app: &App) -> Value {
    use kn_ui::theme;
    let (l, s) = (&app.look, &app.settings);
    let themes: Vec<Value> = theme::COLOR_THEMES
        .iter()
        .map(|&(id, name, main, accent)| {
            // Default is Letters' own orange
            let (main, accent) = if id == "default" { (theme::default_accent(), theme::default_accent()) } else { (main, accent) };
            json!({"id": id, "name": if id == "default" { "Pages Orange" } else { name }, "main": main, "accent": accent})
        })
        .collect();
    let theme_name = theme::COLOR_THEMES.iter().skip(1).find(|t| t.0 == l.color_theme).map(|t| t.1).unwrap_or("Pages Orange");
    json!({
        "id": "settings",
        "title": "Settings",
        "android": cfg!(target_os = "android"),
        "windows": cfg!(target_os = "windows"),
        "themes": themes,
        "themeName": theme_name,
        "backgrounds": kn_ui::glass::ToolbarBackground::ALL.iter().map(|(b, n)| json!([b.key(), n])).collect::<Vec<_>>(),
        "scales": std::iter::once(0).chain(kn_ui::scale::CHOICES).map(|p| json!([p, if p == 0 { "Default".to_string() } else { format!("{p}%") }])).collect::<Vec<_>>(),
        "layouts": kn_ui::form::CHOICES.iter().map(|(v, n)| json!([v, n])).collect::<Vec<_>>(),
        "zooms": crate::chrome::ZOOMS,
        "dark": l.dark,
        "color_theme": l.color_theme,
        "liquid_glass": l.liquid_glass,
        "translucent_panes": l.translucent_panes,
        "classic_look": l.classic_look,
        "system_title_bar": l.system_title_bar,
        "toolbar_background": l.toolbar_background.key(),
        "ui_scale": s.ui_scale,
        "ui_layout": s.ui_layout,
        "show_size_while_moving": s.show_size_while_moving,
        "formula_suggestions": s.formula_suggestions,
        "show_chooser": s.show_chooser,
        "default_font": s.default_font,
        // (the Default Font field's list: the font menu's)
        "fonts": app.canvas.tiles.font_families(),
        "default_font_size": s.default_font_size,
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
        "detect_lists": s.detect_lists,
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
        "toolbar_background" => {
            if let Some(t) = kn_ui::glass::ToolbarBackground::from_key(&st) {
                l.toolbar_background = t;
            }
        }
        "ui_scale" => s.ui_scale = n as u32,
        "ui_layout" => s.ui_layout = n as u8,
        "show_size_while_moving" => s.show_size_while_moving = b,
        "formula_suggestions" => s.formula_suggestions = b,
        "show_chooser" => s.show_chooser = b,
        "default_font" => s.default_font = st,
        "default_font_size" => s.default_font_size = (n as f32).clamp(6.0, 96.0),
        "default_zoom" => s.default_zoom = n as f32,
        "author" => s.author = st,
        "ruler_units" => s.ruler_units = (n as u8).min(3),
        "guides_center" => s.guides_center = b,
        "guides_edges" => s.guides_edges = b,
        "guides_spacing" => s.guides_spacing = b,
        "guides_sizing" => s.guides_sizing = b,
        "smart_quotes" => s.smart_quotes = b,
        "smart_dashes" => s.smart_dashes = b,
        "capitalize" => s.capitalize = b,
        "text_replacement" => s.text_replacement = b,
        "detect_lists" => s.detect_lists = b,
        "spell_check" => s.spell_check = b,
        "grammar" => s.grammar = b,
        "autocorrect" => s.autocorrect = b,
        _ => {}
    }
    if app.look != look0 {
        app.look.apply(ctx);
        app.look.save();
    }
    if app.settings != s0 {
        app.settings.save();
    }
}
