//! The inspector as data, for front ends that draw it themselves (the Flutter app, ffi.rs):
//! `state` says which pane shows and its controls' values, as the desktop inspector
//! (inspector.rs) would show them now; `set` makes the same edits changing one of them makes
//! there. Keys are "<pane>.<control>".

use kn_model::buildedit::BuildKind;
use kn_model::format::TextProp;
use kn_model::{Align, Color, DrawableKind, VAlign};
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

fn object_state(app: &mut App, sel: &[kn_model::Drawable]) -> Value {
    let d = &sel[0];
    let tabs = crate::objapi::object_tabs(d);
    let tab = app.insp.format_tab.min(tabs.len() - 1);
    let data = match tabs[tab] {
        "Text" => text_state(app, d),
        t => crate::objapi::pane(app, d, t),
    };
    json!({ "pane": "object", "tabs": tabs, "tab": tab, "kind": tabs[tab], "what": kind_name(d), "count": sel.len(), "data": data })
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

// ------------------------------------------------------------------ Format: the slide

fn slide_state(app: &mut App, scene: &kn_model::SlideScene) -> Value {
    let Some(doc) = app.doc.as_ref() else { return json!({ "pane": "none" }) };
    let slide = scene.slide;
    let layout = doc.layout_name(slide);
    let layout_id = doc.file.obj(slide).and_then(|m| m.ref_id("template_slide"));
    let all = doc.layouts();
    let layouts: Vec<String> = all.iter().map(|(_, n)| n.clone()).collect();
    let layout_index = all.iter().position(|(id, _)| Some(*id) == layout_id);
    // Appearance: Title / Body / Slide Number shown, and whether the layout has them
    let shown = |k| scene.drawables.iter().any(|d| d.placeholder == Some(k) && !d.from_template);
    let has = |f: &str| doc.file.obj(slide).and_then(|m| m.ref_id(f)).is_some();
    use kn_model::PlaceholderKind as P;
    let appearance: Vec<Value> = [("Title", P::Title, "titlePlaceholder"), ("Body", P::Body, "bodyPlaceholder"), ("Slide Number", P::SlideNumber, "slideNumberPlaceholder")]
        .iter()
        .map(|(l, k, f)| json!({ "label": l, "on": shown(*k), "enabled": has(f), "field": f }))
        .collect();
    // Background: Standard (a fill) or Dynamic (the theme's presets and the slide's look)
    let presets: Vec<(kn_format::ObjId, kn_model::motionbg::MotionBg)> = doc.motion_presets().into_iter().filter_map(|(id, _)| kn_model::motionbg::read(&doc.file, id).map(|m| (id, m))).collect();
    let tab = if scene.motion.is_some() { 1 } else { app.insp.bg_tab.unwrap_or(0) };
    let dynamic = scene.motion.as_ref().map(|mb| {
        let parent = doc.file.obj(mb.style).and_then(|s| s.find_super("TSS.StyleArchive")).and_then(|s| s.ref_id("parent"));
        let cur = presets.iter().position(|p| p.0 == mb.style).or_else(|| presets.iter().position(|p| Some(p.0) == parent));
        let controls: Vec<Value> = crate::inspector::motion_controls(&mb.family)
            .iter()
            .map(|&(label, prop, lo, hi, dec, suffix)| json!({ "label": label, "prop": prop, "min": lo, "max": hi, "decimals": dec, "suffix": suffix, "value": mb.num(prop, (lo + hi) / 2.0) }))
            .collect();
        json!({
            "preset": cur,
            "colors": mb.colors.iter().map(|(p, c)| json!([p, col(*c)])).collect::<Vec<_>>(),
            "background": mb.family.starts_with("KNWaves").then(|| col(mb.background_color.unwrap_or(Color::BLACK))),
            "controls": controls,
        })
    });
    json!({
        "pane": "slide", "layout": layout, "layouts": layouts, "layoutIndex": layout_index,
        "appearance": appearance,
        "bgTab": tab,
        "background": crate::objapi::fill_json(&scene.background, Some(doc)),
        "presets": presets.iter().map(|p| p.1.name.clone()).collect::<Vec<_>>(),
        "dynamic": dynamic,
        "previewMotion": app.preview_motion,
        "skipped": app.current_slide().is_some_and(|s| s.skipped),
    })
}

fn set_slide(app: &mut App, ctx: &egui::Context, k: &str, v: &Value) {
    use kn_model::motionbg::MotionProp;
    let Some(slide) = app.current_slide().map(|s| s.slide) else { return };
    let edit = |app: &mut App, name: &str, f: &mut dyn FnMut(&mut kn_model::Document, &mut kn_model::edit::History)| {
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, name);
            f(doc, &mut app.hist);
            doc.end(&mut app.hist);
        }
        app.bump(slide);
    };
    let size = app.doc.as_ref().map(|d| d.slide_size()).unwrap_or(kn_model::Size { w: 1920.0, h: 1080.0 });
    let motion = app.scene(app.cur).and_then(|s| s.motion);
    match k {
        "background" => {
            let Some(spec) = crate::objapi::fill_from(v, app.doc.as_mut()) else { return };
            edit(app, "Background", &mut |doc, h| {
                doc.set_slide_background(h, slide, &spec);
            });
        }
        "layout" => {
            let i = v.as_u64().unwrap_or(0) as usize;
            let Some(layout) = app.doc.as_ref().and_then(|d| d.layouts().get(i).map(|l| l.0)) else { return };
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Change Layout");
            let ok = doc.change_slide_layout(&mut app.hist, slide, layout);
            doc.end(&mut app.hist);
            if ok {
                app.bump_all();
            }
        }
        "appearance" => {
            let field = v.get("field").and_then(Value::as_str).unwrap_or_default().to_string();
            let on = v.get("on").and_then(Value::as_bool).unwrap_or(false);
            edit(app, if on { "Show Placeholder" } else { "Hide Placeholder" }, &mut |doc, h| {
                doc.set_placeholder_visible(h, slide, &field, on);
            });
        }
        "bgTab" => {
            let t = v.as_u64().unwrap_or(0) as usize;
            app.insp.bg_tab = Some(t);
            if t == 0 && motion.is_some() {
                edit(app, "Background", &mut |doc, h| {
                    doc.set_motion_background(h, slide, None, None);
                });
            }
        }
        "preset" => {
            let i = v.as_u64().unwrap_or(0) as usize;
            let p = app.doc.as_ref().and_then(|d| d.motion_presets().get(i).and_then(|(id, _)| kn_model::motionbg::read(&d.file, *id).map(|m| (*id, m))));
            if let Some((id, mb)) = p {
                let poster = if mb.poster.is_none() { kn_render::motion::poster_jpeg(&mb, size.w as u32, size.h as u32) } else { None };
                edit(app, "Dynamic Background", &mut |doc, h| {
                    doc.set_motion_background(h, slide, Some(id), poster.clone());
                });
            }
        }
        "previewMotion" => app.preview_motion = v.as_bool().unwrap_or(true),
        "colors" | "dynBackground" | "num" => {
            let Some(mb) = motion else { return };
            let change = match k {
                "colors" => MotionProp::Colors(v.as_array().map(|a| a.iter().filter_map(|s| Some((s.get(0)?.as_f64()? as f32, to_col(s.get(1)?)?))).collect()).unwrap_or_default()),
                "dynBackground" => MotionProp::Background(to_col(v).unwrap_or(Color::BLACK)),
                _ => {
                    let prop = v.get("prop").and_then(Value::as_str).unwrap_or_default();
                    let Some(&(_, p, ..)) = crate::inspector::motion_controls(&mb.family).iter().find(|c| c.1 == prop) else { return };
                    MotionProp::Num(p, v.get("value").and_then(Value::as_f64).unwrap_or(0.0))
                }
            };
            let mut look = mb.clone();
            match &change {
                MotionProp::Num(n, x) => {
                    look.nums.insert(n, *x);
                    if *n == "noise_scale_horizontal" {
                        look.nums.insert("noise_scale_vertical", *x);
                    }
                }
                MotionProp::Colors(cs) => look.colors = cs.clone(),
                MotionProp::Background(c) => look.background_color = Some(*c),
            }
            let poster = kn_render::motion::poster_jpeg(&look, size.w as u32, size.h as u32);
            edit(app, "Dynamic Background", &mut |doc, h| {
                doc.set_motion_property(h, slide, change.clone(), poster.clone());
                if let MotionProp::Num("noise_scale_horizontal", x) = change {
                    doc.set_motion_property(h, slide, MotionProp::Num("noise_scale_vertical", x), poster.clone());
                }
            });
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
    use kn_model::buildedit::{Delivery as D, effect_name};
    if let Some(&target) = app.sel.first() {
        let t = app.insp.animate_tab.min(2);
        let kind = KINDS[t];
        let sections: Vec<Value> = build_sections(kind).iter().map(|(title, names)| json!({ "title": title, "names": names.iter().filter(|n| effect_id(kind, n).is_some()).collect::<Vec<_>>() })).collect();
        let is_text = scene.drawables.iter().find(|d| d.id == target).is_some_and(|d| matches!(&d.kind, DrawableKind::Shape(s) if s.text.as_ref().is_some_and(|t| !t.text.trim().is_empty())));
        let builds: Vec<Value> = scene
            .builds
            .iter()
            .filter(|b| b.drawable == target && b.kind == kind.as_str())
            .map(|b| {
                let name = effect_name(&b.effect, &b.kind).map(|n| n.trim_end_matches(" (by character)").trim_end_matches(" (action)").to_string()).unwrap_or_else(|| crate::inspector::pretty_effect(&b.effect));
                let e = b.effect.as_str();
                let p = &b.params;
                let dirs = crate::inspector::direction_options(e).map(|(opts, def)| {
                    let cur = if opts.iter().any(|o| o.0 == b.direction) { b.direction } else { def };
                    json!({ "options": opts.iter().map(|o| json!([o.0, o.1])).collect::<Vec<_>>(), "value": cur })
                });
                let delivery = app.doc.as_ref().map(|d| d.build_delivery(b.id)).unwrap_or(D::AllAtOnce);
                let text_anim = match delivery {
                    D::ByWord => 1,
                    D::ByCharacter => 2,
                    _ => 0,
                };
                let deliv = [D::AllAtOnce, D::ByParagraph, D::ByParagraphGroup, D::ByHighlightedParagraph].iter().position(|d| *d == delivery).unwrap_or(0);
                let total = p.rotation_angle.unwrap_or(90.0) as f64;
                json!({
                    "id": b.id, "name": name, "duration": b.duration, "direction": dirs,
                    "bounce": e.contains("move in").then_some(p.bounce),
                    "rotation": (e == "apple:action-rotation").then(|| json!({ "clockwise": p.rotation_clockwise, "angle": total - (total / 360.0).floor() * 360.0, "turns": (total / 360.0).floor() })),
                    "scale": (e == "apple:action-scale").then(|| p.scale.unwrap_or(1.0) as f64 * 100.0),
                    "opacity": (e == "apple:action-opacity").then(|| p.opacity.unwrap_or(1.0) as f64 * 100.0),
                    "acceleration": (b.kind == "Action").then_some(p.acceleration.min(3)),
                    "textAnimation": (is_text && b.kind != "Action").then_some(text_anim),
                    "delivery": (is_text && b.kind != "Action").then_some(deliv),
                    "order": scene.build_order.iter().position(|s| s.build == b.id),
                })
            })
            .collect();
        return json!({ "pane": "build", "tabs": ["Build In", "Action", "Build Out"], "tab": t, "sections": sections, "builds": builds, "steps": scene.build_order.len(), "action": kind == BuildKind::Action });
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
    use kn_model::buildedit::{BuildOption as O, Delivery as D};
    let Some(scene) = crate::canvas::api::scene(app) else { return };
    let slide = scene.slide;
    match k {
        "tab" => {
            app.insp.animate_tab = (v.as_u64().unwrap_or(0) as usize).min(2);
            return;
        }
        "preview" => {
            let from = if scene.transition.is_some() && app.sel.is_empty() { app.cur.saturating_sub(1) } else { app.cur };
            app.start_slideshow(from);
            return;
        }
        "buildOrder" => {
            app.insp.show_build_order = !app.insp.show_build_order;
            return;
        }
        _ => {}
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
    let id = v.get("id").and_then(Value::as_u64);
    let b = scene.builds.iter().find(|b| Some(b.id) == id).cloned();
    let x = v.get("value").cloned().unwrap_or(Value::Null);
    let Some(doc) = app.doc.as_mut() else { return };
    match (k, b) {
        ("add", _) => {
            let Some(e) = x.as_str().and_then(|n| effect_id(kind, n)) else { return };
            doc.begin(&mut app.hist, "Add Build");
            let r = doc.add_build(&mut app.hist, slide, target, kind, e, 1.0);
            doc.end(&mut app.hist);
            if let Err(e) = r {
                app.notify(format!("Couldn't add the effect: {e}"));
            }
        }
        ("change", Some(b)) => {
            doc.begin(&mut app.hist, "Change Build");
            match x.as_str().and_then(|n| effect_id(kind, n)) {
                Some(e) => doc.set_build_effect(&mut app.hist, slide, b.id, e, b.duration),
                None => doc.remove_build(&mut app.hist, slide, b.id),
            };
            doc.end(&mut app.hist);
        }
        ("duration", Some(b)) => {
            doc.begin_merge(&mut app.hist, "Change Build");
            doc.set_build_effect(&mut app.hist, slide, b.id, &b.effect, x.as_f64().unwrap_or(1.0).clamp(0.1, 60.0));
            doc.end(&mut app.hist);
        }
        ("option", Some(b)) => {
            let n = x.as_f64().unwrap_or(0.0);
            let o = match v.get("option").and_then(Value::as_str).unwrap_or_default() {
                "direction" => O::Direction(n as u32),
                "bounce" => O::Bounce(x.as_bool().unwrap_or(false)),
                "clockwise" => O::RotationClockwise(x.as_bool().unwrap_or(true)),
                "angle" => O::RotationAngle(n),
                "scale" => O::ScaleSize(n / 100.0),
                "opacity" => O::Opacity(n / 100.0),
                "acceleration" => O::Acceleration(n as u32),
                _ => return,
            };
            doc.begin_merge(&mut app.hist, "Effect Options");
            doc.set_build_option(&mut app.hist, b.id, o);
            doc.end(&mut app.hist);
        }
        ("textAnimation" | "delivery", Some(b)) => {
            let i = x.as_u64().unwrap_or(0) as usize;
            let d = if k == "textAnimation" { [D::AllAtOnce, D::ByWord, D::ByCharacter][i.min(2)] } else { [D::AllAtOnce, D::ByParagraph, D::ByParagraphGroup, D::ByHighlightedParagraph][i.min(3)] };
            doc.begin(&mut app.hist, "Delivery");
            doc.set_build_delivery(&mut app.hist, slide, b.id, d);
            doc.end(&mut app.hist);
        }
        ("order", Some(b)) => {
            let Some(from) = scene.build_order.iter().position(|s| s.build == b.id) else { return };
            doc.begin(&mut app.hist, "Reorder Builds");
            doc.move_build_step(&mut app.hist, slide, from, (x.as_u64().unwrap_or(0) as usize).min(scene.build_order.len().saturating_sub(1)));
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
        "sizeName": if (size.w - 1920.0).abs() < 0.5 && (size.h - 1080.0).abs() < 0.5 { "Wide (16:9)".to_string() } else if (size.w - 1024.0).abs() < 0.5 && (size.h - 768.0).abs() < 0.5 { "Standard (4:3)".to_string() } else { format!("Custom ({:.0} \u{00d7} {:.0})", size.w, size.h) },
        "theme": doc.theme_name(),
        "idle": s.restart_if_idle,
        "idleMinutes": s.idle_minutes,
        "password": doc.password.is_some(),
        "tab": app.insp.doc_tab.min(1),
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
        "idle" => ShowSetting::RestartIfIdle(v.as_bool().unwrap_or(false)),
        "idleMinutes" => ShowSetting::IdleMinutes(v.as_f64().unwrap_or(15.0).clamp(1.0, 1440.0)),
        "tab" => {
            app.insp.doc_tab = (v.as_u64().unwrap_or(0) as usize).min(1);
            return;
        }
        "changeTheme" => {
            app.changing_theme = true;
            app.show_theme_chooser = true;
            return;
        }
        "password" => {
            if v.as_bool() == Some(false) {
                crate::versions::set_password(app, "", "");
            } else {
                app.dialogs.set_password = Some((String::new(), String::new(), String::new()));
            }
            return;
        }
        "size" => {
            let (w, h) = match v {
                Value::Array(a) => (a.first().and_then(Value::as_f64).unwrap_or(1920.0).clamp(16.0, 10000.0) as f32, a.get(1).and_then(Value::as_f64).unwrap_or(1080.0).clamp(16.0, 10000.0) as f32),
                _ if v.as_str() == Some("4:3") => (1024.0, 768.0),
                _ => (1920.0, 1080.0),
            };
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
        "obj" if k == "tab" => app.insp.format_tab = v.as_u64().unwrap_or(0) as usize,
        "text" => set_text(app, k, v),
        p if crate::objapi::set(app, p, k, v) => {}
        "slide" => set_slide(app, ctx, k, v),
        "animate" => set_animate(app, k, v),
        "doc" => set_document(app, k, v),
        _ => {}
    }
}
