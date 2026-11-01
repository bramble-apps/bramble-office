//! Keydeck's windows as data, for front ends that draw them (ffi.rs kd_status "windows"):
//! Settings, Customize Toolbar, the Content Hub and About, in the shapes the suite's Flutter
//! windows take (Letters' dlgapi.rs); `set` does what their buttons do ("<window>.<button>").

use serde_json::{Value, json};

use crate::app::App;

/// The windows open now.
pub fn state(app: &App) -> Value {
    let mut out = Vec::new();
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
        let (basic, mine) = crate::editing::hub_shapes();
        out.push(json!({"id": "hub", "title": "Content Hub", "tab": app.hub.tab_index(), "basic": basic.iter().map(shape).collect::<Vec<_>>(), "mine": mine.iter().map(shape).collect::<Vec<_>>()}));
    }
    if app.show_customize_toolbar {
        let in_bar = crate::toolbar::items(app);
        let space = |id: &str| id == crate::toolbar::SPACE || id == crate::toolbar::FLEX;
        let items: Vec<Value> = crate::toolbar::ITEMS
            .iter()
            .map(|d| json!({"id": tb_id(d.id), "label": d.label, "tip": d.tip, "icon": format!("{:?}", d.icon), "used": !space(d.id) && in_bar.iter().any(|x| x == d.id)}))
            .collect();
        let defaults: Vec<String> = crate::toolbar::DEFAULT.iter().copied().filter(|id| !space(id)).map(tb_id).collect();
        out.push(json!({"id": "customize", "title": "", "items": items, "defaults": defaults, "mode": app.settings.toolbar_mode}));
    }
    if app.show_settings {
        out.push(settings_state(app));
    }
    if app.show_about {
        out.push(json!({"id": "about", "title": "About Keydeck", "version": env!("CARGO_PKG_VERSION")}));
    }
    Value::Array(out)
}

/// A toolbar item's id as the front end names it (ffi.rs tb_name).
fn tb_id(id: &str) -> String {
    use crate::toolbar::{FLEX, SPACE, VIEW, ZOOM};
    match id {
        VIEW => "View".into(),
        ZOOM => "Zoom".into(),
        SPACE => "Space".into(),
        FLEX => "FlexibleSpace".into(),
        _ => id.to_string(),
    }
}

/// And back.
fn from_tb_id(id: &str) -> String {
    use crate::toolbar::{FLEX, SPACE, VIEW, ZOOM};
    match id {
        "View" => VIEW.into(),
        "Zoom" => ZOOM.into(),
        "Space" => SPACE.into(),
        "FlexibleSpace" => FLEX.into(),
        "Format" | "Animate" | "Document" => crate::toolbar::INSPECTOR.into(),
        _ => id.to_string(),
    }
}

fn settings_state(app: &App) -> Value {
    use kn_ui::theme;
    let s = &app.settings;
    let themes: Vec<Value> = theme::COLOR_THEMES
        .iter()
        .map(|&(id, name, main, accent)| {
            let (main, accent) = if id == "default" { (theme::default_accent(), theme::default_accent()) } else { (main, accent) };
            json!({"id": id, "name": if id == "default" { "Keynote Blue" } else { name }, "main": main, "accent": accent})
        })
        .collect();
    let theme_name = theme::COLOR_THEMES.iter().skip(1).find(|t| t.0 == s.color_theme).map(|t| t.1).unwrap_or("Keynote Blue");
    json!({
        "id": "settings",
        "title": "Settings",
        "app": "keydeck",
        "android": cfg!(target_os = "android"),
        "windows": cfg!(target_os = "windows"),
        "themes": themes,
        "themeName": theme_name,
        "backgrounds": kn_ui::glass::ToolbarBackground::ALL.iter().map(|(b, n)| json!([b.key(), n])).collect::<Vec<_>>(),
        "scales": std::iter::once(0).chain(kn_ui::scale::CHOICES).map(|p| json!([p, if p == 0 { "Default".to_string() } else { format!("{p}%") }])).collect::<Vec<_>>(),
        "layouts": kn_ui::form::CHOICES.iter().map(|(v, n)| json!([v, n])).collect::<Vec<_>>(),
        "dark": s.dark,
        "color_theme": s.color_theme,
        "liquid_glass": s.liquid_glass,
        "translucent_panes": s.translucent_panes,
        "classic_look": s.classic_look,
        "system_title_bar": s.system_title_bar,
        "toolbar_background": s.toolbar_background,
        "ui_scale": s.ui_scale,
        "ui_layout": s.ui_layout,
        "show_size_while_moving": s.show_size_while_moving,
        "ruler_units": s.ruler_units,
        "guides_center": s.guides_center,
        "guides_edges": s.guides_edges,
        "guides_spacing": s.guides_spacing,
        "guides_sizing": s.guides_sizing,
        "smart_quotes": s.smart_quotes,
        "smart_dashes": s.smart_dashes,
        "text_replacement": s.text_replacement,
        "spell_check": s.spell_check,
        "grammar": s.grammar,
        "autocorrect": s.auto_correct,
        "exit_after_last_slide": s.exit_after_last_slide,
    })
}

/// A setting changed in the Settings window ("settings.<field>"), saved as the desktop does;
/// a look change re-styles at once.
fn set_setting(app: &mut App, ctx: &egui::Context, field: &str, v: &Value) {
    let b = v.as_bool().unwrap_or(false);
    let st = v.as_str().unwrap_or_default().to_string();
    let n = v.as_f64().unwrap_or(0.0);
    let s = &mut app.settings;
    match field {
        "close" => {
            app.show_settings = false;
            return;
        }
        "dark" => s.dark = b,
        "color_theme" => s.color_theme = st,
        "liquid_glass" => s.liquid_glass = b,
        "translucent_panes" => s.translucent_panes = b,
        "classic_look" => s.classic_look = b,
        "system_title_bar" => s.system_title_bar = b,
        "toolbar_background" => s.toolbar_background = st,
        "ui_scale" => s.ui_scale = n as u32,
        "ui_layout" => s.ui_layout = n as u8,
        "show_size_while_moving" => s.show_size_while_moving = b,
        "ruler_units" => s.ruler_units = (n as u8).min(3),
        "guides_center" => s.guides_center = b,
        "guides_edges" => s.guides_edges = b,
        "guides_spacing" => s.guides_spacing = b,
        "guides_sizing" => s.guides_sizing = b,
        "smart_quotes" => s.smart_quotes = b,
        "smart_dashes" => s.smart_dashes = b,
        "text_replacement" => s.text_replacement = b,
        "spell_check" => s.spell_check = b,
        "grammar" => s.grammar = b,
        "autocorrect" => s.auto_correct = b,
        "exit_after_last_slide" => s.exit_after_last_slide = b,
        _ => return,
    }
    app.settings.save();
    crate::settings::apply(ctx, &app.settings);
}

/// A window's button: "<window>.<button>".
pub fn set(app: &mut App, ctx: &egui::Context, key: &str, v: &Value) {
    let (win, button) = key.split_once('.').unwrap_or((key, ""));
    match win {
        "about" => app.show_about = false,
        "settings" => set_setting(app, ctx, button, v),
        "customize" => match button {
            "items" => {
                let mut list: Vec<String> = Vec::new();
                for id in v["items"].as_array().into_iter().flatten().filter_map(Value::as_str).map(from_tb_id) {
                    // (the inspector switch's three buttons are one item)
                    if id == crate::toolbar::INSPECTOR && list.last().is_some_and(|l| *l == id) {
                        continue;
                    }
                    list.push(id);
                }
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
            "open" => app.show_customize_toolbar = true,
            _ => app.show_customize_toolbar = false,
        },
        "hub" => {
            use kn_ui::contenthub::Picked;
            let key = v.as_str().unwrap_or_default().to_string();
            match button {
                "app" => crate::editing::insert_from_hub(app, Picked::App(key)),
                "remove" => crate::editing::insert_from_hub(app, Picked::Remove(key)),
                "lib" => {
                    if let Some(s) = kn_model::shapelib::shapes().get(v.as_u64().unwrap_or(u64::MAX) as usize) {
                        crate::editing::insert_from_hub(app, Picked::Library(s));
                    }
                }
                "image" => {
                    let p = v["path"].as_str().unwrap_or_default();
                    if let Ok(bytes) = std::fs::read(p) {
                        crate::editing::insert_object(app, kn_model::insert::NewObject::Image(bytes, v["name"].as_str().unwrap_or("picture.jpg").to_string()));
                    }
                }
                "tab" => {}
                _ => app.hub.open = false,
            }
        }
        _ => {}
    }
}
