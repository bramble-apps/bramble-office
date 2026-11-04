//! Vector icons in the style of the SF Symbols Calculator uses, drawn from a few primitives so
//! the egui and Flutter UIs show the same shapes. 24×24 box, origin top-left, y down.
//! Stroked prims (Line, Arc, unfilled Poly/Circle/RRect) use round caps and joins.

use serde::Serialize;
use std::f32::consts::PI;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "k")]
pub enum Prim {
    Line { a: [f32; 2], b: [f32; 2] },
    Poly { pts: Vec<[f32; 2]>, closed: bool, fill: bool },
    Circle { c: [f32; 2], r: f32, fill: bool },
    /// angles in radians, 0 = +x, increasing clockwise on screen (y down); drawn from a0 to a1
    Arc { c: [f32; 2], r: f32, a0: f32, a1: f32 },
    RRect { min: [f32; 2], max: [f32; 2], r: f32, fill: bool },
}

#[derive(Clone, Debug, Serialize)]
pub struct Icon {
    pub prims: Vec<Prim>,
    /// stroke width in box units
    pub stroke: f32,
}

pub const NAMES: &[&str] = &[
    "clock", "list", "calculator", "delete", "swap", "updown", "sidebar", "check", "search", "mic",
    "basic", "function", "mathnotes", "convert", "back", "chevron_right", "chevron_down", "trash",
    "share", "ellipsis", "graph", "compose", "plus", "xmark", "gear", "copy", "paintbrush",
    "circle", "checkcircle", "checklist", "paperclip", "markup",
];

fn ln(ax: f32, ay: f32, bx: f32, by: f32) -> Prim {
    Prim::Line { a: [ax, ay], b: [bx, by] }
}
fn open(pts: &[[f32; 2]]) -> Prim {
    Prim::Poly { pts: pts.to_vec(), closed: false, fill: false }
}
fn shut(pts: &[[f32; 2]]) -> Prim {
    Prim::Poly { pts: pts.to_vec(), closed: true, fill: false }
}
fn ring(x: f32, y: f32, r: f32) -> Prim {
    Prim::Circle { c: [x, y], r, fill: false }
}
fn dot(x: f32, y: f32, r: f32) -> Prim {
    Prim::Circle { c: [x, y], r, fill: true }
}
fn arc(x: f32, y: f32, r: f32, a0: f32, a1: f32) -> Prim {
    Prim::Arc { c: [x, y], r, a0, a1 }
}
fn rrect(x0: f32, y0: f32, x1: f32, y1: f32, r: f32, fill: bool) -> Prim {
    Prim::RRect { min: [x0, y0], max: [x1, y1], r, fill }
}
fn deg(d: f32) -> f32 {
    d * PI / 180.0
}

/// The icon called `name` (see [`NAMES`]).
pub fn icon(name: &str) -> Option<Icon> {
    let mut stroke = 1.9;
    let prims = match name {
        "clock" => vec![ring(12.0, 12.0, 9.2), open(&[[12.0, 6.6], [12.0, 12.0], [15.6, 14.4]])],
        "list" => vec![
            dot(4.8, 6.0, 1.5),
            dot(4.8, 12.0, 1.5),
            dot(4.8, 18.0, 1.5),
            ln(9.0, 6.0, 20.5, 6.0),
            ln(9.0, 12.0, 20.5, 12.0),
            ln(9.0, 18.0, 20.5, 18.0),
        ],
        "calculator" => {
            let mut v = vec![rrect(4.8, 2.5, 19.2, 21.5, 3.2, false), rrect(7.6, 5.4, 16.4, 8.4, 0.9, true)];
            for y in [11.9, 15.1, 18.3] {
                for x in [8.4, 12.0, 15.6] {
                    v.push(dot(x, y, 1.15));
                }
            }
            v
        }
        "delete" => vec![
            shut(&[[8.4, 5.0], [20.5, 5.0], [20.5, 19.0], [8.4, 19.0], [2.5, 12.0]]),
            ln(11.4, 9.0, 17.4, 15.0),
            ln(17.4, 9.0, 11.4, 15.0),
        ],
        "swap" => vec![
            ln(8.0, 20.0, 8.0, 4.0),
            open(&[[4.0, 8.0], [8.0, 4.0], [12.0, 8.0]]),
            ln(16.0, 4.0, 16.0, 20.0),
            open(&[[12.0, 16.0], [16.0, 20.0], [20.0, 16.0]]),
        ],
        "updown" => {
            stroke = 2.0;
            vec![
                open(&[[7.5, 9.5], [12.0, 5.0], [16.5, 9.5]]),
                open(&[[7.5, 14.5], [12.0, 19.0], [16.5, 14.5]]),
            ]
        }
        "sidebar" => vec![
            rrect(2.5, 4.5, 21.5, 19.5, 3.2, false),
            ln(9.2, 4.5, 9.2, 19.5),
            ln(4.9, 8.2, 6.8, 8.2),
            ln(4.9, 11.2, 6.8, 11.2),
        ],
        "check" => vec![open(&[[4.5, 12.8], [9.5, 17.8], [19.5, 6.0]])],
        "search" => vec![ring(10.4, 10.4, 6.6), ln(15.3, 15.3, 20.8, 20.8)],
        "mic" => vec![
            rrect(8.8, 2.5, 15.2, 14.6, 3.2, false),
            arc(12.0, 11.0, 6.6, 0.0, PI),
            ln(12.0, 17.6, 12.0, 21.2),
            ln(8.6, 21.2, 15.4, 21.2),
        ],
        "basic" => {
            stroke = 1.8;
            vec![
                ln(7.0, 3.6, 7.0, 10.4),
                ln(3.6, 7.0, 10.4, 7.0),
                ln(13.6, 7.0, 20.4, 7.0),
                ln(4.4, 14.4, 9.6, 19.6),
                ln(9.6, 14.4, 4.4, 19.6),
                ln(13.6, 17.0, 20.4, 17.0),
                dot(17.0, 13.9, 1.2),
                dot(17.0, 20.1, 1.2),
            ]
        }
        "function" => {
            stroke = 1.8;
            // italic f, then (x)
            let mut f = vec![[11.4, 4.0], [10.2, 3.5], [8.9, 3.8], [8.1, 5.0], [7.7, 7.0]];
            f.extend([[5.8, 18.8], [5.3, 20.2], [4.2, 20.8], [3.0, 20.4]]);
            vec![
                open(&f),
                ln(5.4, 9.0, 10.6, 9.0),
                arc(19.0, 14.0, 7.0, deg(146.0), deg(214.0)),
                arc(15.0, 14.0, 7.0, deg(-34.0), deg(34.0)),
                ln(15.3, 11.6, 18.7, 16.4),
                ln(18.7, 11.6, 15.3, 16.4),
            ]
        }
        "mathnotes" => vec![
            open(&[[2.8, 13.0], [5.2, 11.8], [8.6, 20.2], [12.6, 3.8], [21.2, 3.8]]),
            ln(13.8, 9.2, 19.4, 15.8),
            ln(19.4, 9.2, 13.8, 15.8),
        ],
        "convert" => vec![
            ln(3.5, 7.5, 20.5, 7.5),
            open(&[[17.0, 4.0], [20.5, 7.5], [17.0, 11.0]]),
            ln(20.5, 16.5, 3.5, 16.5),
            open(&[[7.0, 13.0], [3.5, 16.5], [7.0, 20.0]]),
        ],
        "back" => {
            stroke = 2.2;
            vec![open(&[[15.0, 4.5], [7.5, 12.0], [15.0, 19.5]])]
        }
        "chevron_right" => {
            stroke = 2.2;
            vec![open(&[[9.0, 4.5], [16.5, 12.0], [9.0, 19.5]])]
        }
        "chevron_down" => {
            stroke = 2.2;
            vec![open(&[[4.5, 9.0], [12.0, 16.5], [19.5, 9.0]])]
        }
        "trash" => vec![
            ln(3.5, 6.0, 20.5, 6.0),
            open(&[[9.0, 6.0], [9.0, 4.2], [10.0, 3.2], [14.0, 3.2], [15.0, 4.2], [15.0, 6.0]]),
            open(&[[5.6, 6.0], [6.6, 19.6], [7.8, 20.8], [16.2, 20.8], [17.4, 19.6], [18.4, 6.0]]),
            ln(10.0, 10.0, 10.2, 17.0),
            ln(14.0, 10.0, 13.8, 17.0),
        ],
        "share" => vec![
            open(&[[8.6, 9.0], [6.4, 9.0], [4.8, 10.6], [4.8, 19.4], [6.4, 21.0], [17.6, 21.0], [19.2, 19.4], [19.2, 10.6], [17.6, 9.0], [15.4, 9.0]]),
            ln(12.0, 14.6, 12.0, 2.8),
            open(&[[8.2, 6.4], [12.0, 2.6], [15.8, 6.4]]),
        ],
        "ellipsis" => vec![dot(5.0, 12.0, 1.9), dot(12.0, 12.0, 1.9), dot(19.0, 12.0, 1.9)],
        "graph" => vec![
            open(&[[3.5, 3.0], [3.5, 20.5], [21.0, 20.5]]),
            open(&[[6.5, 16.0], [10.5, 10.0], [14.0, 13.5], [20.0, 5.5]]),
        ],
        "compose" => vec![
            open(&[[11.0, 4.5], [6.5, 4.5], [4.5, 6.5], [4.5, 17.5], [6.5, 19.5], [17.5, 19.5], [19.5, 17.5], [19.5, 13.0]]),
            shut(&[[9.6, 14.4], [10.4, 11.4], [18.8, 3.0], [21.0, 5.2], [12.6, 13.6]]),
        ],
        "plus" => vec![ln(12.0, 4.0, 12.0, 20.0), ln(4.0, 12.0, 20.0, 12.0)],
        "xmark" => vec![ln(5.5, 5.5, 18.5, 18.5), ln(18.5, 5.5, 5.5, 18.5)],
        "gear" => {
            stroke = 1.7;
            let mut pts = Vec::new();
            let n = 8;
            let step = 2.0 * PI / n as f32;
            for i in 0..n {
                let a = i as f32 * step;
                for (da, r) in [(-0.30, 7.4), (-0.17, 9.8), (0.17, 9.8), (0.30, 7.4)] {
                    let t = a + da;
                    pts.push([12.0 + r * t.cos(), 12.0 + r * t.sin()]);
                }
            }
            vec![shut(&pts), ring(12.0, 12.0, 3.2)]
        }
        "copy" => vec![
            rrect(3.8, 7.6, 15.6, 21.2, 2.4, false),
            open(&[[8.4, 7.4], [8.4, 5.2], [10.6, 3.0], [18.0, 3.0], [20.2, 5.2], [20.2, 14.4], [18.0, 16.6], [15.8, 16.6]]),
        ],
        "paintbrush" => vec![
            shut(&[[18.4, 2.8], [21.2, 5.6], [14.0, 14.4], [9.6, 10.0]]),
            shut(&[[8.4, 11.6], [12.4, 15.6], [11.6, 18.4], [9.0, 20.4], [3.2, 21.0], [4.8, 18.6], [5.4, 14.8]]),
        ],
        "circle" => vec![ring(12.0, 12.0, 9.2)],
        "checkcircle" => vec![ring(12.0, 12.0, 9.2), open(&[[7.8, 12.4], [10.8, 15.4], [16.4, 8.8]])],
        // Math Notes bottom bar (iOS 26)
        "checklist" => vec![
            ring(5.5, 7.0, 2.4),
            open(&[[4.4, 7.0], [5.3, 7.9], [6.8, 6.0]]),
            ring(5.5, 16.5, 2.4),
            ln(10.5, 7.0, 21.0, 7.0),
            ln(10.5, 16.5, 21.0, 16.5),
        ],
        "paperclip" => vec![open(&[
            [15.5, 7.0], [8.6, 14.6], [8.3, 16.2], [9.2, 17.3], [10.8, 17.4], [18.4, 9.6], [19.0, 7.0], [17.8, 4.8],
            [15.6, 3.9], [13.2, 4.6], [5.2, 12.8], [4.0, 15.6], [4.6, 18.6], [6.6, 20.4], [9.6, 20.6], [12.4, 19.2], [18.6, 12.8],
        ])],
        "markup" => vec![
            ring(12.0, 12.0, 9.2),
            shut(&[[9.0, 16.6], [9.6, 13.8], [14.8, 8.6], [17.2, 11.0], [12.0, 16.2]]),
            ln(13.6, 9.8, 16.0, 12.2),
        ],
        _ => return None,
    };
    Some(Icon { prims, stroke })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(p: &Prim) -> Vec<[f32; 2]> {
        match p {
            Prim::Line { a, b } => vec![*a, *b],
            Prim::Poly { pts, .. } => pts.clone(),
            Prim::Circle { c, r, .. } => vec![[c[0] - r, c[1] - r], [c[0] + r, c[1] + r]],
            Prim::Arc { c, r, a0, a1 } => (0..=16)
                .map(|i| {
                    let a = a0 + (a1 - a0) * i as f32 / 16.0;
                    [c[0] + r * a.cos(), c[1] + r * a.sin()]
                })
                .collect(),
            Prim::RRect { min, max, .. } => vec![*min, *max],
        }
    }

    #[test]
    fn all_icons_fit_the_box() {
        for n in NAMES {
            let i = icon(n).unwrap_or_else(|| panic!("missing {n}"));
            assert!(!i.prims.is_empty(), "{n}");
            for p in &i.prims {
                for [x, y] in pts(p) {
                    assert!((-0.5..=24.5).contains(&x) && (-0.5..=24.5).contains(&y), "{n}: {x},{y}");
                }
            }
        }
        assert!(icon("nope").is_none());
    }
}
