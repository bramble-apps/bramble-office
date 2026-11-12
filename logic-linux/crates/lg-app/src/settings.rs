//! Settings (Logic Pro ▸ Settings, ⌘,), drawn as Logic 13's window: the title band with its ten
//! icon tabs (General … My Info), the sub-tab segmented control on the group box's top border,
//! and each sub-tab's rows: right-aligned labels, checkboxes, pop-ups, steppers, sliders,
//! buttons, tables. The rows, their order, defaults and the measured look come from Logic's own
//! window (data/settings-layout.txt, transcribed from the research's screenshots).
//!
//! Values persist in ~/.config/bramble-studio/settings-logic.txt, keyed "Tab / Sub / control".
//! The suite's look (appearance, colour theme, liquid glass, title bar) and Bramble's SoundFont
//! choice sit at the end of View ▸ General and Audio ▸ General, under "Bramble".

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::{App, SoundFontMode};
use crate::look;

#[derive(Clone, Debug)]
pub struct Row {
    pub label: String,
    pub kind: String,
    pub text: String,
    pub default: String,
    pub choices: Vec<String>,
    /// "# heading" / "# small" before it.
    pub style: Option<&'static str>,
    /// Its key in the settings file ("Tab / Sub / control"): unique in its section (a popup on
    /// the line of another gets that one's label + " Audio", a repeated name " #2"…).
    pub key: String,
}

#[derive(Clone, Debug)]
pub struct Section {
    pub tab: String,
    pub sub: String,
    pub height: f32,
    pub label_right: f32,
    /// Where the controls start, and the widths Logic gives this tab's pop-ups, steppers and
    /// sliders (from the layout's notes).
    pub ctrl_x: Option<f32>,
    pub popup_w: f32,
    pub stepper_w: f32,
    pub slider_w: f32,
    pub rows: Vec<Row>,
}

/// The number after `key` in a layout note ("controls at x≈202" → 202).
fn num_after(c: &str, key: &str) -> Option<f32> {
    let i = c.find(key)? + key.len();
    let rest = c[i..].trim_start_matches(['≈', ' ', 'x']);
    let n: String = rest.chars().take_while(|ch| ch.is_ascii_digit() || *ch == '.').collect();
    n.parse().ok()
}

/// "popups 309 wide" → 309 (the first width given for that kind).
fn width_of(c: &str, kind: &str) -> Option<f32> {
    let i = c.find(kind)? + kind.len();
    let n: String = c[i..].trim_start().chars().take_while(|ch| ch.is_ascii_digit() || *ch == '.').collect();
    n.parse().ok()
}

pub fn sections() -> &'static [Section] {
    static S: OnceLock<Vec<Section>> = OnceLock::new();
    S.get_or_init(|| {
        let mut out: Vec<Section> = Vec::new();
        let mut style = None;
        for line in include_str!("../data/settings-layout.txt").lines() {
            if let Some(name) = line.strip_prefix("=== ").and_then(|l| l.strip_suffix(" ===")) {
                let (tab, sub) = name.split_once(" / ").unwrap_or((name, ""));
                out.push(Section { tab: tab.to_string(), sub: sub.to_string(), height: 540.0, label_right: 190.0, ctrl_x: None, popup_w: 235.0, stepper_w: 105.0, slider_w: 265.0, rows: Vec::new() });
                continue;
            }
            let Some(sec) = out.last_mut() else { continue };
            if let Some(c) = line.strip_prefix("# ") {
                if let Some(w) = c.strip_prefix("window: ") {
                    if let Some(h) = w.split('x').nth(1).and_then(|h| h.split_whitespace().next()).and_then(|h| h.parse().ok()) {
                        sec.height = h;
                    }
                } else if c.contains("label column right edge") || c.contains("controls at") {
                    if let Some(v) = num_after(c, "label column right edge") {
                        sec.label_right = v;
                    }
                    if let Some(v) = num_after(c, "controls at") {
                        sec.ctrl_x = Some(v);
                    }
                    if let Some(v) = width_of(c, "popups ").or_else(|| width_of(c, "popup ")) {
                        sec.popup_w = v;
                    }
                    if let Some(v) = width_of(c, "steppers ").or_else(|| width_of(c, "stepper ")) {
                        sec.stepper_w = v;
                    }
                    if let Some(v) = width_of(c, "sliders ").or_else(|| width_of(c, "slider ")) {
                        sec.slider_w = v;
                    }
                } else if c.starts_with("heading") {
                    style = Some("heading");
                } else if c.starts_with("small") {
                    style = Some("small");
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("row\t") {
                let f: Vec<&str> = rest.split('\t').collect();
                let g = |i: usize| f.get(i).copied().unwrap_or("").to_string();
                let choices = g(4).trim_start_matches('~').split('|').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                let mut row = Row { label: g(0), kind: g(1), text: g(2), default: g(3), choices, style: style.take(), key: String::new() };
                let mut key = row_key(sec, &row);
                if row.label == "^" && row.kind == "popup" {
                    if let Some(prev) = sec.rows.iter().rev().find(|r| r.label != "^" && !r.label.is_empty()) {
                        key = format!("{} / {} / {} Audio", sec.tab, sec.sub, prev.label);
                    }
                }
                let mut n = 1;
                let base = key.clone();
                while sec.rows.iter().any(|r| r.key == key) {
                    n += 1;
                    key = format!("{base} #{n}");
                }
                row.key = key;
                sec.rows.push(row);
            }
        }
        out
    })
}

pub const TABS: [&str; 10] = ["General", "Audio", "Recording", "MIDI", "Score", "Movie", "Automation", "Control Surfaces", "View", "My Info"];

fn config() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("settings-logic.txt"))
}

/// The values set (key → value); the defaults are the layout's.
#[derive(Default, Clone)]
pub struct Values(pub HashMap<String, String>);

impl Values {
    pub fn load() -> Values {
        let mut v = HashMap::new();
        if let Some(t) = config().and_then(|p| std::fs::read_to_string(p).ok()) {
            for l in t.lines() {
                if let Some((k, x)) = l.split_once('=') {
                    v.insert(k.to_string(), x.to_string());
                }
            }
        }
        Values(v)
    }

    pub fn save(&self) {
        let Some(p) = config() else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let mut keys: Vec<&String> = self.0.keys().collect();
        keys.sort();
        let text: String = keys.into_iter().map(|k| format!("{k}={}\n", self.0[k])).collect();
        let _ = std::fs::write(p, text);
    }

    /// A setting by its key ("General / Editing / Number of Undo Steps:"), else its default.
    pub fn get(&self, key: &str) -> String {
        if let Some(v) = self.0.get(key) {
            return v.clone();
        }
        for s in sections() {
            for r in &s.rows {
                if r.key == key {
                    return r.default.clone();
                }
            }
        }
        String::new()
    }

    pub fn on(&self, key: &str) -> bool {
        self.get(key) == "1"
    }
}

fn row_key(s: &Section, r: &Row) -> String {
    let id = if !r.text.is_empty() && r.kind == "check" { &r.text } else if !r.label.is_empty() && r.label != "^" { &r.label } else { &r.text };
    format!("{} / {} / {}", s.tab, s.sub, id)
}

// ---- the measured look (data/settings-layout.txt "# look:")
fn c(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
const W: f32 = 750.0;
const BAND_H: f32 = 88.0;

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.show_settings {
        return;
    }
    let tab = app.settings_tab.clone();
    let subs: Vec<&Section> = sections().iter().filter(|s| s.tab == tab).collect();
    let sub_name = if subs.iter().any(|s| s.sub == app.settings_sub) { app.settings_sub.clone() } else { subs.first().map(|s| s.sub.clone()).unwrap_or_default() };
    let sec = subs.iter().find(|s| s.sub == sub_name).copied();
    let height = sec.map_or(400.0, |s| s.height.max(260.0)) + bramble_extra_height(&tab, &sub_name);
    let mut open = true;
    let screen = ctx.content_rect();
    egui::Window::new("Settings").id(egui::Id::unique("logic-settings")).title_bar(false).resizable(false).collapsible(false).fixed_size(vec2(W, height)).default_pos(pos2(screen.center().x - W / 2.0, screen.min.y + 80.0)).frame(egui::Frame::NONE).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(W, height), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(r, 12.0, c(0x262524));
        p.rect_stroke(r, 12.0, Stroke::new(0.5, Color32::BLACK), egui::StrokeKind::Outside);
        let band = Rect::from_min_size(r.min, vec2(W, BAND_H));
        p.rect_filled(band, egui::CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 }, c(0x2e2d2c));
        p.hline(band.x_range(), band.max.y, Stroke::new(0.5, c(0x353433)));
        p.text(pos2(band.center().x, band.min.y + 15.5), Align2::CENTER_CENTER, "Settings", look::bold(13.0), c(0xaaa9a8));
        // the close light (the others are the window's; red closes)
        let close = Rect::from_center_size(band.min + vec2(16.0, 16.0), vec2(14.0, 14.0));
        let cr = ui.interact(close, ui.scope_id().with("set-close"), Sense::click());
        p.circle_filled(close.center(), 7.0, c(0xe1675b));
        for k in 1..3 {
            p.circle_filled(close.center() + vec2(20.0 * k as f32, 0.0), 7.0, c(0x525150));
        }
        if cr.clicked() {
            open = false;
        }
        // the icon tabs, centred as a row
        let widths: Vec<f32> = TABS.iter().map(|t| p.layout_no_wrap(t.to_string(), look::regular(11.0), Color32::WHITE).size().x + 16.0).collect();
        let gap = 10.0;
        let total = widths.iter().sum::<f32>() + gap * (TABS.len() - 1) as f32;
        let mut x = band.center().x - total / 2.0;
        for (i, t) in TABS.iter().enumerate() {
            let tr = Rect::from_min_size(pos2(x, band.min.y + 32.0), vec2(widths[i].max(48.0), 48.0));
            x += tr.width() + gap;
            let sel = *t == tab;
            let resp = ui.interact(tr, ui.scope_id().with(("set-tab", i)), Sense::click());
            if sel {
                p.rect_filled(tr, 7.5, c(0x4b4a49));
                p.rect_stroke(tr, 7.5, Stroke::new(0.5, c(0x100f0f)), egui::StrokeKind::Outside);
                p.hline(tr.min.x + 7.0..=tr.max.x - 7.0, tr.min.y + 0.5, Stroke::new(0.5, c(0x7b7977)));
            }
            let icol = if sel { c(0x3478f6) } else { c(0xa1a1a0) };
            tab_icon(&p, pos2(tr.center().x, band.min.y + 48.0), i, icol);
            p.text(pos2(tr.center().x, band.min.y + 68.0), Align2::CENTER_CENTER, *t, look::regular(11.0), if sel { c(0x3d85f7) } else { c(0x979696) });
            if resp.clicked() {
                app.settings_tab = t.to_string();
                app.settings_sub.clear();
            }
        }
        let Some(sec) = sec else {
            p.text(pos2(r.center().x, band.max.y + 60.0), Align2::CENTER_CENTER, "My Info: your name and picture for shared projects (not needed here)", look::regular(13.0), c(0x979696));
            return;
        };
        // the sub-tab box and its segmented control
        let boxed = subs.len() > 1;
        let content_left;
        let mut y;
        if boxed {
            let bx = Rect::from_min_max(pos2(r.min.x + 20.0, band.max.y + 30.0), pos2(r.max.x - 20.0, r.max.y - 20.0));
            p.rect_filled(bx, 4.0, c(0x2d2c2b));
            p.rect_stroke(bx, 4.0, Stroke::new(1.0, c(0x464545)), egui::StrokeKind::Inside);
            let fontsz = look::regular(13.0);
            let segw: Vec<f32> = subs.iter().map(|s| p.layout_no_wrap(s.sub.clone(), fontsz.clone(), Color32::WHITE).size().x + 30.0).collect();
            let tw: f32 = segw.iter().sum();
            let seg = Rect::from_center_size(pos2(bx.center().x, bx.min.y), vec2(tw + 4.0, 24.0));
            p.rect_filled(seg, 5.0, c(0x363534));
            let mut sx = seg.min.x + 2.0;
            for (k, s) in subs.iter().enumerate() {
                let sr = Rect::from_min_size(pos2(sx, seg.min.y + 2.0), vec2(segw[k], 20.0));
                sx += segw[k];
                if s.sub == sub_name {
                    p.rect_filled(sr, 4.0, c(0x545251));
                    p.rect_stroke(sr, 4.0, Stroke::new(0.5, c(0x7c7978)), egui::StrokeKind::Inside);
                }
                p.text(sr.center(), Align2::CENTER_CENTER, &s.sub, fontsz.clone(), c(0xe0e0e0));
                if ui.interact(sr, ui.scope_id().with(("set-sub", k)), Sense::click()).clicked() {
                    app.settings_sub = s.sub.clone();
                }
            }
            content_left = bx.min.x + 23.0;
            y = bx.min.y + 30.0;
        } else {
            content_left = r.min.x + 20.0;
            y = band.max.y + 20.0;
        }
        let label_right = r.min.x + sec.label_right;
        let ctrl_x = sec.ctrl_x.map_or(label_right + 9.5, |x| r.min.x + x);
        let fill = if boxed { c(0x3d3c3b) } else { c(0x363534) };
        let mut last_y = y;
        let mut values = app.logic_settings.clone();
        let mut changed = false;
        let mut touched: Vec<String> = Vec::new();
        let mut in_group = false;
        let mut last_right = ctrl_x;
        for (k, row) in sec.rows.iter().enumerate() {
            // (I/O Assignments: surround_ui draws the Output / Bounce Extensions / Input panes)
            if crate::surround_ui::skip_setting_row(&sec.tab, &sec.sub, &row.kind) {
                continue;
            }
            let same_line = row.label == "^";
            let ry = if same_line { last_y } else { y };
            let key = row.key.clone();
            let cur = crate::prefs::live_get(app, &key).unwrap_or_else(|| values.get(&key));
            let id = ui.scope_id().with(("set-row", &sec.tab, &sec.sub, k));
            let pitch;
            if !row.label.is_empty() && !same_line {
                p.text(pos2(label_right, ry + 12.0), Align2::RIGHT_CENTER, &row.label, look::regular(13.0), c(0xdfdfdf));
            }
            let x0 = if same_line { last_right } else if row.label.is_empty() && matches!(row.kind.as_str(), "text" | "separator" | "table") { content_left } else { ctrl_x };
            let disabled = row.text.ends_with("(disabled)") && !crate::prefs::enabled_now(app, &key);
            let dyn_text = crate::prefs::dynamic_text(&values, &key);
            let text = dyn_text.as_deref().unwrap_or(row.text.trim_end_matches("(disabled)").trim());
            let tcol = if disabled { c(0x61605f) } else { c(0xdfdfdf) };
            let mut right = x0;
            match row.kind.as_str() {
                "check" => {
                    let b = Rect::from_min_size(pos2(x0, ry + 4.0), vec2(16.0, 16.0));
                    let on = cur == "1";
                    p.rect_filled(b, 3.5, if on { if disabled { c(0x3c3b3a) } else { c(0x397bf7) } } else if boxed { c(0x424140) } else { c(0x3b3a39) });
                    if on {
                        let tick = if disabled { c(0x6c6b6b) } else { Color32::WHITE };
                        p.line_segment([b.center() + vec2(-4.0, 0.0), b.center() + vec2(-1.2, 3.2)], Stroke::new(2.0, tick));
                        p.line_segment([b.center() + vec2(-1.2, 3.2), b.center() + vec2(4.5, -3.8)], Stroke::new(2.0, tick));
                    }
                    // (long labels wrap at the box's right, the box beside their first line)
                    let line_h = p.layout_no_wrap("A".into(), look::regular(13.0), tcol).size().y;
                    let tg = p.layout(text.to_string(), look::regular(13.0), tcol, (r.max.x - 45.0 - x0 - 22.5).max(100.0));
                    let tw = tg.size().x;
                    let extra = (tg.size().y - line_h).max(0.0);
                    p.galley(pos2(x0 + 22.5, ry + 12.0 - line_h / 2.0), tg, tcol);
                    let hit = Rect::from_min_size(b.min, vec2(22.5 + tw, 16.0 + extra));
                    if !disabled && ui.interact(hit, id, Sense::click()).clicked() {
                        values.0.insert(key.clone(), if on { "0".into() } else { "1".into() });
                        changed = true;
                        touched.push(key.clone());
                    }
                    right = hit.max.x;
                    pitch = 22.0 + extra;
                }
                "popup" => {
                    let w = if row.default.len() > 26 { sec.popup_w.max(309.0) } else { sec.popup_w };
                    let pr = Rect::from_min_size(pos2(x0, ry), vec2(w, 24.0));
                    let resp = ui.interact(pr, id, Sense::click());
                    p.rect_filled(pr, 5.0, if resp.hovered() { look::mix(fill, Color32::WHITE, 0.06) } else { fill });
                    p.with_clip_rect(pr.shrink2(vec2(10.0, 0.0))).text(pos2(pr.min.x + 12.0, pr.center().y), Align2::LEFT_CENTER, &cur, look::regular(13.0), tcol);
                    let m = pos2(pr.max.x - 14.0, pr.center().y);
                    for s in [-1.0f32, 1.0] {
                        p.line_segment([m + vec2(-3.5, s * 1.5), m + vec2(0.0, s * 5.0)], Stroke::new(1.3, c(0xe1e1e1)));
                        p.line_segment([m + vec2(0.0, s * 5.0), m + vec2(3.5, s * 1.5)], Stroke::new(1.3, c(0xe1e1e1)));
                    }
                    let mut choices = crate::prefs::choices(app, &key).unwrap_or_else(|| row.choices.clone());
                    if !choices.contains(&cur) {
                        choices.insert(0, cur.clone());
                    }
                    egui::Popup::menu(&resp).show(|ui| {
                        ui.set_min_width(w);
                        for ch in &choices {
                            if ui.add(egui::Button::selectable(*ch == cur, ch)).clicked() {
                                values.0.insert(key.clone(), ch.clone());
                                changed = true;
                                touched.push(key.clone());
                                ui.close();
                            }
                        }
                    });
                    right = pr.max.x;
                    pitch = 31.0;
                }
                "stepper" => {
                    let w = sec.stepper_w;
                    let sr = Rect::from_min_size(pos2(x0, ry), vec2(w, 24.0));
                    p.rect_filled(sr, 5.0, if disabled { c(0x353433) } else { fill });
                    p.text(sr.center(), Align2::CENTER_CENTER, &cur, look::regular(13.0), if disabled { c(0x676665) } else { c(0xdfdfdf) });
                    let num: Option<f64> = cur.trim().parse().ok();
                    for (s, dir) in [(-1.0f32, -1.0f64), (1.0, 1.0)] {
                        let cx = if s < 0.0 { sr.min.x + 12.0 } else { sr.max.x - 12.0 };
                        let a = Rect::from_center_size(pos2(cx, sr.center().y), vec2(20.0, 24.0));
                        let m = a.center();
                        let up = s > 0.0;
                        let k = if up { -1.0 } else { 1.0 };
                        p.line_segment([m + vec2(-4.5, -k * 2.0), m + vec2(0.0, k * 2.5)], Stroke::new(1.3, c(0x8b8a89)));
                        p.line_segment([m + vec2(0.0, k * 2.5), m + vec2(4.5, -k * 2.0)], Stroke::new(1.3, c(0x8b8a89)));
                        if let (false, Some(n)) = (disabled, num) {
                            if ui.interact(a, id.with(("step", up)), Sense::click()).clicked() {
                                let step = if cur.contains('.') { 0.1 } else { 1.0 };
                                let v = n + dir * step;
                                values.0.insert(key.clone(), if cur.contains('.') { format!("{v:.1}") } else { format!("{v:.0}") });
                                changed = true;
                            }
                        }
                    }
                    right = sr.max.x;
                    pitch = 31.0;
                }
                "slider" => {
                    let w = sec.slider_w;
                    let sr = Rect::from_min_size(pos2(x0, ry + 4.0), vec2(w, 16.0));
                    let (lo, hi) = row.choices.first().and_then(|c| c.split_once("..")).and_then(|(a, b)| Some((a.trim().parse::<f32>().ok()?, b.trim().parse::<f32>().ok()?))).unwrap_or((0.0, 100.0));
                    let v: f32 = cur.parse().unwrap_or(lo);
                    let f = ((v - lo) / (hi - lo).max(1e-6)).clamp(0.0, 1.0);
                    let track = Rect::from_center_size(sr.center(), vec2(w, 6.0));
                    p.rect_filled(track, 3.0, c(0x424140));
                    // (a disabled slider has no blue fill)
                    if !disabled {
                        p.rect_filled(Rect::from_min_size(track.min, vec2(w * f, 6.0)), 3.0, c(0x397bf7));
                    }
                    p.rect_filled(Rect::from_center_size(pos2(track.min.x + w * f, track.center().y), vec2(20.0, 16.0)), 8.0, c(0xe0e0e0));
                    let resp = ui.interact(sr.expand(4.0), id, Sense::click_and_drag());
                    if let (false, true, Some(pp)) = (disabled, resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
                        let nf = ((pp.x - track.min.x) / w).clamp(0.0, 1.0);
                        let nv = lo + nf * (hi - lo);
                        values.0.insert(key.clone(), format!("{:.0}", nv));
                        changed = true;
                    }
                    right = sr.max.x;
                    pitch = 31.0;
                }
                "radio" => {
                    let mut xx = x0;
                    for ch in &row.choices {
                        let on = *ch == cur;
                        let cc = pos2(xx + 8.0, ry + 12.0);
                        p.circle_filled(cc, 8.0, if on { c(0x397bf7) } else { c(0x3b3a39) });
                        if on {
                            p.circle_filled(cc, 2.5, Color32::WHITE);
                        }
                        let g = p.layout_no_wrap(ch.clone(), look::regular(13.0), tcol);
                        let gw = g.size().x;
                        p.galley(pos2(xx + 22.5, ry + 12.0 - g.size().y / 2.0), g, tcol);
                        if ui.interact(Rect::from_min_size(pos2(xx, ry + 4.0), vec2(22.5 + gw, 16.0)), id.with(ch), Sense::click()).clicked() {
                            values.0.insert(key.clone(), ch.clone());
                            changed = true;
                        }
                        xx += 22.5 + gw + 18.0;
                    }
                    right = xx;
                    pitch = 26.0;
                }
                "button" => {
                    // (Apply: on while the device settings differ from the running output's)
                    let disabled = if text == "Apply" { !crate::prefs::apply_pending(&values) } else { disabled };
                    let tcol = if disabled { c(0x61605f) } else { c(0xdfdfdf) };
                    let g = p.layout_no_wrap(text.to_string(), look::regular(13.0), tcol);
                    let br = Rect::from_min_size(pos2(x0, ry), vec2((g.size().x + 28.0).max(80.0), 24.0));
                    p.rect_filled(br, 5.0, if disabled { if boxed { c(0x353433) } else { c(0x2e2d2b) } } else { fill });
                    p.galley(br.center() - g.size() / 2.0, g, if disabled { c(0x676665) } else { c(0xe1e1e1) });
                    if !disabled && ui.interact(br, id, Sense::click()).clicked() {
                        // (the values so far first: Apply reads them)
                        app.logic_settings = values.clone();
                        let bkey = format!("{} / {} / {}", sec.tab, sec.sub, text);
                        if !crate::prefs::button(app, &bkey) {
                            app.tell(format!("“{text}” isn't available yet"));
                        }
                    }
                    right = br.max.x;
                    pitch = 31.0;
                }
                "field" => {
                    let fr = Rect::from_min_size(pos2(x0, ry), vec2(235.0, 24.0));
                    let mut t = cur.clone();
                    let te = ui.put(fr, egui::TextEdit::singleline(&mut t).font(look::regular(13.0)));
                    if te.changed() {
                        values.0.insert(key.clone(), t);
                        changed = true;
                    }
                    right = fr.max.x;
                    pitch = 31.0;
                }
                "separator" => {
                    let rr = if boxed { r.max.x - 43.0 } else { r.max.x - 20.0 };
                    p.hline(content_left..=rr, ry + 6.0, Stroke::new(1.0, if boxed { c(0x424140) } else { c(0x3b3a39) }));
                    pitch = 13.0;
                }
                "table" => {
                    let rr = if boxed { r.max.x - 43.0 } else { r.max.x - 20.0 };
                    // (MIDI ▸ Inputs: a row per input port, its On checkbox)
                    let ports: Vec<String> = if sec.tab == "MIDI" && sec.sub == "Inputs" { app.live.all_ports.clone() } else { Vec::new() };
                    let nrows = ports.len().max(3);
                    let tr = Rect::from_min_max(pos2(x0, ry), pos2(rr, ry + 29.0 + 24.0 * nrows as f32));
                    p.rect_stroke(tr, 0.0, Stroke::new(1.0, c(0x424140)), egui::StrokeKind::Inside);
                    let head = Rect::from_min_size(tr.min, vec2(tr.width(), 29.0));
                    p.rect_filled(head, 0.0, c(0x2e2d2c));
                    let n = row.choices.len().max(1);
                    for (ci, title) in row.choices.iter().enumerate() {
                        let cx = head.min.x + head.width() * ci as f32 / n as f32;
                        p.text(pos2(cx + 8.0, head.center().y), Align2::LEFT_CENTER, title, look::regular(11.0), Color32::WHITE);
                        if ci > 0 {
                            p.vline(cx, head.y_range(), Stroke::new(0.5, c(0x525150)));
                        }
                    }
                    for ri in 0..nrows {
                        let rrw = Rect::from_min_size(pos2(tr.min.x + 1.0, head.max.y + ri as f32 * 24.0), vec2(tr.width() - 2.0, 24.0));
                        p.rect_filled(rrw, 0.0, if ri % 2 == 0 { c(0x262524) } else { c(0x302f2e) });
                        if let Some(port) = ports.get(ri) {
                            let pk = crate::prefs::input_key(port);
                            let on = values.get(&pk) != "0";
                            let b = Rect::from_min_size(pos2(rrw.min.x + 8.0, rrw.center().y - 8.0), vec2(16.0, 16.0));
                            p.rect_filled(b, 3.5, if on { c(0x397bf7) } else { c(0x424140) });
                            if on {
                                p.line_segment([b.center() + vec2(-4.0, 0.0), b.center() + vec2(-1.2, 3.2)], Stroke::new(2.0, Color32::WHITE));
                                p.line_segment([b.center() + vec2(-1.2, 3.2), b.center() + vec2(4.5, -3.8)], Stroke::new(2.0, Color32::WHITE));
                            }
                            let cx = rrw.min.x + rrw.width() / n as f32;
                            p.text(pos2(cx + 8.0, rrw.center().y), Align2::LEFT_CENTER, crate::live::port_label(port), look::regular(13.0), c(0xdfdfdf));
                            if ui.interact(b.expand(2.0), id.with(("port", ri)), Sense::click()).clicked() {
                                values.0.insert(pk.clone(), if on { "0".into() } else { "1".into() });
                                changed = true;
                                touched.push(pk);
                            }
                        }
                    }
                    pitch = tr.height() + 8.0;
                }
                k if k.starts_with("group") => {
                    in_group = true;
                    let title = k.trim_start_matches("group").trim();
                    p.text(pos2(content_left, ry + 10.0), Align2::LEFT_CENTER, title, look::bold(13.0), c(0xdedede));
                    pitch = 22.0;
                }
                "endgroup" => {
                    in_group = false;
                    pitch = 6.0;
                }
                _ => {
                    // text: a heading, help text or an info value
                    let (font, col) = match row.style {
                        Some("heading") => (look::bold(13.0), c(0xdedede)),
                        Some("small") => (look::regular(11.0), c(0xdfdfdf)),
                        _ => (look::regular(13.0), tcol),
                    };
                    let rr = if boxed { r.max.x - 43.0 } else { r.max.x - 20.0 };
                    let g = p.layout(text.to_string(), font, col, rr - x0);
                    let gh = g.size().y;
                    right = x0 + g.size().x;
                    p.galley(pos2(x0, ry + 12.0 - gh.min(16.0) / 2.0), g, col);
                    pitch = (gh + 8.0).max(22.0);
                }
            }
            let _ = in_group;
            last_right = right + 8.0;
            if !same_line {
                last_y = y;
                y += pitch;
            }
        }
        // Bramble's own settings at the end of View ▸ General and Audio ▸ General
        bramble_extra(app, ui, &p, &tab, &sub_name, content_left, label_right, y + 8.0);
        if changed {
            values.save();
            app.logic_settings = values;
            for k in touched {
                crate::prefs::changed(app, &k);
            }
        }
    });
    if !open {
        app.show_settings = false;
    }
}

fn bramble_extra_height(tab: &str, sub: &str) -> f32 {
    match (tab, sub) {
        ("View", "General") => 150.0,
        ("Audio", "General") => 100.0,
        _ => 0.0,
    }
}

#[allow(clippy::too_many_arguments)]
fn bramble_extra(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, tab: &str, sub: &str, left: f32, label_right: f32, mut y: f32) {
    let right_edge = left + W - 86.0;
    match (tab, sub) {
        ("View", "General") => {
            p.hline(left..=right_edge, y, Stroke::new(1.0, c(0x424140)));
            y += 10.0;
            p.text(pos2(left, y + 10.0), Align2::LEFT_CENTER, "Bramble", look::bold(13.0), c(0xdedede));
            y += 26.0;
            let ctrl = label_right + 9.5;
            let mut changed = false;
            p.text(pos2(label_right, y + 12.0), Align2::RIGHT_CENTER, "Appearance:", look::regular(13.0), c(0xdfdfdf));
            let r = Rect::from_min_size(pos2(ctrl, y), vec2(235.0, 24.0));
            let names = ["Dark", "Light"];
            let cur = if app.look.dark { 0 } else { 1 };
            if let Some(k) = popup(ui, p, r, ui.scope_id().with("bramble-dark"), cur, &names) {
                app.look.dark = k == 0;
                changed = true;
            }
            y += 31.0;
            p.text(pos2(label_right, y + 12.0), Align2::RIGHT_CENTER, "Colour Theme:", look::regular(13.0), c(0xdfdfdf));
            let r = Rect::from_min_size(pos2(ctrl, y), vec2(235.0, 24.0));
            let themes: Vec<&str> = kn_ui::theme::COLOR_THEMES.iter().map(|t| t.1).collect();
            let cur = kn_ui::theme::COLOR_THEMES.iter().position(|t| t.0 == app.look.color_theme).unwrap_or(0);
            if let Some(k) = popup(ui, p, r, ui.scope_id().with("bramble-theme"), cur, &themes) {
                app.look.color_theme = kn_ui::theme::COLOR_THEMES[k].0.to_string();
                changed = true;
            }
            y += 31.0;
            let b = Rect::from_min_size(pos2(ctrl, y + 4.0), vec2(300.0, 16.0));
            changed |= crate::widgets::check_slot(ui, Rect::from_min_size(b.min, vec2(20.0, 16.0)), ui.scope_id().with("bramble-glass"), &mut app.look.liquid_glass);
            p.text(pos2(ctrl + 26.0, y + 12.0), Align2::LEFT_CENTER, "Liquid glass", look::regular(13.0), c(0xdfdfdf));
            y += 22.0;
            changed |= crate::widgets::check_slot(ui, Rect::from_min_size(pos2(ctrl, y + 4.0), vec2(20.0, 16.0)), ui.scope_id().with("bramble-titlebar"), &mut app.look.system_title_bar);
            p.text(pos2(ctrl + 26.0, y + 12.0), Align2::LEFT_CENTER, "System title bar (after a restart)", look::regular(13.0), c(0xdfdfdf));
            if changed {
                app.look.apply(ui.ctx());
                app.look.save();
            }
        }
        ("Audio", "I/O Assignments") => crate::surround_ui::io_assignments(app, ui, left, label_right, y),
        ("Audio", "General") => {
            p.hline(left..=right_edge, y, Stroke::new(1.0, c(0x424140)));
            y += 10.0;
            p.text(pos2(left, y + 10.0), Align2::LEFT_CENTER, "Bramble", look::bold(13.0), c(0xdedede));
            y += 26.0;
            p.text(pos2(label_right, y + 12.0), Align2::RIGHT_CENTER, "SoundFonts:", look::regular(13.0), c(0xdfdfdf));
            let r = Rect::from_min_size(pos2(label_right + 9.5, y), vec2(309.0, 24.0));
            let names = ["Play natively (SoundFont engine)", "Convert into Sampler instruments"];
            let cur = if app.soundfont_mode == SoundFontMode::Native { 0 } else { 1 };
            if let Some(k) = popup(ui, p, r, ui.scope_id().with("bramble-sf"), cur, &names) {
                app.soundfont_mode = if k == 0 { SoundFontMode::Native } else { SoundFontMode::Sampler };
            }
        }
        _ => {}
    }
}

/// A pop-up as Logic's Settings draw it (filled, the value at the left, ⌃⌄ at the right).
fn popup(ui: &mut egui::Ui, p: &egui::Painter, pr: Rect, id: egui::Id, cur: usize, choices: &[&str]) -> Option<usize> {
    let resp = ui.interact(pr, id, Sense::click());
    let fill = c(0x3d3c3b);
    p.rect_filled(pr, 5.0, if resp.hovered() { look::mix(fill, Color32::WHITE, 0.06) } else { fill });
    p.text(pos2(pr.min.x + 12.0, pr.center().y), Align2::LEFT_CENTER, choices.get(cur).copied().unwrap_or(""), look::regular(13.0), c(0xdfdfdf));
    let m = pos2(pr.max.x - 14.0, pr.center().y);
    for s in [-1.0f32, 1.0] {
        p.line_segment([m + vec2(-3.5, s * 1.5), m + vec2(0.0, s * 5.0)], Stroke::new(1.3, c(0xe1e1e1)));
        p.line_segment([m + vec2(0.0, s * 5.0), m + vec2(3.5, s * 1.5)], Stroke::new(1.3, c(0xe1e1e1)));
    }
    let mut out = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(pr.width());
        for (i, ch) in choices.iter().enumerate() {
            if ui.add(egui::Button::selectable(i == cur, *ch)).clicked() {
                out = Some(i);
                ui.close();
            }
        }
    });
    out
}

/// The tab icons, as Logic's (gear, waveform, record, MIDI port, notes, film, the automation
/// curve, faders, eye, person).
fn tab_icon(p: &egui::Painter, m: egui::Pos2, i: usize, col: Color32) {
    use crate::glyphs::{br, ph};
    let icon = [ph::GEAR_SIX, ph::WAVEFORM, ph::RECORD, br::MIDI, ph::MUSIC_NOTES, ph::FILM_STRIP, ph::LINE_SEGMENTS, ph::FADERS, ph::EYE, ph::USER_CIRCLE];
    crate::glyphs::paint(p, Rect::from_center_size(m, vec2(26.0, 26.0)), icon[i.min(icon.len() - 1)], 24.0, col);
}
