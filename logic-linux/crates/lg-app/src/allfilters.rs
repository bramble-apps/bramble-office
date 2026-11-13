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

// (sizes in Logic's points, measured from the guide's All Filters figure at 2x, times K)
/// Studio's UI is drawn larger than Logic's: its Browser filter button is 33 × 24 pt where
/// Logic's is 27 × 18.5, its list text has a 9.5 pt cap height where Logic's text beside the
/// window has 8. The window scales by the same ratio so it sits with the UI around it as
/// Logic's does.
pub(crate) const K: f32 = 1.25;
const PAD: f32 = 8.0 * K;
const CARD_GAP: f32 = 7.0 * K;
const CARD_R: f32 = 10.0 * K;
const HEAD_H: f32 = 29.0 * K;
const PILL_H: f32 = 18.0 * K;
const PILL_R: f32 = 5.0 * K;
const PILL_PAD: f32 = 5.0 * K;
const GAP_X: f32 = 5.0 * K;
const GAP_Y: f32 = 6.0 * K;
const FONT: f32 = 10.0 * K;

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
fn blue() -> Tint {
    tint(0x6d7cff, 0x6670cd, 0x4a55e0)
}
fn green() -> Tint {
    tint(0x63c75a, 0x4f8f48, 0x3a8f33)
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
    Type(String),
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
        Pill::Tag { text: t, icon, .. } => w(t) + 2.0 * PILL_PAD + if icon.is_some() { 15.0 * K } else { 0.0 },
        Pill::Pair { a, b, .. } => w(a) + w(b) + 4.0 * PILL_PAD + K,
        Pill::Group { name, cur, .. } => match cur {
            Some(v) => w(name) + 5.0 * PILL_PAD + w(v),
            None => w(name) + 2.0 * PILL_PAD + 13.0 * K,
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
                    p.rect_stroke(r.shrink(0.5 * K), PILL_R - 0.5 * K, Stroke::new(K, sub_rim()), egui::StrokeKind::Inside);
                }
                let col = if *sel { Color32::WHITE } else { t.text };
                let mut tx = r.min.x + PILL_PAD;
                if let Some(ic) = icon {
                    glyphs::paint(&p, Rect::from_center_size(pos2(tx + 5.5 * K, r.center().y), vec2(11.0 * K, 11.0 * K)), ic, 11.0 * K, col);
                    tx += 15.0 * K;
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
                let rb = Rect::from_min_max(pos2(ra.max.x + K, r.min.y), r.max);
                let cr = PILL_R.round() as u8;
                for (half, s, sel, round) in [(ra, a, *sa, egui::CornerRadius { nw: cr, sw: cr, ne: 0, se: 0 }), (rb, b, *sb, egui::CornerRadius { nw: 0, sw: 0, ne: cr, se: cr })] {
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
                        let inner = Rect::from_min_max(pos2(r.min.x + 2.0 * PILL_PAD + gw - 2.0 * K, r.min.y + 2.0 * K), pos2(r.max.x - 2.0 * K, r.max.y - 2.0 * K));
                        p.rect_filled(inner, PILL_R - 1.5 * K, t.fill);
                        let g = text(ui, v, Color32::WHITE, inner.width() - 2.0 * PILL_PAD + 4.0 * K, font());
                        p.galley(pos2(inner.min.x + PILL_PAD - 2.0 * K, inner.center().y - g.size().y / 2.0), g, Color32::WHITE);
                    }
                    None => {
                        glyphs::paint(&p, Rect::from_center_size(pos2(r.max.x - PILL_PAD - 4.5 * K, r.center().y), vec2(9.0 * K, 11.0 * K)), ph::CARET_UP_DOWN, 11.0 * K, t.text);
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
    // (an open card with nothing to show is left out, as Logic leaves out empty categories)
    if open && pills.is_empty() {
        return (false, false);
    }
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
    let g = text(ui, name, head_text(), inner_w * 0.6, look::semibold(10.0 * K));
    let name_r = Rect::from_min_size(pos2(rect.min.x + PAD, cy - g.size().y / 2.0), g.size());
    p.galley(name_r.min, g, head_text());
    let fold;
    let mut more_t = false;
    if open {
        if let Some(m) = more {
            let label = if m { "Show Less" } else { "Show More" };
            let g = text(ui, label, dim_text(), inner_w * 0.4, look::semibold(8.6 * K));
            let mr = Rect::from_min_size(pos2(rect.max.x - PAD - g.size().x, cy - g.size().y / 2.0), g.size());
            let resp = ui.interact(mr.expand(3.0 * K), ui.scope_id().with((name, "more")), Sense::click());
            p.galley(mr.min, g, if resp.hovered() { head_text() } else { dim_text() });
            more_t = resp.clicked();
        }
        let hr = Rect::from_min_max(head.min, pos2(name_r.max.x + PAD, head.max.y));
        fold = ui.interact(hr, ui.scope_id().with((name, "head")), Sense::click()).clicked();
    } else {
        glyphs::paint(&p, Rect::from_center_size(pos2(rect.max.x - PAD - 5.0 * K, cy), vec2(10.0 * K, 12.0 * K)), ph::CARET_RIGHT, 12.0 * K, dim_text());
        fold = ui.interact(head, ui.scope_id().with((name, "head")), Sense::click()).clicked();
    }
    if open {
        draw_pills(ui, name, pos2(rect.min.x + PAD, rect.min.y + HEAD_H), inner_w, pills, t, out, group);
    }
    ui.add_space(CARD_GAP);
    (fold, more_t)
}

/// Cards that start folded (a chevron at the right), as Logic's.
const FOLDED: &[&str] = &["Sound Packs", "Effects"];

fn is_open(app: &App, name: &str) -> bool {
    if FOLDED.contains(&name) { app.br.expanded.contains(&format!("open:{name}")) } else { !app.br.expanded.contains(&format!("closed:{name}")) }
}

fn set_open(app: &mut App, name: &str, open: bool) {
    for k in [format!("open:{name}"), format!("closed:{name}")] {
        app.br.expanded.remove(&k);
    }
    let folded = FOLDED.contains(&name);
    if open && folded {
        app.br.expanded.insert(format!("open:{name}"));
    } else if !open && !folded {
        app.br.expanded.insert(format!("closed:{name}"));
    }
}

/// Notes a card's header clicks: (folded toggled, Show More toggled).
fn note(fold: &mut Option<String>, more: &mut Option<String>, title: &str, (f, m): (bool, bool)) {
    if f {
        *fold = Some(title.to_string());
    }
    if m {
        *more = Some(title.to_string());
    }
}

/// Show More's key for a card.
fn more_key(name: &str) -> String {
    format!("more:{name}")
}

/// An effect plug-in's category in Logic's Audio FX menu ("Dynamics", "Reverb"…).
pub(crate) fn fx_category(name: &str) -> Option<&'static str> {
    fn has(items: &[crate::menus::Item], name: &str) -> bool {
        items.iter().any(|i| i.label == name || has(&i.children, name))
    }
    crate::menus::context("mixer-audiofx-slot").iter().filter(|c| !c.children.is_empty() && c.label != "Audio Units").find(|c| has(&c.children, name)).map(|c| c.label.as_str())
}

/// The effect categories, in the Audio FX menu's order.
fn fx_categories() -> Vec<&'static str> {
    crate::menus::context("mixer-audiofx-slot").iter().filter(|c| !c.children.is_empty() && c.label != "Audio Units").map(|c| c.label.as_str()).collect()
}

/// The cards: Favorites / Your Sounds, Type, Instruments, Properties, Genre, Descriptors, Sound
/// Packs and Effects (Logic's order).
fn content(app: &mut App, ui: &mut egui::Ui, base: &[Item]) {
    let counts = sl::tag_counts(base.iter());
    let mut act: Option<Act> = None;
    let mut group: Option<(String, String)> = None;
    let mut fold: Option<String> = None;
    let mut more: Option<String> = None;
    let s = app.br.search.clone();
    let count = |k: &str| counts.get(k).copied().unwrap_or(0);
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
    // a card of plain tags: selected first, the rest those with matches, `first` of them before
    // Show More (which also shows the subtags)
    let tag_card = |ui: &mut egui::Ui, title: &str, tags: Vec<(String, Vec<String>)>, first: usize, t: Tint, mk: &dyn Fn(&str) -> Act, sel_of: &dyn Fn(&str) -> bool, act: &mut Option<Act>, group: &mut Option<(String, String)>| -> (bool, bool) {
        let open = is_open(app, title);
        let all = app.br.expanded.contains(&more_key(title));
        let mut tags = tags;
        tags.sort_by_key(|(t, _)| !sel_of(t));
        let mut pills = Vec::new();
        let mut shown = 0;
        for (tag, subs) in tags {
            let sel = sel_of(&tag);
            if !sel && count(&tag) == 0 {
                continue;
            }
            shown += 1;
            if !all && shown > first && !sel {
                continue;
            }
            pills.push(Pill::Tag { text: tag.clone(), sel, sub: false, icon: None, act: mk(&tag) });
            // subtags: after their main tag when it is selected, or after Show More
            if sel || all {
                for st in subs.iter().filter(|st| s.has_tag(st) || count(st) > 0) {
                    pills.push(Pill::Tag { text: st.clone(), sel: s.has_tag(st), sub: true, icon: None, act: Act::Tag(st.clone()) });
                }
            }
        }
        card(ui, title, open, Some(all), &pills, t, act, group)
    };
    let owned = |tags: &[(&str, &[&str])]| tags.iter().map(|(t, subs)| (t.to_string(), subs.iter().map(|x| x.to_string()).collect())).collect::<Vec<(String, Vec<String>)>>();
    let has = |t: &str| s.has_tag(t);
    // Type: by what each sound is (soundlib::Item::type_tag); counted as "type␟Audio Loop"
    {
        let types: Vec<(String, Vec<String>)> = sl::TYPES.iter().map(|t| (t.to_string(), Vec::new())).collect();
        let type_counts: std::collections::HashMap<String, usize> = sl::TYPES.iter().map(|t| (t.to_string(), count(&format!("type\u{1f}{t}")))).collect();
        // (counted under their own key: draw with a count closure of their own)
        let open = is_open(app, "Type");
        let all = app.br.expanded.contains(&more_key("Type"));
        let pills: Vec<Pill> = types
            .into_iter()
            .filter(|(t, _)| s.types.contains(t) || type_counts.get(t).copied().unwrap_or(0) > 0)
            .map(|(t, _)| Pill::Tag { sel: s.types.contains(&t), act: Act::Type(t.clone()), text: t, sub: false, icon: None })
            .collect();
        note(&mut fold, &mut more, "Type", card(ui, "Type", open, Some(all), &pills, blue(), &mut act, &mut group));
    }
    let cats = sl::categories();
    if let Some(c) = cats.iter().find(|c| c.name == "Instrument") {
        note(&mut fold, &mut more, "Instruments", tag_card(ui, "Instruments", owned(c.tags), 17, magenta(), &|t| Act::Tag(t.to_string()), &has, &mut act, &mut group));
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
        let all = app.br.expanded.contains(&more_key("Properties"));
        {
        note(&mut fold, &mut more, "Properties", card(ui, "Properties", is_open(app, "Properties"), Some(all), &pills, magenta(), &mut act, &mut group));
        }
    }
    if let Some(c) = cats.iter().find(|c| c.name == "Genre") {
        note(&mut fold, &mut more, "Genre", tag_card(ui, "Genre", owned(c.tags), 12, orange(), &|t| Act::Tag(t.to_string()), &has, &mut act, &mut group));
    }
    // Descriptors: the pairs (four before Show More, as Logic's)
    {
        let all = app.br.expanded.contains(&more_key("Descriptors"));
        let pills: Vec<Pill> = sl::PAIRS
            .iter()
            .filter(|(a, b)| count(a) + count(b) > 0 || s.has_tag(a) || s.has_tag(b))
            .enumerate()
            .filter(|(i, (a, b))| all || *i < 4 || s.has_tag(a) || s.has_tag(b))
            .map(|(_, (a, b))| Pill::Pair { a: a.to_string(), b: b.to_string(), sa: s.has_tag(a), sb: s.has_tag(b) })
            .collect();
        note(&mut fold, &mut more, "Descriptors", card(ui, "Descriptors", is_open(app, "Descriptors"), Some(all), &pills, cyan(), &mut act, &mut group));
    }
    // Sound Packs (folded at first)
    {
        let mut packs: Vec<String> = counts.keys().filter_map(|k| k.strip_prefix("pack\u{1f}").map(str::to_string)).collect();
        packs.sort();
        let pills: Vec<Pill> = packs.into_iter().map(|p| Pill::Tag { text: p.clone(), sel: s.packs.contains(&p), sub: false, icon: None, act: Act::Pack(p) }).collect();
        let all = app.br.expanded.contains(&more_key("Sound Packs"));
        note(&mut fold, &mut more, "Sound Packs", card(ui, "Sound Packs", is_open(app, "Sound Packs"), Some(all), &pills, yellow(), &mut act, &mut group));
    }
    // Effects: the Audio FX menu's categories (folded at first)
    {
        let tags: Vec<(String, Vec<String>)> = fx_categories().into_iter().map(|c| (c.to_string(), Vec::new())).collect();
        note(&mut fold, &mut more, "Effects", tag_card(ui, "Effects", tags, usize::MAX, green(), &|t| Act::Tag(t.to_string()), &has, &mut act, &mut group));
    }
    if let Some(name) = fold {
        let open = is_open(app, &name);
        set_open(app, &name, !open);
    }
    if let Some(c) = more {
        let k = more_key(&c);
        if !app.br.expanded.remove(&k) {
            app.br.expanded.insert(k);
        }
    }
    match act {
        Some(Act::Tag(t)) => app.br.search.toggle_tag(&t),
        Some(Act::Type(t)) => {
            if let Some(i) = app.br.search.types.iter().position(|x| *x == t) {
                app.br.search.types.remove(i);
            } else {
                app.br.search.types.push(t);
            }
        }
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
}

// ---- the window (sizes in points from the guide's figure at 2x)
const WIN_W: f32 = 397.0 * K;
const WIN_R: f32 = 13.0 * K;
/// The arrow's tip sits this far right of the filter button, the window this far.
const TIP_GAP: f32 = 2.0 * K;
const ARROW_W: f32 = 9.0 * K;
const ARROW_H: f32 = 16.0 * K;
/// The arrow's middle below the window's top.
const ARROW_Y: f32 = 107.5 * K;
const TITLE_CY: f32 = 16.0 * K;
const BODY_TOP: f32 = 32.0 * K;
const INSET: f32 = 7.0 * K;

fn win_fill() -> Color32 {
    if dark() { Color32::from_rgba_unmultiplied(0x2f, 0x30, 0x31, 246) } else { Color32::from_rgba_unmultiplied(0xf2, 0xf2, 0xf4, 246) }
}

/// The glass's blue glow (the figure's backdrop: strongest left of the middle, fading out).
fn glow(p: &egui::Painter, win: Rect) {
    let c = pos2(win.min.x + 57.0 * K, win.min.y + 270.0 * K);
    let (rx, ry) = (220.0 * K, 170.0 * K);
    let inner = if dark() { Color32::from_rgba_unmultiplied(0x12, 0x40, 0x5e, 235) } else { Color32::from_rgba_unmultiplied(0x9c, 0xc4, 0xe6, 90) };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(c, inner);
    let n = 48;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(c + vec2(a.cos() * rx, a.sin() * ry), Color32::TRANSPARENT);
    }
    for i in 0..n as u32 {
        mesh.add_triangle(0, i + 1, i + 2);
    }
    p.with_clip_rect(win.shrink2(vec2(0.0, WIN_R))).add(egui::Shape::mesh(mesh));
}

/// The All Filters window floating beside the Browser's filter button `anchor`, over the
/// Browser and the Tracks area. The ×, Escape or the filter button again close it.
pub fn window(app: &mut App, ctx: &egui::Context, anchor: Rect, base: Vec<Item>) {
    let screen = ctx.content_rect();
    let left = anchor.max.x + TIP_GAP + ARROW_W;
    let top = (anchor.center().y - ARROW_Y).max(screen.min.y + 4.0);
    let h = (screen.max.y - 10.0 - top).clamp(200.0, 640.0 * K);
    let x = left.min(screen.max.x - WIN_W - 4.0);
    let win = Rect::from_min_size(pos2(x, top), vec2(WIN_W, h));
    let close_id = egui::Id::unique("all-filters-close");
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        app.br.all_filters = false;
        return;
    }
    let first = ctx.data_mut(|d| {
        let was = d.get_temp::<bool>(egui::Id::unique("all-filters-was-open")).unwrap_or(false);
        d.insert_temp(egui::Id::unique("all-filters-was-open"), true);
        !was
    });
    egui::Area::new(egui::Id::unique("all-filters-window")).order(egui::Order::Foreground).fixed_pos(win.min).show(ctx, |ui| {
        let p = ui.painter().clone();
        // shadow, glass, arrow
        p.add(egui::Shadow { offset: [0, (10.0 * K) as i8], blur: (34.0 * K) as u8, spread: 0, color: Color32::from_black_alpha(if dark() { 120 } else { 50 }) }.as_shape(win, WIN_R));
        p.rect_filled(win, WIN_R, win_fill());
        let ay = anchor.center().y.clamp(win.min.y + WIN_R + ARROW_H / 2.0, win.max.y - WIN_R - ARROW_H / 2.0);
        p.add(egui::Shape::convex_polygon(vec![pos2(win.min.x + 0.5, ay - ARROW_H / 2.0), pos2(win.min.x - ARROW_W, ay), pos2(win.min.x + 0.5, ay + ARROW_H / 2.0)], win_fill(), Stroke::NONE));
        glow(&p, win);
        ui.allocate_rect(win, Sense::click_and_drag());
        // the title and the round close button (Logic's blue focus ring while it has the focus)
        let g = text(ui, "All Filters", head_text(), WIN_W - 80.0, look::semibold(10.5 * K));
        p.galley(pos2(win.center().x - g.size().x / 2.0, win.min.y + TITLE_CY - g.size().y / 2.0), g, head_text());
        let cc = pos2(win.max.x - 16.75 * K, win.min.y + 15.75 * K);
        let cr = Rect::from_center_size(cc, vec2(20.0 * K, 20.0 * K));
        let resp = ui.interact(cr, close_id, Sense::click());
        if first {
            resp.request_focus();
        }
        p.circle_filled(cc, 10.0 * K, if resp.hovered() { kn_ui::theme::rgb(if dark() { 0x5a5a5c } else { 0xc6c6ca }) } else { kn_ui::theme::rgb(if dark() { 0x4c4c4c } else { 0xd8d8dc }) });
        let d = 3.6 * K;
        let xs = Stroke::new(1.3 * K, head_text());
        p.line_segment([cc + vec2(-d, -d), cc + vec2(d, d)], xs);
        p.line_segment([cc + vec2(-d, d), cc + vec2(d, -d)], xs);
        // (the close button is the window's focused control from the start, as Logic's: Space or
        // Return closes)
        if resp.has_focus() || !ui.memory(|m| m.focused().is_some_and(|f| f != close_id)) {
            p.circle_stroke(cc, 14.25 * K, Stroke::new(2.75 * K, Color32::from_rgb(0x41, 0x6d, 0x95)));
        }
        let close = resp.clicked() || (!ui.memory(|m| m.focused().is_some_and(|f| f != close_id)) && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter)));
        // the cards, scrolling under the title
        let body = Rect::from_min_max(pos2(win.min.x + INSET, win.min.y + BODY_TOP), pos2(win.max.x - INSET, win.max.y - INSET));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
        child.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt("all-filters").auto_shrink([false, false]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded).show(&mut child, |ui| content(app, ui, &base));
        if close {
            app.br.all_filters = false;
        }
    });
}

/// Forgets that the window was open (so it takes the focus again next time).
pub fn closed(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<bool>(egui::Id::unique("all-filters-was-open")));
}

#[cfg(test)]
mod tests {
    use super::flow;
    use crate::soundlib::{self as sl, Item, Kind, LoopType, Search};

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

    fn item(kind: Kind, lt: LoopType, category: &str) -> Item {
        Item { id: format!("{kind:?}{lt:?}{category}"), name: "x".into(), kind, loop_type: lt, category: category.into(), path: None, tags: Vec::new(), groups: Vec::new(), pack: String::new(), user: false, bpm: None, key: None, minor: false, bars: None, seconds: None, location: None }
    }

    #[test]
    fn every_sound_has_a_type_and_the_type_pills_filter() {
        assert_eq!(item(Kind::Loop, LoopType::Audio, "").type_tag(), "Audio Loop");
        assert_eq!(item(Kind::Loop, LoopType::Midi, "").type_tag(), "MIDI Loop");
        assert_eq!(item(Kind::Loop, LoopType::Pattern, "").type_tag(), "Pattern Loop");
        assert_eq!(item(Kind::Loop, LoopType::SessionPlayer, "").type_tag(), "Session Player Loop");
        assert_eq!(item(Kind::OneShot, LoopType::Audio, "").type_tag(), "One-Shot");
        assert_eq!(item(Kind::Patch, LoopType::Midi, "Keyboard").type_tag(), "Instrument Patch");
        assert_eq!(item(Kind::Patch, LoopType::Audio, "Audio Effects").type_tag(), "Audio Patch");
        assert_eq!(item(Kind::Patch, LoopType::Audio, "Aux").type_tag(), "Aux Patch");
        for t in sl::TYPES {
            assert!(sl::TYPES.contains(t));
        }
        let fav = std::collections::HashSet::new();
        let mut s = Search::default();
        let (lp, os, pa) = (item(Kind::Loop, LoopType::Audio, ""), item(Kind::OneShot, LoopType::Audio, ""), item(Kind::Patch, LoopType::Midi, "Bass"));
        assert!(s.matches(&lp, &fav) && s.matches(&os, &fav) && s.matches(&pa, &fav));
        s.types.push("One-Shot".into());
        assert!(!s.matches(&lp, &fav) && s.matches(&os, &fav) && !s.matches(&pa, &fav));
        // two types: either matches
        s.types.push("Instrument Patch".into());
        assert!(!s.matches(&lp, &fav) && s.matches(&os, &fav) && s.matches(&pa, &fav));
        // a chip each, removable; kept in Smart Searches
        assert!(s.chips().contains(&sl::Chip::Type("One-Shot".into())));
        assert_eq!(sl::search_from_text(&sl::search_to_text(&s)).types, s.types);
        s.remove(&sl::Chip::Type("One-Shot".into()));
        assert_eq!(s.types, vec!["Instrument Patch".to_string()]);
        // counted for the Type card
        let c = sl::tag_counts([&lp, &os, &os].into_iter());
        assert_eq!(c.get("type\u{1f}One-Shot"), Some(&2));
    }

    #[test]
    fn effects_have_their_audio_fx_menu_category() {
        assert_eq!(super::fx_category("Compressor"), Some("Dynamics"));
        assert_eq!(super::fx_category("ChromaVerb"), Some("Reverb"));
        assert_eq!(super::fx_category("No Such Plug-in"), None);
        assert_eq!(super::fx_categories().first(), Some(&"Amps and Pedals"));
    }
}
