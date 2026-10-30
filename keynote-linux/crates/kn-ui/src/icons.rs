//! Toolbar / inspector icons drawn as vector strokes, after Keynote 15.4's toolbar symbols
//! (sidebar, play, add slide, table, chart, text, shape, media, comment, share, format,
//! animate, document). Each icon is drawn into a square `r` in the given colour.

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// Keydeck's mark (the app menu): a wall screen on its roller showing a pie and bars.
    Projector,
    Sidebar,
    Play,
    AddSlide,
    Table,
    Chart,
    Text,
    Shape,
    Media,
    Comment,
    Share,
    Format,
    Animate,
    Document,
    ChevronDown,
    // menu icons (Keynote 15.4's menus show these SF Symbols)
    Print,
    Search,
    Line,
    Highlight,
    Gallery,
    WebVideo,
    LiveVideo,
    Audio,
    File,
    LayerForward,
    LayerFront,
    LayerBackward,
    LayerBack,
    AlignObjects,
    Distribute,
    FlipH,
    FlipV,
    List,
    Palette,
    Sliders,
    Music,
    Photo,
    Mic,
    Smiley,
    // toolbar items (View > Customize Toolbar…)
    PlayWindow,
    Rehearse,
    Layouts,
    Skip,
    Record,
    Group,
    Ungroup,
    Bigger,
    Smaller,
    Superscript,
    Subscript,
    Outdent,
    Indent,
    Connect,
    CopyStyle,
    PasteStyle,
    Guides,
    Mask,
    RemoveBg,
    Lock,
    Unlock,
    Fonts,
    Link,
    Overflow,
}

fn p(r: Rect, x: f32, y: f32) -> Pos2 {
    // x, y in a 0..24 design grid
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

pub fn draw(painter: &Painter, r: Rect, icon: Icon, color: Color32) {
    let k = r.width() / 24.0;
    let s = Stroke::new((1.6 * k).max(1.2), color);
    use std::f32::consts::PI;
    match icon {
        Icon::Projector => {
            // roller, screen, the picture on it, the pull cord
            line(painter, r, &[(2.0, 4.0), (22.0, 4.0)], Stroke::new(s.width * 1.5, color));
            rrect(painter, r, 4.0, 4.0, 20.0, 16.5, 1.0, s);
            painter.add(Shape::line(arc(r, 8.8, 10.8, 2.6, -PI / 2.0, 1.5 * PI), Stroke::new(s.width * 0.9, color)));
            line(painter, r, &[(8.8, 8.2), (8.8, 10.8), (11.4, 10.8)], Stroke::new(s.width * 0.9, color));
            for (x, top) in [(14.0, 11.0), (16.2, 8.5)] {
                line(painter, r, &[(x, 13.6), (x, top)], Stroke::new(s.width * 1.1, color));
            }
            line(painter, r, &[(12.0, 16.5), (12.0, 19.5)], Stroke::new(s.width * 0.8, color));
            painter.circle_filled(p(r, 12.0, 20.3), 1.1 * k, color);
        }
        Icon::Sidebar => {
            rrect(painter, r, 3.0, 5.0, 21.0, 19.0, 3.0, s);
            line(painter, r, &[(9.0, 5.0), (9.0, 19.0)], s);
            for y in [8.5, 11.0, 13.5] {
                line(painter, r, &[(5.0, y), (7.0, y)], Stroke::new(s.width * 0.9, color));
            }
        }
        Icon::Play => {
            let pts = vec![p(r, 7.5, 5.0), p(r, 19.0, 12.0), p(r, 7.5, 19.0)];
            painter.add(Shape::convex_polygon(pts, color, Stroke::new(1.0 * k, color)));
        }
        Icon::AddSlide => {
            rrect(painter, r, 3.5, 5.0, 20.5, 19.0, 2.5, s);
            line(painter, r, &[(12.0, 8.5), (12.0, 15.5)], s);
            line(painter, r, &[(8.5, 12.0), (15.5, 12.0)], s);
        }
        Icon::Table => {
            rrect(painter, r, 3.0, 5.0, 21.0, 19.0, 2.0, s);
            line(painter, r, &[(3.0, 9.5), (21.0, 9.5)], s);
            line(painter, r, &[(3.0, 14.25), (21.0, 14.25)], s);
            line(painter, r, &[(9.0, 9.5), (9.0, 19.0)], s);
            line(painter, r, &[(15.0, 9.5), (15.0, 19.0)], s);
        }
        Icon::Chart => {
            // pie with one slice pulled to the centre
            painter.add(Shape::line(arc(r, 12.0, 12.0, 8.0, -PI / 2.0 + 0.0, 1.5 * PI), s));
            line(painter, r, &[(12.0, 4.0), (12.0, 12.0), (19.0, 15.5)], s);
        }
        Icon::Text => {
            // "A" in a frame with corner marks
            for (x, y, dx, dy) in [(3.5, 5.0, 1.0, 1.0), (20.5, 5.0, -1.0, 1.0), (3.5, 19.0, 1.0, -1.0), (20.5, 19.0, -1.0, -1.0)] {
                line(painter, r, &[(x + 2.5 * dx, y), (x, y), (x, y + 2.5 * dy)], s);
            }
            line(painter, r, &[(8.5, 16.0), (12.0, 7.5), (15.5, 16.0)], s);
            line(painter, r, &[(9.8, 13.0), (14.2, 13.0)], s);
        }
        Icon::Shape => {
            rrect(painter, r, 3.5, 3.5, 15.0, 15.0, 2.5, s);
            painter.circle_stroke(p(r, 14.5, 14.5), 6.0 * k, s);
        }
        Icon::Media => {
            // paper clip
            let mut pts = arc(r, 12.0, 6.5, 3.0, PI, 2.0 * PI);
            pts.insert(0, p(r, 9.0, 17.0));
            pts.push(p(r, 15.0, 16.5));
            pts.extend(arc(r, 12.0, 16.5, 3.0, 0.0, PI));
            pts.push(p(r, 9.0, 8.0));
            pts.extend(arc(r, 10.5, 8.0, 1.5, PI, 2.0 * PI));
            pts.push(p(r, 12.0, 16.0));
            painter.add(Shape::line(pts, s));
        }
        Icon::Comment => {
            rrect(painter, r, 3.5, 4.5, 20.5, 16.5, 3.0, s);
            line(painter, r, &[(8.0, 16.5), (7.5, 20.0), (11.5, 16.5)], s);
            line(painter, r, &[(7.5, 8.5), (16.5, 8.5)], s);
            line(painter, r, &[(7.5, 12.0), (14.0, 12.0)], s);
        }
        Icon::Share => {
            line(painter, r, &[(9.0, 9.5), (6.0, 9.5), (6.0, 20.5), (18.0, 20.5), (18.0, 9.5), (15.0, 9.5)], s);
            line(painter, r, &[(12.0, 3.5), (12.0, 14.5)], s);
            line(painter, r, &[(8.5, 7.0), (12.0, 3.5), (15.5, 7.0)], s);
        }
        Icon::Format => {
            // paintbrush
            line(painter, r, &[(17.5, 3.5), (20.5, 6.5), (12.5, 14.5), (9.5, 11.5), (17.5, 3.5)], s);
            line(painter, r, &[(9.5, 11.5), (6.0, 13.0), (4.0, 20.0), (11.0, 18.0), (12.5, 14.5)], s);
        }
        Icon::Animate => {
            line(painter, r, &[(12.0, 3.5), (20.5, 12.0), (12.0, 20.5), (3.5, 12.0), (12.0, 3.5)], s);
            line(painter, r, &[(15.0, 3.5), (23.0, 11.5)], Stroke::new(s.width * 0.8, color));
        }
        Icon::Document => {
            rrect(painter, r, 3.0, 5.0, 21.0, 19.0, 2.5, s);
            painter.rect_filled(Rect::from_min_max(p(r, 6.5, 8.5), p(r, 17.5, 15.5)), 1.0 * k, color);
        }
        Icon::ChevronDown => {
            line(painter, r, &[(7.0, 9.5), (12.0, 14.5), (17.0, 9.5)], s);
        }
        Icon::Print => {
            rrect(painter, r, 3.5, 9.0, 20.5, 17.0, 2.0, s);
            rrect(painter, r, 7.0, 4.0, 17.0, 9.0, 0.5, s);
            rrect(painter, r, 7.0, 14.0, 17.0, 20.5, 0.5, s);
        }
        Icon::Search => {
            painter.circle_stroke(p(r, 10.5, 10.5), 6.0 * k, s);
            line(painter, r, &[(15.0, 15.0), (20.5, 20.5)], s);
        }
        Icon::Line => line(painter, r, &[(5.0, 19.0), (19.0, 5.0)], s),
        Icon::Highlight => {
            line(painter, r, &[(6.0, 18.0), (16.0, 8.0), (18.5, 10.5), (8.5, 20.5), (6.0, 18.0)], s);
            line(painter, r, &[(4.0, 21.0), (20.0, 21.0)], s);
        }
        Icon::Gallery => {
            rrect(painter, r, 4.0, 5.0, 20.0, 15.0, 1.5, s);
            for x in [8.0, 12.0, 16.0] {
                painter.circle_filled(p(r, x, 19.0), 0.9 * k, color);
            }
        }
        Icon::WebVideo => {
            painter.add(Shape::line(arc(r, 12.0, 12.0, 8.0, 0.0, 2.0 * PI), s));
            painter.add(Shape::convex_polygon(vec![p(r, 10.0, 8.5), p(r, 15.5, 12.0), p(r, 10.0, 15.5)], color, Stroke::NONE));
        }
        Icon::LiveVideo => {
            rrect(painter, r, 3.0, 8.0, 15.0, 17.0, 2.0, s);
            line(painter, r, &[(15.0, 11.0), (20.5, 8.0), (20.5, 17.0), (15.0, 14.0)], s);
            painter.circle_stroke(p(r, 6.5, 5.5), 1.8 * k, s);
            painter.circle_stroke(p(r, 11.5, 5.5), 1.8 * k, s);
        }
        Icon::Audio => {
            for (i, hgt) in [3.0, 7.0, 11.0, 7.0, 4.0, 9.0, 5.0].iter().enumerate() {
                let x = 5.0 + i as f32 * 2.3;
                line(painter, r, &[(x, 12.0 - hgt / 2.0), (x, 12.0 + hgt / 2.0)], s);
            }
        }
        Icon::File => {
            line(painter, r, &[(6.0, 3.5), (14.0, 3.5), (18.5, 8.0), (18.5, 20.5), (6.0, 20.5), (6.0, 3.5)], s);
            line(painter, r, &[(14.0, 3.5), (14.0, 8.0), (18.5, 8.0)], s);
        }
        Icon::LayerForward | Icon::LayerFront | Icon::LayerBackward | Icon::LayerBack => {
            let solid_top = matches!(icon, Icon::LayerForward | Icon::LayerFront);
            let n = if matches!(icon, Icon::LayerFront | Icon::LayerBack) { 3 } else { 2 };
            for i in 0..n {
                let y = 8.0 + i as f32 * 4.0;
                let pts = vec![p(r, 4.0, y), p(r, 12.0, y - 4.0), p(r, 20.0, y), p(r, 12.0, y + 4.0)];
                let solid = (solid_top && i == 0) || (!solid_top && i == n - 1);
                if solid {
                    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
                } else {
                    painter.add(Shape::closed_line(pts, s));
                }
            }
        }
        Icon::AlignObjects => {
            line(painter, r, &[(12.0, 3.5), (12.0, 20.5)], s);
            rrect(painter, r, 6.0, 6.0, 18.0, 10.0, 1.0, s);
            rrect(painter, r, 8.0, 14.0, 16.0, 18.0, 1.0, s);
        }
        Icon::Distribute => {
            line(painter, r, &[(4.0, 4.0), (4.0, 20.0)], s);
            line(painter, r, &[(20.0, 4.0), (20.0, 20.0)], s);
            rrect(painter, r, 9.0, 7.0, 15.0, 17.0, 1.0, s);
        }
        Icon::FlipH => {
            line(painter, r, &[(12.0, 3.5), (12.0, 20.5)], Stroke::new(s.width * 0.7, color));
            painter.add(Shape::convex_polygon(vec![p(r, 10.0, 6.0), p(r, 10.0, 18.0), p(r, 4.0, 18.0)], color, Stroke::NONE));
            painter.add(Shape::closed_line(vec![p(r, 14.0, 6.0), p(r, 14.0, 18.0), p(r, 20.0, 18.0)], s));
        }
        Icon::FlipV => {
            line(painter, r, &[(3.5, 12.0), (20.5, 12.0)], Stroke::new(s.width * 0.7, color));
            painter.add(Shape::convex_polygon(vec![p(r, 6.0, 10.0), p(r, 18.0, 10.0), p(r, 18.0, 4.0)], color, Stroke::NONE));
            painter.add(Shape::closed_line(vec![p(r, 6.0, 14.0), p(r, 18.0, 14.0), p(r, 18.0, 20.0)], s));
        }
        Icon::List => {
            for y in [7.0, 12.0, 17.0] {
                painter.circle_filled(p(r, 5.0, y), 1.0 * k, color);
                line(painter, r, &[(8.5, y), (20.0, y)], s);
            }
        }
        Icon::Palette => {
            painter.add(Shape::line(arc(r, 12.0, 12.0, 8.5, 0.3, 2.0 * PI - 0.6), s));
            for (x, y) in [(8.0, 9.0), (12.0, 7.0), (16.0, 9.0), (8.5, 14.0)] {
                painter.circle_filled(p(r, x, y), 1.2 * k, color);
            }
        }
        Icon::Sliders => {
            for (y, x) in [(7.0, 15.0), (12.0, 9.0), (17.0, 13.0)] {
                line(painter, r, &[(4.0, y), (20.0, y)], s);
                painter.circle_filled(p(r, x, y), 1.8 * k, color);
            }
        }
        Icon::Music => {
            line(painter, r, &[(10.0, 18.0), (10.0, 5.0), (17.0, 7.0)], s);
            painter.circle_filled(p(r, 8.0, 18.0), 2.5 * k, color);
        }
        Icon::Photo => {
            rrect(painter, r, 3.5, 6.0, 17.5, 18.0, 1.5, s);
            rrect(painter, r, 6.5, 3.5, 20.5, 15.5, 1.5, s);
        }
        Icon::Mic => {
            rrect(painter, r, 9.0, 3.5, 15.0, 14.0, 3.0, s);
            painter.add(Shape::line(arc(r, 12.0, 11.0, 6.0, 0.0, PI), s));
            line(painter, r, &[(12.0, 17.0), (12.0, 20.5)], s);
        }
        Icon::PlayWindow => {
            rrect(painter, r, 3.0, 5.0, 21.0, 19.0, 2.5, s);
            line(painter, r, &[(3.0, 8.5), (21.0, 8.5)], s);
            painter.add(Shape::convex_polygon(vec![p(r, 10.0, 11.0), p(r, 15.5, 14.0), p(r, 10.0, 17.0)], color, Stroke::NONE));
        }
        Icon::Rehearse => {
            rrect(painter, r, 3.0, 5.0, 21.0, 19.0, 3.0, s);
            painter.add(Shape::convex_polygon(vec![p(r, 9.5, 8.5), p(r, 16.0, 12.0), p(r, 9.5, 15.5)], color, Stroke::NONE));
        }
        Icon::Layouts => {
            rrect(painter, r, 3.0, 5.0, 21.0, 19.0, 2.0, s);
            rrect(painter, r, 6.0, 8.0, 18.0, 11.0, 1.0, s);
            line(painter, r, &[(6.0, 14.0), (18.0, 14.0)], s);
            line(painter, r, &[(6.0, 16.5), (14.0, 16.5)], s);
        }
        Icon::Skip => {
            painter.add(Shape::line(arc(r, 12.0, 14.0, 7.0, PI, 2.0 * PI), s));
            line(painter, r, &[(16.0, 14.0), (19.0, 14.0), (19.0, 11.0)], s);
            painter.circle_filled(p(r, 12.0, 17.0), 1.5 * k, color);
        }
        Icon::Record => {
            painter.circle_stroke(p(r, 12.0, 12.0), 8.0 * k, s);
            painter.circle_filled(p(r, 12.0, 12.0), 4.0 * k, color);
        }
        Icon::Group => {
            rrect(painter, r, 4.0, 4.0, 20.0, 20.0, 3.0, s);
            rrect(painter, r, 8.0, 8.0, 16.0, 16.0, 1.5, s);
        }
        Icon::Ungroup => {
            for (x, y) in [(4.0, 4.0), (16.0, 4.0), (4.0, 16.0), (16.0, 16.0)] {
                rrect(painter, r, x, y, x + 4.0, y + 4.0, 1.0, s);
            }
            rrect(painter, r, 9.0, 9.0, 15.0, 15.0, 1.5, s);
        }
        Icon::Bigger | Icon::Smaller | Icon::Superscript | Icon::Subscript | Icon::Fonts => {
            let (big, small, dy) = match icon {
                Icon::Bigger => ("A", "+", 0.0),
                Icon::Smaller => ("A", "−", 0.0),
                Icon::Superscript => ("A", "1", -5.0),
                Icon::Subscript => ("A", "1", 5.0),
                _ => ("Aa", "", 0.0),
            };
            let f = egui::FontId::proportional(13.0 * k);
            painter.text(p(r, if small.is_empty() { 12.0 } else { 10.0 }, 12.5), egui::Align2::CENTER_CENTER, big, f, color);
            if !small.is_empty() {
                painter.text(p(r, 18.0, 12.0 + dy), egui::Align2::CENTER_CENTER, small, egui::FontId::proportional(8.5 * k), color);
            }
        }
        Icon::Outdent | Icon::Indent => {
            for y in [5.0, 19.0] {
                line(painter, r, &[(4.0, y), (20.0, y)], s);
            }
            for y in [10.0, 14.0] {
                line(painter, r, &[(11.0, y), (20.0, y)], s);
            }
            let pts = if icon == Icon::Indent { vec![p(r, 4.0, 9.0), p(r, 8.0, 12.0), p(r, 4.0, 15.0)] } else { vec![p(r, 8.0, 9.0), p(r, 4.0, 12.0), p(r, 8.0, 15.0)] };
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::Connect => {
            painter.circle_stroke(p(r, 6.5, 17.5), 2.5 * k, s);
            painter.circle_stroke(p(r, 17.5, 6.5), 2.5 * k, s);
            line(painter, r, &[(8.5, 15.5), (15.5, 8.5)], s);
        }
        Icon::CopyStyle | Icon::PasteStyle => {
            // eyedropper
            line(painter, r, &[(5.0, 19.0), (14.0, 10.0)], s);
            rrect(painter, r, 13.0, 5.0, 19.0, 11.0, 2.5, s);
            if icon == Icon::PasteStyle {
                painter.circle_filled(p(r, 5.0, 19.0), 2.0 * k, color);
            }
        }
        Icon::Guides => {
            line(painter, r, &[(12.0, 3.0), (12.0, 21.0)], s);
            line(painter, r, &[(3.0, 12.0), (21.0, 12.0)], s);
            rrect(painter, r, 7.0, 7.0, 17.0, 17.0, 1.0, Stroke::new(s.width * 0.7, color));
        }
        Icon::Mask => {
            line(painter, r, &[(7.0, 3.0), (7.0, 17.0), (21.0, 17.0)], s);
            line(painter, r, &[(3.0, 7.0), (17.0, 7.0), (17.0, 21.0)], s);
        }
        Icon::RemoveBg => {
            rrect(painter, r, 3.0, 5.0, 21.0, 19.0, 2.0, s);
            for (x, y) in [(5.0, 7.0), (9.0, 11.0), (5.0, 15.0), (13.0, 7.0), (17.0, 11.0), (13.0, 15.0)] {
                painter.rect_filled(Rect::from_min_max(p(r, x, y), p(r, x + 2.5, y + 2.5)), 0.0, color);
            }
        }
        Icon::Lock | Icon::Unlock => {
            rrect(painter, r, 6.0, 11.0, 18.0, 20.0, 2.0, s);
            let a1 = if icon == Icon::Lock { 2.0 * PI } else { 1.75 * PI };
            let mut pts = vec![p(r, 8.5, 11.0)];
            pts.extend(arc(r, 12.0, 7.5, 3.5, PI, a1));
            if icon == Icon::Lock {
                pts.push(p(r, 15.5, 11.0));
            }
            painter.add(Shape::line(pts, s));
        }
        Icon::Link => {
            painter.circle_stroke(p(r, 12.0, 12.0), 8.0 * k, s);
            line(painter, r, &[(8.5, 15.5), (15.5, 8.5)], s);
            line(painter, r, &[(11.0, 8.5), (15.5, 8.5), (15.5, 13.0)], s);
        }
        Icon::Overflow => {
            line(painter, r, &[(6.0, 7.0), (11.0, 12.0), (6.0, 17.0)], s);
            line(painter, r, &[(13.0, 7.0), (18.0, 12.0), (13.0, 17.0)], s);
        }
        Icon::Smiley => {
            painter.circle_stroke(p(r, 12.0, 12.0), 8.0 * k, s);
            painter.circle_filled(p(r, 9.5, 10.0), 1.0 * k, color);
            painter.circle_filled(p(r, 14.5, 10.0), 1.0 * k, color);
            painter.add(Shape::line(arc(r, 12.0, 12.5, 3.5, 0.3, PI - 0.3), s));
        }
    }
}

/// The icon Keynote 15.4 shows next to a menu item (only some items have one).
pub fn menu_icon(label: &str) -> Option<Icon> {
    Some(match label {
        "Share…" => Icon::Share,
        "Print…" => Icon::Print,
        "Find" => Icon::Search,
        "Table" => Icon::Table,
        "Chart" => Icon::Chart,
        "Text" => Icon::Text,
        "Shape" => Icon::Shape,
        "Line" => Icon::Line,
        "Highlight" => Icon::Highlight,
        "Comment" => Icon::Comment,
        "Image Gallery" => Icon::Gallery,
        "Web Video…" => Icon::WebVideo,
        "Live Video" | "Show Live Video Preview" => Icon::LiveVideo,
        "Record Audio" => Icon::Audio,
        "Choose File" => Icon::File,
        "New Slide" => Icon::AddSlide,
        "Bring Forward" => Icon::LayerForward,
        "Bring to Front" => Icon::LayerFront,
        "Send Backward" => Icon::LayerBackward,
        "Send to Back" => Icon::LayerBack,
        "Show Arrange Tools" => Icon::LayerFront,
        "Align Objects" => Icon::AlignObjects,
        "Distribute Objects" => Icon::Distribute,
        "Flip Horizontally" => Icon::FlipH,
        "Flip Vertically" => Icon::FlipV,
        "Show Build Order" => Icon::List,
        "Show Colors" => Icon::Palette,
        "Show Adjust Image" => Icon::Sliders,
        "Show Music Browser" => Icon::Music,
        "Show Photo and Video Browser" => Icon::Photo,
        "Start Dictation" => Icon::Mic,
        "Emoji & Symbols" => Icon::Smiley,
        "AutoFill" => Icon::Text,
        _ => return None,
    })
}

/// Paint a menu item's icon at the left of its row (`r` = the item's rect).
pub fn paint_menu_icon(ui: &egui::Ui, center: Pos2, icon: Icon, enabled: bool) {
    let col = if enabled { crate::theme::TEXT() } else { crate::theme::TEXT_DIM() };
    let ir = Rect::from_center_size(center, vec2(16.0, 16.0));
    draw(ui.painter(), ir, icon, col);
}
