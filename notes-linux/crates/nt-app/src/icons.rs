//! Notes' glyphs. Those the suite already has are kn-ui's (kn_ui::icons, as Letters, Values and
//! Keydeck draw them): the side pane, table, paperclip, share, search, lock, list, gallery,
//! chevron and overflow. The ones only Notes needs — compose, checklist, markup,
//! new folder, Add Link (a chain: kn-ui's Link is Keynote's arrow), folders, Smart Folder (a gear), Recently Deleted, Quick Note, Shared, tag, the "…", pin —
//! are drawn here in kn-ui's style: its 24-unit grid and 1.6-unit stroke, rounded corners and
//! smooth arcs (candidates for kn-ui; see the shell's hand-over notes).

use egui::{pos2, Color32, Painter, Pos2, Rect, Shape, Stroke};
use kn_ui::icons::Icon;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Compose,
    Format,
    Checklist,
    Table,
    Paperclip,
    Markup,
    Share,
    More,
    Search,
    FolderPlus,
    Sidebar,
    Folder,
    SmartFolder,
    Trash,
    QuickNote,
    Shared,
    Tag,
    ChevronRight,
    ChevronDown,
    ChevronLeft,
    Pin,
    Link,
    Attachments,
    Overflow,
    Lock,
    Gallery,
    List,
}

/// kn-ui's icon for a glyph the suite already has.
fn suite(g: Glyph) -> Option<Icon> {
    Some(match g {
        Glyph::Sidebar => Icon::Sidebar,
        Glyph::Table => Icon::Table,
        Glyph::Paperclip => Icon::Media,
        Glyph::Share => Icon::Share,
        Glyph::Search => Icon::Search,
        Glyph::Lock => Icon::Lock,
        Glyph::List => Icon::List,
        Glyph::Gallery => Icon::Gallery,
        Glyph::ChevronDown => Icon::ChevronDown,
        Glyph::Overflow => Icon::Overflow,
        _ => return None,
    })
}

// kn-ui's drawing helpers (kn_ui::icons keeps its own private): a 0..24 design grid
fn p(r: Rect, x: f32, y: f32) -> Pos2 {
    pos2(r.min.x + x / 24.0 * r.width(), r.min.y + y / 24.0 * r.height())
}

fn rrect(painter: &Painter, r: Rect, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, s: Stroke) {
    let k = r.width() / 24.0;
    painter.rect_stroke(Rect::from_min_max(p(r, x0, y0), p(r, x1, y1)), radius * k, s, egui::StrokeKind::Middle);
}

fn line(painter: &Painter, r: Rect, pts: &[(f32, f32)], s: Stroke) {
    painter.add(Shape::line(pts.iter().map(|(x, y)| p(r, *x, *y)).collect(), s));
}

fn arc(r: Rect, cx: f32, cy: f32, rad: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let n = 24;
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            p(r, cx + rad * a.cos(), cy + rad * a.sin())
        })
        .collect()
}

/// The points of an outline through `pts` with each corner rounded by `rad` (grid units).
fn rounded(r: Rect, pts: &[(f32, f32)], rad: f32) -> Vec<Pos2> {
    let n = pts.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let d1 = (a.0 - b.0, a.1 - b.1);
        let d2 = (c.0 - b.0, c.1 - b.1);
        let l1 = (d1.0 * d1.0 + d1.1 * d1.1).sqrt().max(0.001);
        let l2 = (d2.0 * d2.0 + d2.1 * d2.1).sqrt().max(0.001);
        let t = rad.min(l1 / 2.0).min(l2 / 2.0);
        let s0 = (b.0 + d1.0 / l1 * t, b.1 + d1.1 / l1 * t);
        let s1 = (b.0 + d2.0 / l2 * t, b.1 + d2.1 / l2 * t);
        // a quadratic curve through the corner
        for k in 0..=6 {
            let u = k as f32 / 6.0;
            let x = (1.0 - u) * (1.0 - u) * s0.0 + 2.0 * (1.0 - u) * u * b.0 + u * u * s1.0;
            let y = (1.0 - u) * (1.0 - u) * s0.1 + 2.0 * (1.0 - u) * u * b.1 + u * u * s1.1;
            out.push(p(r, x, y));
        }
    }
    out
}

/// An open polyline through `pts` with its inner corners rounded by `rad` (grid units).
fn rounded_open(r: Rect, pts: &[(f32, f32)], rad: f32) -> Vec<Pos2> {
    let n = pts.len();
    let mut out = vec![p(r, pts[0].0, pts[0].1)];
    for i in 1..n - 1 {
        let (a, b, c) = (pts[i - 1], pts[i], pts[i + 1]);
        let d1 = (a.0 - b.0, a.1 - b.1);
        let d2 = (c.0 - b.0, c.1 - b.1);
        let l1 = (d1.0 * d1.0 + d1.1 * d1.1).sqrt().max(0.001);
        let l2 = (d2.0 * d2.0 + d2.1 * d2.1).sqrt().max(0.001);
        let t = rad.min(l1 / 2.0).min(l2 / 2.0);
        let s0 = (b.0 + d1.0 / l1 * t, b.1 + d1.1 / l1 * t);
        let s1 = (b.0 + d2.0 / l2 * t, b.1 + d2.1 / l2 * t);
        for k in 0..=6 {
            let u = k as f32 / 6.0;
            out.push(p(r, (1.0 - u) * (1.0 - u) * s0.0 + 2.0 * (1.0 - u) * u * b.0 + u * u * s1.0, (1.0 - u) * (1.0 - u) * s0.1 + 2.0 * (1.0 - u) * u * b.1 + u * u * s1.1));
        }
    }
    out.push(p(r, pts[n - 1].0, pts[n - 1].1));
    out
}

/// A folder's outline (the tab on the left), rounded.
fn folder(painter: &Painter, r: Rect, s: Stroke) {
    painter.add(Shape::closed_line(rounded(r, &[(2.5, 5.0), (9.0, 5.0), (11.0, 7.5), (21.5, 7.5), (21.5, 19.0), (2.5, 19.0)], 1.6), s));
    line(painter, r, &[(2.5, 10.0), (21.5, 10.0)], s);
}

/// Draw `g` in the square `r` in `col`.
pub fn draw(painter: &Painter, r: Rect, g: Glyph, col: Color32) {
    if let Some(i) = suite(g) {
        kn_ui::icons::draw(painter, r, i, col);
        return;
    }
    let k = r.width() / 24.0;
    let s = Stroke::new((1.6 * k).max(1.2), col);
    use std::f32::consts::PI;
    match g {
        Glyph::Compose => {
            // a rounded square open at the top right, a pencil through the gap
            let mut pts = vec![p(r, 12.0, 4.5)];
            pts.extend(arc(r, 7.0, 7.0, 2.5, -PI / 2.0, -PI));
            pts.extend(arc(r, 7.0, 17.0, 2.5, PI, PI / 2.0));
            pts.extend(arc(r, 17.0, 17.0, 2.5, PI / 2.0, 0.0));
            pts.push(p(r, 19.5, 12.0));
            painter.add(Shape::line(pts, s));
            painter.add(Shape::closed_line(rounded(r, &[(10.0, 14.0), (10.6, 11.4), (18.4, 3.6), (20.4, 5.6), (12.6, 13.4)], 0.6), s));
        }
        Glyph::Format => {
            // SF's toolbar Aa: a single-storey ɑ
            let f = egui::FontId::proportional(r.width() * 0.8);
            painter.text(p(r, 12.0, 12.5), egui::Align2::CENTER_CENTER, "A\u{251}", f, col);
        }
        Glyph::Checklist => {
            // Apple's checklist (measured in Notes' toolbar at 2x): two circles of exactly the
            // same size, 7 pt across and 9.5 pt apart on one x, the top one filled with the tick
            // cut out, and a line of the circles' weight beside each, level with its centre.
            // (The ring is a path centred on its radius, so its outside meets the disc's: egui's
            // circle_stroke draws outside the radius.)
            let (cx, top, bottom, rad) = (5.75, 6.85, 17.25, 3.85);
            let cut = Stroke::new(s.width * 0.7, crate::look::capsule_fill());
            painter.circle_filled(p(r, cx, top), rad * k, col);
            line(painter, r, &[(cx - 1.7, top + 0.1), (cx - 0.4, top + 1.5), (cx + 1.8, top - 1.4)], cut);
            let ring: Vec<Pos2> = (0..48).map(|i| {
                let a = i as f32 / 48.0 * 2.0 * PI;
                p(r, cx, bottom) + egui::vec2(a.cos(), a.sin()) * (rad * k - s.width / 2.0)
            }).collect();
            painter.add(Shape::closed_line(ring, s));
            line(painter, r, &[(12.0, top), (22.0, top)], s);
            line(painter, r, &[(12.0, bottom), (22.0, bottom)], s);
        }
        Glyph::Markup => {
            // Apple's pencil.tip.crop.circle: a pen nib rising from the circle's foot, its sides
            // easing into a rounded point, a band across it
            painter.circle_stroke(p(r, 12.0, 12.0), 8.5 * k, s);
            let side = |x0: f32, xm: f32| -> Vec<Pos2> {
                (0..=12)
                    .map(|i| {
                        let u = i as f32 / 12.0;
                        // from the foot (x0, 20.3) up to the point (12, 6.5), bowing out at xm
                        let (ax, ay, bx, by, cx, cy) = (x0, 20.3, xm, 11.0, 12.0, 6.5);
                        p(r, (1.0 - u) * (1.0 - u) * ax + 2.0 * (1.0 - u) * u * bx + u * u * cx, (1.0 - u) * (1.0 - u) * ay + 2.0 * (1.0 - u) * u * by + u * u * cy)
                    })
                    .collect()
            };
            let mut nib = side(8.3, 9.9);
            let mut right = side(15.7, 14.1);
            right.reverse();
            nib.extend(right.into_iter().skip(1));
            painter.add(Shape::line(nib, s));
            line(painter, r, &[(9.6, 15.0), (14.4, 15.0)], s);
            // the point, inked
            painter.add(Shape::convex_polygon(vec![p(r, 12.0, 6.6), p(r, 13.15, 10.2), p(r, 10.85, 10.2)], col, Stroke::new(s.width * 0.5, col)));
        }
        Glyph::More => {
            for x in [5.5, 12.0, 18.5] {
                painter.circle_filled(p(r, x, 12.0), 1.8 * k, col);
            }
        }
        Glyph::Link => {
            // two chain links on a slant and a plus badge (Notes' Add Link)
            let (c, sn) = (0.7071f32, 0.7071f32);
            let link = |cx: f32, cy: f32| -> Vec<Pos2> {
                // a stadium 10.4 long, 6 wide, on the rising diagonal
                let mut pts = Vec::new();
                for (ox, a0) in [(2.2f32, -PI / 2.0), (-2.2, PI / 2.0)] {
                    for i in 0..=12 {
                        let a = a0 + PI * i as f32 / 12.0;
                        let (x, y) = (ox + 3.0 * a.cos(), 3.0 * a.sin());
                        pts.push(p(r, cx + x * c + y * sn, cy - x * sn + y * c));
                    }
                }
                pts
            };
            painter.add(Shape::closed_line(link(8.6, 15.4), s));
            painter.add(Shape::closed_line(link(15.4, 8.6), s));
            painter.circle_filled(p(r, 18.5, 18.5), 4.5 * k, col);
            let cut = Stroke::new(s.width, if col.r() > 128 { Color32::BLACK } else { Color32::WHITE });
            line(painter, r, &[(18.5, 16.3), (18.5, 20.7)], cut);
            line(painter, r, &[(16.3, 18.5), (20.7, 18.5)], cut);
        }
        Glyph::FolderPlus => {
            // Apple's folder.badge.plus: the folder, its top-right corner under a plus badge
            painter.add(Shape::line(rounded_open(r, &[(14.0, 7.5), (11.0, 7.5), (9.0, 5.0), (2.5, 5.0), (2.5, 19.0), (21.5, 19.0), (21.5, 12.5)], 1.6), s));
            line(painter, r, &[(2.5, 10.0), (14.5, 10.0)], s);
            let c = (19.5, 7.0);
            painter.circle_filled(p(r, c.0, c.1), 4.8 * k, col);
            let cut = Stroke::new(s.width, if col.r() > 128 { Color32::BLACK } else { Color32::WHITE });
            line(painter, r, &[(c.0, c.1 - 2.4), (c.0, c.1 + 2.4)], cut);
            line(painter, r, &[(c.0 - 2.4, c.1), (c.0 + 2.4, c.1)], cut);
        }
        Glyph::Folder => folder(painter, r, s),
        Glyph::SmartFolder => {
            // Apple's gearshape (the sidebar shows Smart Folders as a gear alone): eight rounded
            // teeth round a centre hole
            let (rm, amp, sharp) = (7.9f32, 1.45f32, 1.7f32);
            let n = 192;
            let pts: Vec<Pos2> = (0..n)
                .map(|i| {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    let t = (sharp * (8.0 * a).cos()).tanh() / sharp.tanh();
                    let rr = rm + amp * t;
                    p(r, 12.0 + rr * a.cos(), 12.0 + rr * a.sin())
                })
                .collect();
            painter.add(Shape::closed_line(pts, s));
            painter.circle_stroke(p(r, 12.0, 12.0), 3.0 * k, s);
        }
        Glyph::Trash => {
            line(painter, r, &[(4.0, 6.5), (20.0, 6.5)], s);
            let mut lid = vec![p(r, 9.0, 6.5)];
            lid.extend(arc(r, 10.0, 4.5, 1.0, PI, 1.5 * PI));
            lid.extend(arc(r, 14.0, 4.5, 1.0, 1.5 * PI, 2.0 * PI));
            lid.push(p(r, 15.0, 6.5));
            painter.add(Shape::line(lid, s));
            let mut can = vec![p(r, 6.0, 6.5)];
            can.extend(arc(r, 8.6, 18.5, 2.0, PI * 0.97, PI * 0.5));
            can.extend(arc(r, 15.4, 18.5, 2.0, PI * 0.5, PI * 0.03));
            can.push(p(r, 18.0, 6.5));
            painter.add(Shape::line(can, s));
            line(painter, r, &[(10.0, 10.0), (10.0, 17.0)], s);
            line(painter, r, &[(14.0, 10.0), (14.0, 17.0)], s);
        }
        Glyph::QuickNote => {
            // a note with a folded corner and two lines
            painter.add(Shape::closed_line(rounded(r, &[(5.0, 3.5), (14.5, 3.5), (19.0, 8.0), (19.0, 20.5), (5.0, 20.5)], 1.5), s));
            line(painter, r, &[(14.5, 3.5), (14.5, 8.0), (19.0, 8.0)], s);
            line(painter, r, &[(8.5, 12.5), (15.5, 12.5)], s);
            line(painter, r, &[(8.5, 16.5), (13.5, 16.5)], s);
        }
        Glyph::Shared => {
            painter.circle_stroke(p(r, 9.0, 8.5), 3.0 * k, s);
            painter.add(Shape::line(arc(r, 9.0, 20.0, 6.0, PI, 2.0 * PI), s));
            painter.circle_stroke(p(r, 17.0, 9.5), 2.4 * k, s);
            painter.add(Shape::line(arc(r, 17.5, 19.5, 4.5, 1.15 * PI, 2.0 * PI), s));
        }
        Glyph::Tag => {
            painter.add(Shape::closed_line(rounded(r, &[(3.5, 4.0), (12.0, 4.0), (20.5, 12.5), (12.5, 20.5), (3.5, 12.0)], 1.8), s));
            painter.circle_filled(p(r, 8.0, 8.5), 1.4 * k, col);
        }
        Glyph::ChevronRight => line(painter, r, &[(9.5, 7.0), (14.5, 12.0), (9.5, 17.0)], s),
        Glyph::ChevronLeft => line(painter, r, &[(14.5, 7.0), (9.5, 12.0), (14.5, 17.0)], s),
        Glyph::Pin => {
            painter.add(Shape::closed_line(rounded(r, &[(8.0, 3.5), (16.0, 3.5), (14.5, 6.0), (14.5, 10.5), (18.0, 14.0), (6.0, 14.0), (9.5, 10.5), (9.5, 6.0)], 0.8), s));
            line(painter, r, &[(12.0, 14.0), (12.0, 21.0)], s);
        }
        Glyph::Attachments => {
            rrect(painter, r, 3.0, 5.0, 19.0, 19.0, 2.5, s);
            painter.add(Shape::closed_line(rounded(r, &[(10.0, 14.0), (10.6, 11.4), (18.4, 3.6), (20.4, 5.6), (12.6, 13.4)], 0.6), s));
        }
        _ => {}
    }
}
