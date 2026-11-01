//! The inspector as data, for front ends that draw it themselves (the Flutter app, ffi.rs):
//! `state` says which pane shows and its controls' values, as the desktop inspector
//! (inspector.rs) would show them now; `set` makes the same edits changing one of them makes
//! there. Keys are "<pane>.<control>".

use kn_model::buildedit::BuildKind;
use kn_model::format::{FillSpec, StrokeSpec, TextProp};
use kn_model::{Align, Color, DrawableKind, Fill, VAlign};
use serde_json::{Value, json};

use crate::app::{App, InspectorKind};

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

fn fill_json(f: &Fill) -> Value {
    match f {
        // (the kinds as the suite's fill editor numbers them: none, colour, gradient, image)
        Fill::None => json!({ "kind": 0 }),
        Fill::Color(c) => json!({ "kind": 1, "color": col(*c) }),
        Fill::Gradient { stops, .. } => json!({ "kind": 2, "from": stops.first().map(|s| col(s.1)), "to": stops.last().map(|s| col(s.1)), "angle": 270.0 }),
        _ => json!({ "kind": 3 }),
    }
}

/// The fill editor's fill as the model's (kinds: 0 none, 1 colour, 2 gradient).
fn fill_spec(v: &Value) -> Option<FillSpec> {
    Some(match v.get("kind").and_then(Value::as_u64)? {
        0 => FillSpec::None,
        2 => FillSpec::Gradient { from: to_col(&v["from"]).unwrap_or(Color::WHITE), to: to_col(&v["to"]).unwrap_or(Color::BLACK), angle: num(&v["angle"]) },
        1 => FillSpec::Color(to_col(&v["color"]).unwrap_or(Color::WHITE)),
        _ => return None,
    })
}

/// Which pane shows, and its values.
pub fn state(app: &mut App) -> Value {
    let Some(scene) = crate::canvas::api::scene(app) else { return json!({ "pane": "none" }) };
    match app.inspector {
        InspectorKind::Document => document_state(app),
        InspectorKind::Animate => animate_state(app, &scene),
        InspectorKind::Format => {
            let sel: Vec<kn_model::Drawable> = scene.drawables.iter().filter(|d| app.sel.contains(&d.id)).cloned().collect();
            if sel.is_empty() {
                return slide_state(app, &scene);
            }
            object_state(app, &sel)
        }
    }
}

// ------------------------------------------------------------------ Format: objects

fn object_tabs(d: &kn_model::Drawable, many: bool) -> Vec<&'static str> {
    match &d.kind {
        DrawableKind::Shape(s) if s.text.is_some() && !many => vec!["Style", "Text", "Arrange"],
        DrawableKind::Image(_) if !many => vec!["Style", "Image", "Arrange"],
        _ => vec!["Style", "Arrange"],
    }
}

fn object_state(app: &mut App, sel: &[kn_model::Drawable]) -> Value {
    let d = &sel[0];
    let tabs = object_tabs(d, sel.len() > 1);
    let tab = app.insp.format_tab.min(tabs.len() - 1);
    let body = match tabs[tab] {
        "Text" => text_state(app, d),
        "Arrange" => arrange_state(app, sel),
        "Image" => image_state(d),
        _ => style_state(d),
    };
    json!({ "pane": "object", "tabs": tabs, "tab": tab, "kind": kind_name(d), "count": sel.len(), "body": body })
}

fn kind_name(d: &kn_model::Drawable) -> &'static str {
    match &d.kind {
        DrawableKind::Shape(s) if s.is_line => "line",
        DrawableKind::Shape(s) if s.is_text_box => "text",
        DrawableKind::Shape(_) => "shape",
        DrawableKind::Image(_) => "image",
        DrawableKind::Movie(_) => "movie",
        DrawableKind::Group(_) => "group",
        DrawableKind::Table(_) => "table",
        DrawableKind::Chart(_) => "chart",
        DrawableKind::Unsupported(_) => "other",
    }
}

fn style_state(d: &kn_model::Drawable) -> Value {
    let (fill, stroke) = match &d.kind {
        DrawableKind::Shape(s) => (Some(fill_json(&s.fill)), s.stroke.as_ref()),
        DrawableKind::Image(i) => (None, i.stroke.as_ref()),
        _ => (None, None),
    };
    let stroke = stroke.map(|s| json!({ "color": col(s.color), "width": s.width, "dash": if s.dash.is_empty() { "Solid" } else if s.dash.first().is_some_and(|d| *d < 1.5) { "Dotted" } else { "Dashed" } }));
    json!({
        "fill": fill,
        "stroke": stroke,
        "opacity": (d.opacity * 100.0).round(),
        "shadow": d.shadow.is_some(),
        "reflection": d.reflection.map(|r| (r * 100.0).round()),
    })
}

fn set_style(app: &mut App, k: &str, v: &Value) {
    let ids = app.sel.clone();
    match k {
        "fill" => {
            let Some(spec) = fill_spec(v) else { return };
            crate::editing::apply(app, "Fill", |doc, h, id| doc.set_fill(h, id, &spec));
        }
        "stroke" => {
            let spec = if v.is_null() {
                None
            } else {
                let dash = match v["dash"].as_str() {
                    Some("Dashed") => kn_model::format::Dash::Dashed,
                    Some("Dotted") => kn_model::format::Dash::Dotted,
                    _ => kn_model::format::Dash::Solid,
                };
                Some(StrokeSpec { color: to_col(&v["color"]).unwrap_or(Color::BLACK), width: num(&v["width"]).max(0.0), dash })
            };
            crate::editing::apply(app, "Border", |doc, h, id| doc.set_stroke(h, id, spec.as_ref()));
        }
        "opacity" => {
            let o = (num(v) / 100.0).clamp(0.0, 1.0);
            crate::editing::apply(app, "Opacity", |doc, h, id| doc.set_opacity(h, id, o));
        }
        "shadow" => {
            let on = v.as_bool().unwrap_or(false);
            crate::editing::apply(app, "Shadow", |doc, h, id| doc.set_shadow(h, id, on));
        }
        _ => {}
    }
    let _ = ids;
}

// ------------------------------------------------------------------ Format: Text

fn text_state(app: &mut App, d: &kn_model::Drawable) -> Value {
    let cs = crate::editing::current_run(app).or_else(|| crate::editing::first_run(d)).unwrap_or_default();
    let tb = match &d.kind {
        DrawableKind::Shape(s) => s.text.as_ref(),
        _ => None,
    };
    let caret = app.canvas.editing.as_ref().map(|e| e.cursor).unwrap_or(0);
    let para = tb.and_then(|t| t.paragraphs.iter().find(|p| caret >= p.start && caret <= p.end).or(t.paragraphs.first()));
    let align = para.map(|p| match p.align {
        Align::Left | Align::Natural => 0,
        Align::Center => 1,
        Align::Right => 2,
        Align::Justify => 3,
    });
    let valign = tb.map(|t| match t.valign {
        VAlign::Top => 0,
        VAlign::Middle => 1,
        VAlign::Bottom => 2,
    });
    let spacing = para.map(|p| match p.line_spacing {
        kn_model::LineSpacing::Relative(r) => r,
        _ => 1.0,
    });
    let fonts = crate::editing::font_names(app);
    json!({
        "family": cs.font_name,
        "size": cs.size,
        "bold": cs.bold,
        "italic": cs.italic,
        "underline": cs.underline,
        "strike": cs.strike,
        "color": col(cs.color),
        "align": align,
        "valign": valign,
        "spacing": spacing,
        "fonts": fonts,
    })
}

fn set_text(app: &mut App, k: &str, v: &Value) {
    let on = v.as_bool().unwrap_or(false);
    let (label, prop) = match k {
        "family" => ("Font", TextProp::Font(v.as_str().unwrap_or_default().to_string())),
        "size" => ("Font Size", TextProp::Size(num(v).round().clamp(1.0, 1638.0))),
        "bold" => ("Bold", TextProp::Bold(on)),
        "italic" => ("Italic", TextProp::Italic(on)),
        "underline" => ("Underline", TextProp::Underline(on)),
        "strike" => ("Strikethrough", TextProp::Strike(on)),
        "color" => ("Text Color", TextProp::Color(to_col(v).unwrap_or(Color::BLACK))),
        "align" => ("Alignment", TextProp::Align([Align::Left, Align::Center, Align::Right, Align::Justify][(v.as_u64().unwrap_or(0) as usize).min(3)])),
        "valign" => ("Alignment", TextProp::VAlign([VAlign::Top, VAlign::Middle, VAlign::Bottom][(v.as_u64().unwrap_or(0) as usize).min(2)])),
        "spacing" => ("Line Spacing", TextProp::LineSpacing(num(v).clamp(0.5, 5.0))),
        _ => return,
    };
    crate::editing::set_text(app, label, prop);
}

// ------------------------------------------------------------------ Format: Arrange

fn arrange_state(app: &mut App, sel: &[kn_model::Drawable]) -> Value {
    let d = &sel[0];
    let g = app.renderer.effective_geometry(d);
    json!({
        "x": g.x, "y": g.y, "w": g.w, "h": g.h, "angle": g.angle,
        "locked": d.locked,
        "count": sel.len(),
        "hflip": g.hflip, "vflip": g.vflip,
    })
}

fn set_arrange(app: &mut App, ctx: &egui::Context, k: &str, v: &Value) {
    match k {
        // the desktop's menu commands (Bring to Front, Flip Horizontally, Lock, Group, Align…)
        "command" => crate::menus::run(app, ctx, v.as_str().unwrap_or_default()),
        "x" | "y" | "w" | "h" | "angle" => {
            let Some(scene) = crate::canvas::api::scene(app) else { return };
            let &[id] = &app.sel[..] else { return };
            let Some(d) = scene.drawables.iter().find(|d| d.id == id) else { return };
            let mut g = app.renderer.effective_geometry(d);
            let n = num(v);
            match k {
                "x" => g.x = n,
                "y" => g.y = n,
                "w" => {
                    g.w = n.max(1.0);
                    g.auto_width = false;
                }
                "h" => {
                    g.h = n.max(1.0);
                    g.auto_height = false;
                }
                _ => g.angle = n.rem_euclid(360.0),
            }
            let slide = scene.slide;
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, if k == "angle" { "Rotate" } else { "Resize" });
            doc.set_frame(&mut app.hist, id, g);
            doc.end(&mut app.hist);
            app.bump(slide);
        }
        _ => {}
    }
}

fn image_state(d: &kn_model::Drawable) -> Value {
    let DrawableKind::Image(i) = &d.kind else { return Value::Null };
    json!({ "data": i.data_id, "masked": i.mask.is_some(), "description": d.alt_text })
}

// ------------------------------------------------------------------ Format: the slide

fn slide_state(app: &mut App, scene: &kn_model::SlideScene) -> Value {
    let layout = app.doc.as_ref().and_then(|d| d.layout_name(scene.slide));
    let layouts: Vec<String> = app.doc.as_ref().map(|d| d.layouts().iter().map(|(_, n)| n.clone()).collect()).unwrap_or_default();
    json!({ "pane": "slide", "layout": layout, "layouts": layouts, "background": fill_json(&scene.background), "skipped": app.current_slide().is_some_and(|s| s.skipped) })
}

fn set_slide(app: &mut App, ctx: &egui::Context, k: &str, v: &Value) {
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    match k {
        "background" => {
            let Some(spec) = fill_spec(v) else { return };
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Background");
            doc.set_slide_background(&mut app.hist, slide, &spec);
            doc.end(&mut app.hist);
            app.bump(slide);
        }
        "layout" => {
            let i = v.as_u64().unwrap_or(0) as usize;
            let Some(layout) = app.doc.as_ref().and_then(|d| d.layouts().get(i).map(|l| l.0)) else { return };
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Slide Layout");
            let ok = doc.change_slide_layout(&mut app.hist, slide, layout);
            doc.end(&mut app.hist);
            if ok {
                app.bump_all();
            }
        }
        "command" => crate::menus::run(app, ctx, v.as_str().unwrap_or_default()),
        _ => {}
    }
}

// ------------------------------------------------------------------ Animate

/// Keynote's effect picker sections (inspector.rs build_inspector).
fn build_sections(kind: BuildKind) -> Vec<(&'static str, &'static [&'static str])> {
    match kind {
        BuildKind::Action => vec![("Basic", &["Move", "Opacity", "Rotate", "Scale"]), ("Emphasis", &["Blink", "Bounce", "Flip", "Jiggle", "Pop", "Pulse"])],
        _ => vec![
            ("Appear & Move", &["Appear", "Blur", "Compress", "Dissolve", "Drift", "Drift and Scale", "Drop", "Fade and Move", "Fade and Scale", "Fly In", "Iris", "Keyboard", "Move In", "Radial Wipe", "Shift", "Shift and Scale", "Wipe"]),
            ("Flip, Spin & Scale", &["Blinds", "Cube", "Flip", "Orbital", "Pivot", "Pop", "Scale", "Scale Big", "Spin", "Swoosh", "Twirl", "Twist and Scale"]),
            ("Special Effects", &["Anvil", "Blast", "Bouncy", "Comet", "Confetti", "Fireworks", "Flame", "Flash Bulbs", "Lens Flare", "Shimmer", "Skid", "Sparkle", "Squish", "Trace"]),
        ],
    }
}

/// A picker name's effect id (inspector.rs build_inspector id_for).
fn effect_id(kind: BuildKind, name: &str) -> Option<&'static str> {
    use kn_model::buildedit::EFFECTS;
    let n = match (kind, name) {
        (BuildKind::Out, "Appear") => "Disappear",
        (BuildKind::Out, "Move In") => "Move Out",
        (BuildKind::Out, "Fly In") => "Fly Out",
        (BuildKind::Action, "Scale") => "Scale (action)",
        (BuildKind::Action, "Pop") => "Pop (action)",
        (BuildKind::Action, "Flip") => "Flip (action)",
        (_, other) => other,
    };
    let mut found: Vec<&(&str, &str, &[BuildKind], bool)> = EFFECTS.iter().filter(|(en, _, ks, _)| ks.contains(&kind) && (*en == n || en.strip_suffix(" (by character)") == Some(n))).collect();
    found.sort_by_key(|e| !e.3);
    found.first().map(|e| e.1)
}

const KINDS: [BuildKind; 3] = [BuildKind::In, BuildKind::Action, BuildKind::Out];

fn animate_state(app: &mut App, scene: &kn_model::SlideScene) -> Value {
    if let Some(&target) = app.sel.first() {
        let t = app.insp.animate_tab.min(2);
        let kind = KINDS[t];
        let b = scene.builds.iter().find(|b| b.drawable == target && b.kind == kind.as_str());
        let sections: Vec<Value> = build_sections(kind).iter().map(|(title, names)| json!({ "title": title, "names": names.iter().filter(|n| effect_id(kind, n).is_some()).collect::<Vec<_>>() })).collect();
        let name = b.map(|b| crate::inspector::pretty_effect(&b.effect));
        let current = b.and_then(|b| build_sections(kind).iter().flat_map(|(_, ns)| ns.iter()).find(|n| effect_id(kind, n) == Some(b.effect.as_str())).copied());
        let order = scene.build_order.iter().position(|s| b.is_some_and(|b| b.id == s.build));
        return json!({ "pane": "build", "tabs": ["Build In", "Action", "Build Out"], "tab": t, "effect": current.or(name.as_deref()), "duration": b.map(|b| b.duration), "sections": sections, "order": order, "steps": scene.build_order.len() });
    }
    let tr = scene.transition.clone();
    let names = crate::inspector::transition_names();
    let current = tr.as_ref().map(|t| t.effect.clone()).unwrap_or_else(|| "none".into());
    let current_name = names.iter().find(|(_, id)| *id == current).map(|(n, _)| n.clone()).or_else(|| (current != "none").then(|| crate::inspector::pretty_effect(&current)));
    json!({
        "pane": "transition",
        "effect": current_name,
        "effects": names.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
        "duration": tr.as_ref().map(|t| t.duration).unwrap_or(1.0),
        "delay": tr.as_ref().map(|t| t.delay).unwrap_or(0.0),
        "automatic": tr.as_ref().is_some_and(|t| t.automatic),
    })
}

fn set_animate(app: &mut App, k: &str, v: &Value) {
    let Some(scene) = crate::canvas::api::scene(app) else { return };
    let slide = scene.slide;
    if k == "tab" {
        app.insp.animate_tab = (v.as_u64().unwrap_or(0) as usize).min(2);
        return;
    }
    if let Some(tk) = k.strip_prefix("tr.") {
        let tr = scene.transition.clone();
        let names = crate::inspector::transition_names();
        let mut effect = tr.as_ref().map(|t| t.effect.clone()).unwrap_or_else(|| "none".into());
        let mut duration = tr.as_ref().map(|t| t.duration).unwrap_or(1.0);
        let mut delay = tr.as_ref().map(|t| t.delay).unwrap_or(0.0);
        let mut automatic = tr.as_ref().is_some_and(|t| t.automatic);
        match tk {
            "effect" => effect = v.as_str().and_then(|n| names.iter().find(|(m, _)| m == n)).map(|(_, id)| id.clone()).unwrap_or_else(|| "none".into()),
            "duration" => duration = v.as_f64().unwrap_or(1.0).clamp(0.1, 60.0),
            "delay" => delay = v.as_f64().unwrap_or(0.0).clamp(0.0, 60.0),
            "automatic" => automatic = v.as_bool().unwrap_or(false),
            _ => return,
        }
        let Some(doc) = app.doc.as_mut() else { return };
        doc.begin(&mut app.hist, "Transition");
        doc.set_transition(&mut app.hist, slide, &effect, duration, delay, automatic);
        doc.end(&mut app.hist);
        app.bump(slide);
        return;
    }
    let Some(&target) = app.sel.first() else { return };
    let kind = KINDS[app.insp.animate_tab.min(2)];
    let b = scene.builds.iter().find(|b| b.drawable == target && b.kind == kind.as_str()).cloned();
    let Some(doc) = app.doc.as_mut() else { return };
    match k {
        "effect" => {
            let id = v.as_str().and_then(|n| effect_id(kind, n));
            doc.begin(&mut app.hist, "Build");
            match (id, &b) {
                (None, Some(b)) => doc.remove_build(&mut app.hist, slide, b.id),
                (Some(e), Some(b)) => doc.set_build_effect(&mut app.hist, slide, b.id, e, b.duration),
                (Some(e), None) => {
                    let _ = doc.add_build(&mut app.hist, slide, target, kind, e, 1.0);
                }
                (None, None) => {}
            }
            doc.end(&mut app.hist);
        }
        "duration" => {
            let Some(b) = b else { return };
            doc.begin(&mut app.hist, "Build");
            doc.set_build_effect(&mut app.hist, slide, b.id, &b.effect, v.as_f64().unwrap_or(1.0).clamp(0.1, 60.0));
            doc.end(&mut app.hist);
        }
        _ => return,
    }
    app.bump(slide);
}

// ------------------------------------------------------------------ Document

fn document_state(app: &mut App) -> Value {
    let Some(doc) = app.doc.as_ref() else { return json!({ "pane": "none" }) };
    let s = doc.show_settings();
    let size = doc.slide_size();
    json!({
        "pane": "document",
        "loop": s.looping,
        "autoplay": s.auto_play_on_open,
        "mode": match s.mode { kn_model::slides::ShowMode::Normal => 0, kn_model::slides::ShowMode::SelfPlaying => 1, kn_model::slides::ShowMode::LinksOnly => 2 },
        "transitionDelay": s.transition_delay,
        "buildDelay": s.build_delay,
        "size": [size.w, size.h],
        "wide": (size.w / size.h - 16.0 / 9.0).abs() < 0.01,
    })
}

fn set_document(app: &mut App, k: &str, v: &Value) {
    use kn_model::slides::{ShowMode, ShowSetting};
    let setting = match k {
        "loop" => ShowSetting::Loop(v.as_bool().unwrap_or(false)),
        "autoplay" => ShowSetting::AutoPlayOnOpen(v.as_bool().unwrap_or(false)),
        "mode" => ShowSetting::Mode([ShowMode::Normal, ShowMode::SelfPlaying, ShowMode::LinksOnly][(v.as_u64().unwrap_or(0) as usize).min(2)]),
        "transitionDelay" => ShowSetting::TransitionDelay(v.as_f64().unwrap_or(0.0).max(0.0)),
        "buildDelay" => ShowSetting::BuildDelay(v.as_f64().unwrap_or(0.0).max(0.0)),
        "size" => {
            let (w, h) = if v.as_str() == Some("4:3") { (1024.0, 768.0) } else { (1920.0, 1080.0) };
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Slide Size");
            doc.set_slide_size(&mut app.hist, w, h);
            doc.end(&mut app.hist);
            app.refresh_after_structural_change();
            return;
        }
        _ => return,
    };
    let Some(doc) = app.doc.as_mut() else { return };
    doc.begin(&mut app.hist, "Slideshow Settings");
    doc.set_show_setting(&mut app.hist, setting);
    doc.end(&mut app.hist);
}

/// Changes a control: `key` as `state` names it, `v` its new value (or what was picked).
pub fn set(app: &mut App, ctx: &egui::Context, key: &str, v: &Value) {
    let (pane, k) = key.split_once('.').unwrap_or((key, ""));
    match pane {
        "tab" => app.insp.format_tab = v.as_u64().unwrap_or(0) as usize,
        "style" => set_style(app, k, v),
        "text" => set_text(app, k, v),
        "arrange" => set_arrange(app, ctx, k, v),
        "slide" => set_slide(app, ctx, k, v),
        "animate" => set_animate(app, k, v),
        "doc" => set_document(app, k, v),
        _ => {}
    }
}
