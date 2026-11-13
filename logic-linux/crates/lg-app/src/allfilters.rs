//! The Browser's All Filters view as Logic 13's: each tag category in a rounded card (its name at
//! the top left, Show More / Show Less at the right; a click on the name folds the card, a folded
//! card shows a chevron), the tags as rounded pills in the category's colour that wrap to the
//! pane's width: Instruments and Properties magenta, Genre orange, Descriptors cyan (each pair
//! one joined pill), Sound Packs yellow, Favorites / Your Sounds plain with their icons. A selected
//! tag is filled; a selected main tag's subtags follow it as outlined pills (Show More shows every
//! tag's subtags and the tags past the first dozen); a group tag (Tonality ⌃⌄) holds its chosen
//! value as a filled inner pill. Sound Packs starts folded, as Logic's.

use egui::{pos2, vec2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::glyphs::{self, ph};
use crate::look;
use crate::soundlib::{self as sl, Item};

// (sizes in points, measured from the guide's All Filters figure at 2x)
const PAD: f32 = 8.0;
const CARD_GAP: f32 = 7.0;
const CARD_R: f32 = 10.0;
const HEAD_H: f32 = 29.0;
const PILL_H: f32 = 18.0;
const PILL_R: f32 = 5.0;
const PILL_PAD: f32 = 5.0;
const GAP_X: f32 = 5.0;
const GAP_Y: f32 = 6.0;
const FONT: f32 = 11.0;
/// Tags shown before Show More.
const FIRST: usize = 12;

/// A category's colour: the tag text and a selected tag's fill.
#[derive(Clone, Copy)]
struct Tint {
    text: Color32,
    fill: Color32,
}

fn dark() -> bool {
    !kn_ui::theme::is_light()
}

fn tint(text: u32, fill: u32, light_text: u32) -> Tint {
    Tint { text: kn_ui::theme::rgb(if dark() { text } else { light_text }), fill: kn_ui::theme::rgb(fill) }
}
fn magenta() -> Tint {
    tint(0xc52fda, 0x983cb9, 0xa61fbb)
}
fn orange() -> Tint {
    tint(0xe6832b, 0xb5682c, 0xc0661a)
}
fn cyan() -> Tint {
    tint(0x36bee5, 0x4299bd, 0x1d8fb3)
}
fn yellow() -> Tint {
    tint(0xd9b532, 0x9c8530, 0x9a7a08)
}
fn plain() -> Tint {
    Tint { text: look::text(), fill: if dark() { kn_ui::theme::rgb(0x5c5d61) } else { kn_ui::theme::rgb(0xb4b4ba) } }
}
fn card_fill() -> Color32 {
    kn_ui::theme::rgb(if dark() { 0x3a3c3f } else { 0xffffff })
}
fn pill_fill() -> Color32 {
    kn_ui::theme::rgb(if dark() { 0x232527 } else { 0xececef })
}
fn sub_fill() -> Color32 {
    kn_ui::theme::rgb(if dark() { 0x2c3032 } else { 0xf6f6f8 })
}
fn sub_rim() -> Color32 {
    kn_ui::theme::rgb(if dark() { 0x17191a } else { 0xc8c8cc })
}
fn head_text() -> Color32 {
    kn_ui::theme::rgb(if dark() { 0xebebeb } else { 0x1d1d1f })
}
fn dim_text() -> Color32 {
    kn_ui::theme::rgb(if dark() { 0x7c7c7c } else { 0x8a8a8e })
}

/// What a click on a pill does.
#[derive(Clone, PartialEq, Debug)]
enum Act {
    Tag(String),
    Pack(String),
    YourSounds,
    Favorites,
}

/// One pill in a card's flow.
enum Pill {
    /// A tag (`sub`: a subtag, outlined), with an optional icon before its name.
    Tag { text: String, sel: bool, sub: bool, icon: Option<&'static str>, act: Act },
    /// Two exclusive descriptors as one joined pill.
    Pair { a: String, b: String, sa: bool, sb: bool },
    /// A group tag and its chosen value.
    Group { name: String, cur: Option<String>, vals: Vec<(String, usize)> },
}

/// Lays out items `widths` wide in rows `avail` wide, `gap` apart: each item's (x, row). An item
/// wider than the row gets a row of its own.
pub(crate) fn flow(widths: &[f32], avail: f32, gap: f32) -> Vec<(f32, usize)> {
    let mut out = Vec::with_capacity(widths.len());
    let (mut x, mut row) = (0.0f32, 0usize);
    for (i, &w) in widths.iter().enumerate() {
        if i > 0 && x > 0.0 && x + w > avail + 0.01 {
            x = 0.0;
            row += 1;
        }
        out.push((x, row));
        x += w + gap;
    }
    out
}

fn font() -> egui::FontId {
    look::semibold(FONT)
}

/// A one-line galley, cut with "…" when wider than `max`.
fn text(ui: &egui::Ui, s: &str, color: Color32, max: f32, font: egui::FontId) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(s.to_string(), font, color);
    job.wrap = egui::text::TextWrapping { max_width: max.max(10.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.painter().layout_job(job)
}

fn pill_width(ui: &egui::Ui, p: &Pill, max: f32) -> f32 {
    let w = |s: &str| text(ui, s, Color32::WHITE, max, font()).size().x;
    let v = match p {
        Pill::Tag { text: t, icon, .. } => w(t) + 2.0 * PILL_PAD + if icon.is_some() { 15.0 } else { 0.0 },
        Pill::Pair { a, b, .. } => w(a) + w(b) + 4.0 * PILL_PAD + 1.0,
        Pill::Group { name, cur, .. } => match cur {
            Some(v) => w(name) + 3.0 * PILL_PAD + 2.0 + w(v) + 2.0 * PILL_PAD - 2.0,
            None => w(name) + 2.0 * PILL_PAD + 13.0,
        },
    };
    v.min(max)
}

/// Draws a card's pills at the top left `at`, `avail` wide: their height, and what was clicked.
fn draw_pills(ui: &mut egui::Ui, id: &str, at: egui::Pos2, avail: f32, pills: &[Pill], t: Tint, out: &mut Option<Act>, group: &mut Option<(String, String)>) -> f32 {
    if pills.is_empty() {
        return 0.0;
    }
    let widths: Vec<f32> = pills.iter().map(|p| pill_width(ui, p, avail)).collect();
    let pos = flow(&widths, avail, GAP_X);
    let rows = pos.last().map_or(0, |p| p.1 + 1);
    let p = ui.painter().clone();
    for (i, (pill, ((x, row), w))) in pills.iter().zip(pos.into_iter().zip(widths)).enumerate() {
        let r = Rect::from_min_size(at + vec2(x, row as f32 * (PILL_H + GAP_Y)), vec2(w, PILL_H));
        let hover_fill = |f: Color32, h: bool| if h { look::mix(f, Color32::from_gray(if dark() { 0x60 } else { 0xb0 }), 0.25) } else { f };
        match pill {
            Pill::Tag { text: s, sel, sub, icon, act } => {
                let resp = ui.interact(r, ui.scope_id().with((id, i)), Sense::click());
                let fill = if *sel { t.fill } else if *sub { sub_fill() } else { pill_fill() };
                p.rect_filled(r, PILL_R, hover_fill(fill, resp.hovered() && !sel));
                if *sub && !sel {
                    p.rect_stroke(r.shrink(0.5), PILL_R - 0.5, Stroke::new(1.0, sub_rim()), egui::StrokeKind::Inside);
                }
                let col = if *sel { Color32::WHITE } else { t.text };
                let mut tx = r.min.x + PILL_PAD;
                if let Some(ic) = icon {
                    glyphs::paint(&p, Rect::from_center_size(pos2(tx + 5.5, r.center().y), vec2(11.0, 11.0)), ic, 11.0, col);
                    tx += 15.0;
                }
                let g = text(ui, s, col, r.max.x - PILL_PAD - tx, font());
                p.galley(pos2(tx, r.center().y - g.size().y / 2.0), g, col);
                if resp.clicked() {
                    *out = Some(act.clone());
                }
            }
            Pill::Pair { a, b, sa, sb } => {
                let wa = text(ui, a, Color32::WHITE, avail, font()).size().x + 2.0 * PILL_PAD;
                let ra = Rect::from_min_max(r.min, pos2((r.min.x + wa).min(r.max.x), r.max.y));
                let rb = Rect::from_min_max(pos2(ra.max.x + 1.0, r.min.y), r.max);
                for (half, s, sel, round) in [(ra, a, *sa, egui::CornerRadius { nw: 5, sw: 5, ne: 0, se: 0 }), (rb, b, *sb, egui::CornerRadius { nw: 0, sw: 0, ne: 5, se: 5 })] {
                    let resp = ui.interact(half, ui.scope_id().with((id, i, s.as_str())), Sense::click());
                    p.rect_filled(half, round, hover_fill(if sel { t.fill } else { pill_fill() }, resp.hovered() && !sel));
                    let col = if sel { Color32::WHITE } else { t.text };
                    let g = text(ui, s, col, half.width() - 2.0 * PILL_PAD, font());
                    p.galley(pos2(half.min.x + PILL_PAD, half.center().y - g.size().y / 2.0), g, col);
                    if resp.clicked() {
                        *out = Some(Act::Tag(s.clone()));
                    }
                }
            }
            Pill::Group { name, cur, vals } => {
                let resp = ui.interact(r, ui.scope_id().with((id, i)), Sense::click());
                p.rect_filled(r, PILL_R, hover_fill(pill_fill(), resp.hovered()));
                let g = text(ui, name, t.text, r.width() - 2.0 * PILL_PAD, font());
                let gw = g.size().x;
                p.galley(pos2(r.min.x + PILL_PAD, r.center().y - g.size().y / 2.0), g, t.text);
                match cur {
                    Some(v) => {
                        // the value: a filled pill inside, at the right
                        let inner = Rect::from_min_max(pos2(r.min.x + 2.0 * PILL_PAD + gw - 2.0, r.min.y + 2.0), pos2(r.max.x - 2.0, r.max.y - 2.0));
                        p.rect_filled(inner, PILL_R - 1.5, t.fill);
                        let g = text(ui, v, Color32::WHITE, inner.width() - 2.0 * PILL_PAD + 4.0, font());
                        p.galley(pos2(inner.min.x + PILL_PAD - 2.0, inner.center().y - g.size().y / 2.0), g, Color32::WHITE);
                    }
                    None => {
                        glyphs::paint(&p, Rect::from_center_size(pos2(r.max.x - PILL_PAD - 4.5, r.center().y), vec2(9.0, 11.0)), ph::CARET_UP_DOWN, 11.0, t.text);
                    }
                }
                egui::Popup::menu(&resp).show(|ui| {
                    for (v, c) in vals {
                        let on = cur.as_deref() == Some(v.as_str());
                        if ui.add_enabled(*c > 0 || on, egui::Button::selectable(on, format!("{v}  ({c})"))).clicked() {
                            *group = Some((name.clone(), v.clone()));
                            ui.close();
                        }
                    }
                });
            }
        }
    }
    rows as f32 * PILL_H + (rows.saturating_sub(1)) as f32 * GAP_Y
}

/// One category card: its header (name; Show More / Show Less, or a chevron when folded) and
/// pills. Returns (folded toggled, show more toggled).
#[allow(clippy::too_many_arguments)]
fn card(ui: &mut egui::Ui, name: &str, open: bool, more: Option<bool>, pills: &[Pill], t: Tint, out: &mut Option<Act>, group: &mut Option<(String, String)>) -> (bool, bool) {
    let w = ui.available_width();
    let top = ui.cursor().min;
    let inner_w = w - 2.0 * PAD;
    // measure the pills first (drawn below once the card's background is down)
    let h = if open && !pills.is_empty() {
        let widths: Vec<f32> = pills.iter().map(|p| pill_width(ui, p, inner_w)).collect();
        let rows = flow(&widths, inner_w, GAP_X).last().map_or(0, |p| p.1 + 1);
        HEAD_H + rows as f32 * PILL_H + rows.saturating_sub(1) as f32 * GAP_Y + PAD
    } else {
        HEAD_H
    };
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, CARD_R, card_fill());
    let head = Rect::from_min_size(rect.min, vec2(w, HEAD_H));
    let cy = top.y + HEAD_H / 2.0;
    // the name (a click folds or unfolds the card)
    let g = text(ui, name, head_text(), inner_w * 0.6, look::semibold(12.0));
    let name_r = Rect::from_min_size(pos2(rect.min.x + PAD, cy - g.size().y / 2.0), g.size());
    p.galley(name_r.min, g, head_text());
    let mut fold = false;
    let mut more_t = false;
    if open {
        if let Some(m) = more {
            let label = if m { "Show Less" } else { "Show More" };
            let g = text(ui, label, dim_text(), inner_w * 0.4, look::semibold(11.0));
            let mr = Rect::from_min_size(pos2(rect.max.x - PAD - g.size().x, cy - g.size().y / 2.0), g.size());
            let resp = ui.interact(mr.expand(3.0), ui.scope_id().with((name, "more")), Sense::click());
            p.galley(mr.min, g, if resp.hovered() { head_text() } else { dim_text() });
            more_t = resp.clicked();
        }
        let hr = Rect::from_min_max(head.min, pos2(name_r.max.x + PAD, head.max.y));
        fold = ui.interact(hr, ui.scope_id().with((name, "head")), Sense::click()).clicked();
    } else {
        glyphs::paint(&p, Rect::from_center_size(pos2(rect.max.x - PAD - 5.0, cy), vec2(10.0, 12.0)), ph::CARET_RIGHT, 12.0, dim_text());
        fold = ui.interact(head, ui.scope_id().with((name, "head")), Sense::click()).clicked();
    }
    if open {
        draw_pills(ui, name, pos2(rect.min.x + PAD, rect.min.y + HEAD_H), inner_w, pills, t, out, group);
    }
    ui.add_space(CARD_GAP);
    (fold, more_t)
}

fn is_open(app: &App, name: &str) -> bool {
    // (Sound Packs starts folded, as Logic's)
    if name == "Sound Packs" { app.br.expanded.contains(&format!("open:{name}")) } else { !app.br.expanded.contains(&format!("closed:{name}")) }
}

fn set_open(app: &mut App, name: &str, open: bool) {
    let (on, off) = if name == "Sound Packs" { (format!("open:{name}"), String::new()) } else { (String::new(), format!("closed:{name}")) };
    app.br.expanded.remove(&on);
    app.br.expanded.remove(&off);
    if open && !on.is_empty() {
        app.br.expanded.insert(on);
    } else if !open && !off.is_empty() {
        app.br.expanded.insert(off);
    }
}

/// The All Filters view in the Browser's results area `list`; returns the number of matches.
pub fn show(app: &mut App, ui: &mut egui::Ui, list: Rect, base: Vec<Item>) -> usize {
    let counts = sl::tag_counts(base.iter());
    let n = base.len();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list.shrink2(vec2(8.0, 8.0))).layout(egui::Layout::top_down(egui::Align::Min)));
    let mut act: Option<Act> = None;
    let mut group: Option<(String, String)> = None;
    let mut fold: Option<String> = None;
    let mut more: Option<String> = None;
    let s = app.br.search.clone();
    let count = |k: &str| counts.get(k).copied().unwrap_or(0);
    egui::ScrollArea::vertical().id_salt("all-filters").auto_shrink([false, false]).show(&mut child, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        // Favorites (once there are some) and Your Sounds
        {
            let mut pills = Vec::new();
            if !app.br.saved.favorites.is_empty() {
                pills.push(Pill::Tag { text: "Favorites".into(), sel: s.favorites, sub: false, icon: Some(ph::HEART), act: Act::Favorites });
            }
            pills.push(Pill::Tag { text: "Your Sounds".into(), sel: s.your_sounds, sub: false, icon: Some(ph::USER), act: Act::YourSounds });
            let w = ui.available_width();
            let widths: Vec<f32> = pills.iter().map(|p| pill_width(ui, p, w - 2.0 * PAD)).collect();
            let rows = flow(&widths, w - 2.0 * PAD, GAP_X).last().map_or(1, |p| p.1 + 1);
            let h = 2.0 * PAD + rows as f32 * PILL_H + (rows - 1) as f32 * GAP_Y;
            let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
            ui.painter().rect_filled(rect, CARD_R, card_fill());
            draw_pills(ui, "mine", rect.min + vec2(PAD, PAD), w - 2.0 * PAD, &pills, plain(), &mut act, &mut group);
            ui.add_space(CARD_GAP);
        }
        // Instruments, Properties, Genre, Descriptors, Sound Packs (Logic's order)
        let cats = sl::categories();
        let inst = cats.iter().find(|c| c.name == "Instrument");
        let genre = cats.iter().find(|c| c.name == "Genre");
        let tag_card = |ui: &mut egui::Ui, title: &str, cat: &sl::Category, t: Tint, act: &mut Option<Act>, group: &mut Option<(String, String)>| -> (bool, bool) {
            let open = is_open(app, title);
            let all = app.br.expanded.contains(cat.name);
            let mut tags: Vec<(&str, &[&str])> = cat.tags.to_vec();
            // (selected tags first; the rest only those with matches)
            tags.sort_by_key(|(t, _)| !s.has_tag(t));
            let mut pills = Vec::new();
            let mut shown = 0;
            let mut hidden = false;
            for (tag, subs) in tags {
                let sel = s.has_tag(tag);
                if !sel && count(tag) == 0 {
                    continue;
                }
                shown += 1;
                if !all && shown > FIRST && !sel {
                    hidden = true;
                    continue;
                }
                pills.push(Pill::Tag { text: tag.to_string(), sel, sub: false, icon: None, act: Act::Tag(tag.to_string()) });
                // subtags: after their main tag when it is selected, or after Show More
                if sel || all {
                    for st in subs.iter().filter(|st| s.has_tag(st) || count(st) > 0) {
                        pills.push(Pill::Tag { text: st.to_string(), sel: s.has_tag(st), sub: true, icon: None, act: Act::Tag(st.to_string()) });
                    }
                }
            }
            let has_subs = cat.tags.iter().any(|(_, subs)| subs.iter().any(|st| count(st) > 0));
            let more = (all || hidden || has_subs).then_some(all);
            card(ui, title, open, more, &pills, t, act, group)
        };
        if let Some(c) = inst {
            let (f, m) = tag_card(ui, "Instruments", c, magenta(), &mut act, &mut group);
            if f {
                fold = Some("Instruments".into());
            }
            if m {
                more = Some(c.name.into());
            }
        }
        // Properties: the group tags, each with its value menu
        {
            let pills: Vec<Pill> = sl::GROUPS
                .iter()
                .filter_map(|(g, vals)| {
                    let cur = s.groups.iter().find(|x| x.0 == *g).map(|x| x.1.clone());
                    let vals: Vec<(String, usize)> = vals.iter().map(|v| (v.to_string(), count(&format!("{g}\u{1f}{v}")))).collect();
                    (cur.is_some() || vals.iter().any(|v| v.1 > 0)).then(|| Pill::Group { name: g.to_string(), cur, vals })
                })
                .collect();
            if !pills.is_empty() {
                let (f, _) = card(ui, "Properties", is_open(app, "Properties"), None, &pills, magenta(), &mut act, &mut group);
                if f {
                    fold = Some("Properties".into());
                }
            }
        }
        if let Some(c) = genre {
            let (f, m) = tag_card(ui, "Genre", c, orange(), &mut act, &mut group);
            if f {
                fold = Some("Genre".into());
            }
            if m {
                more = Some(c.name.into());
            }
        }
        {
            let pills: Vec<Pill> = sl::PAIRS
                .iter()
                .filter(|(a, b)| count(a) + count(b) > 0 || s.has_tag(a) || s.has_tag(b))
                .map(|(a, b)| Pill::Pair { a: a.to_string(), b: b.to_string(), sa: s.has_tag(a), sb: s.has_tag(b) })
                .collect();
            if !pills.is_empty() {
                let (f, _) = card(ui, "Descriptors", is_open(app, "Descriptors"), None, &pills, cyan(), &mut act, &mut group);
                if f {
                    fold = Some("Descriptors".into());
                }
            }
        }
        {
            let mut packs: Vec<String> = counts.keys().filter_map(|k| k.strip_prefix("pack\u{1f}").map(str::to_string)).collect();
            packs.sort();
            let pills: Vec<Pill> = packs.into_iter().map(|p| Pill::Tag { text: p.clone(), sel: s.packs.contains(&p), sub: false, icon: None, act: Act::Pack(p) }).collect();
            if !pills.is_empty() {
                let (f, _) = card(ui, "Sound Packs", is_open(app, "Sound Packs"), None, &pills, yellow(), &mut act, &mut group);
                if f {
                    fold = Some("Sound Packs".into());
                }
            }
        }
    });
    if let Some(name) = fold {
        let open = is_open(app, &name);
        set_open(app, &name, !open);
    }
    if let Some(c) = more {
        if !app.br.expanded.remove(&c) {
            app.br.expanded.insert(c);
        }
    }
    match act {
        Some(Act::Tag(t)) => app.br.search.toggle_tag(&t),
        Some(Act::Pack(p)) => {
            if let Some(i) = app.br.search.packs.iter().position(|x| *x == p) {
                app.br.search.packs.remove(i);
            } else {
                app.br.search.packs.push(p);
            }
        }
        Some(Act::YourSounds) => app.br.search.your_sounds = !app.br.search.your_sounds,
        Some(Act::Favorites) => app.br.search.favorites = !app.br.search.favorites,
        None => {}
    }
    if let Some((g, v)) = group {
        app.br.search.set_group(&g, &v);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::flow;

    #[test]
    fn pills_wrap_to_the_width() {
        // three 40-wide pills in 100: two on the first row, the third on the next
        assert_eq!(flow(&[40.0, 40.0, 40.0], 100.0, 5.0), vec![(0.0, 0), (45.0, 0), (0.0, 1)]);
        // exactly filling a row still fits
        assert_eq!(flow(&[45.0, 50.0], 100.0, 5.0), vec![(0.0, 0), (50.0, 0)]);
        // a pill wider than the row gets one of its own, and the next starts a new row
        assert_eq!(flow(&[30.0, 150.0, 20.0], 100.0, 5.0), vec![(0.0, 0), (0.0, 1), (0.0, 2)]);
        assert!(flow(&[], 100.0, 5.0).is_empty());
    }
}
