//! JSON requests for apps that reach the engine through the C ABI (the Flutter app).
//! `{"cmd": "...", ...}` in, a JSON value out. See API.md for the list.

use crate::format::NumberStyle;
use crate::layout::{Insets, Mode};
use crate::session::{key_for_char, Session};
use crate::{icons, notes, theme, units};
use serde_json::{json, Value};

fn f(v: &Value, k: &str) -> f32 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

/// Everything needed to draw the calculator screen.
pub fn frame(sn: &mut Session) -> Value {
    let layout = sn.layout();
    json!({
        "theme": sn.theme(),
        "settings": sn.settings,
        "display": sn.display(),
        "layout": layout,
        "convert": sn.convert_state(),
    })
}

fn note_info(text: &str, rad: bool, style: NumberStyle) -> Value {
    json!({
        "text": text,
        "results": notes::evaluate(text, rad, style),
        "graphs": notes::graphs(text),
    })
}

pub fn call(sn: &mut Session, req: &Value) -> Value {
    let cmd = s(req, "cmd");
    match cmd {
        "frame" => {
            let i = req.get("insets").cloned().unwrap_or(Value::Null);
            sn.set_size(f(req, "w"), f(req, "h"), Insets { top: f(&i, "top"), bottom: f(&i, "bottom"), left: f(&i, "left"), right: f(&i, "right") });
            frame(sn)
        }
        "get" => frame(sn),
        "press" => {
            sn.press(s(req, "key"));
            frame(sn)
        }
        "swipe" => {
            sn.swipe_delete();
            frame(sn)
        }
        "char" => {
            let k = s(req, "ch").chars().next().and_then(key_for_char);
            if let Some(k) = k {
                sn.press(k);
            }
            frame(sn)
        }
        "mode" => {
            let m = match s(req, "mode") {
                "Scientific" => Mode::Scientific,
                "Notes" => Mode::Notes,
                _ => Mode::Basic,
            };
            sn.set_mode(m);
            frame(sn)
        }
        "convert" => {
            sn.set_convert(req.get("on").and_then(|v| v.as_bool()).unwrap_or(false));
            frame(sn)
        }
        "theme" => {
            sn.set_theme(s(req, "id"));
            frame(sn)
        }
        "style" => {
            let st = match s(req, "style") {
                "Dot" => NumberStyle::Dot,
                "Space" => NumberStyle::Space,
                _ => NumberStyle::Comma,
            };
            sn.set_style(st);
            frame(sn)
        }
        "sidebar" => {
            sn.settings.sidebar_open = req.get("open").and_then(|v| v.as_bool()).unwrap_or(true);
            sn.save_settings();
            frame(sn)
        }
        "settings" => {
            if let Some(b) = req.get("haptics").and_then(|v| v.as_bool()) {
                sn.settings.haptics = b;
            }
            if let Some(b) = req.get("sounds").and_then(|v| v.as_bool()) {
                sn.settings.sounds = b;
            }
            sn.save_settings();
            frame(sn)
        }
        "themes" => json!(theme::themes()),
        "styles" => json!(NumberStyle::all().iter().map(|(st, ex)| json!({"style": st, "example": ex})).collect::<Vec<_>>()),
        "units" => json!(units::categories(&sn.rates)),
        "unit_search" => json!(units::search(&sn.rates, s(req, "q")).into_iter().map(|(c, u)| json!({"cat": c, "unit": u})).collect::<Vec<_>>()),
        "set_unit" => {
            sn.set_unit(req.get("top").and_then(|v| v.as_bool()).unwrap_or(true), s(req, "cat"), s(req, "unit"));
            frame(sn)
        }
        "swap" => {
            sn.swap_units();
            frame(sn)
        }
        "rates_url" => json!(units::RATES_URL),
        "rates_stale" => json!(sn.rates_stale()),
        "rates_set" => json!(sn.set_rates_json(s(req, "json"))),
        "history" => json!(sn.history_groups(req.get("utc_offset").and_then(|v| v.as_i64()).unwrap_or(0) as i32)),
        "history_load" => {
            sn.history_load(req.get("id").and_then(|v| v.as_u64()).unwrap_or(0));
            frame(sn)
        }
        "history_delete" => {
            let ids: Vec<u64> = req.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64()).collect()).unwrap_or_default();
            sn.history_delete(&ids);
            json!(true)
        }
        "history_clear" => {
            sn.history_clear();
            json!(true)
        }
        "history_text" => json!(sn.history_text(req.get("id").and_then(|v| v.as_u64()).unwrap_or(0))),
        "copy" => json!(sn.copy_text()),
        "paste" => {
            let ok = sn.paste(s(req, "text"));
            json!({"ok": ok, "frame": frame(sn)})
        }
        "notes" => {
            let off = req.get("utc_offset").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            json!(sn
                .notes
                .sorted()
                .iter()
                .map(|n| json!({"id": n.id, "title": notes::title(&n.text), "preview": notes::preview(&n.text), "modified": n.modified, "date": crate::history::short_date(n.modified, off)}))
                .collect::<Vec<_>>())
        }
        "note_new" => {
            let id = sn.notes.create(crate::session::now());
            sn.save_notes();
            json!(id)
        }
        "note_get" => {
            let id = req.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            match sn.notes.get(id) {
                Some(n) => note_info(&n.text, sn.calc.rad, sn.settings.style),
                None => Value::Null,
            }
        }
        "note_set" => {
            let id = req.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let text = s(req, "text").to_string();
            let info = note_info(&text, sn.calc.rad, sn.settings.style);
            sn.notes.set_text(id, text, crate::session::now());
            sn.save_notes();
            info
        }
        "note_eval" => note_info(s(req, "text"), sn.calc.rad, sn.settings.style),
        "note_delete" => {
            sn.notes.delete(req.get("id").and_then(|v| v.as_u64()).unwrap_or(0));
            sn.save_notes();
            json!(true)
        }
        "note_graph" => {
            let text = s(req, "text");
            let line = req.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let x0 = req.get("x0").and_then(|v| v.as_f64()).unwrap_or(-10.0);
            let x1 = req.get("x1").and_then(|v| v.as_f64()).unwrap_or(10.0);
            let n = req.get("n").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
            json!(notes::graph(text, line, sn.calc.rad, x0, x1, n))
        }
        "icons" => {
            let m: serde_json::Map<String, Value> = icons::NAMES.iter().filter_map(|n| icons::icon(n).map(|i| (n.to_string(), json!(i)))).collect();
            Value::Object(m)
        }
        _ => json!({"error": format!("unknown cmd {cmd}")}),
    }
}
