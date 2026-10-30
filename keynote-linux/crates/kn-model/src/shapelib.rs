//! The shape library of the toolbar's Shape popover (all three apps): filled shapes by category,
//! from Phosphor Icons (MIT, data/shapelib-LICENSE.txt). Each one is inserted as an ordinary
//! freeform shape (a bezier path source), so Keynote, Numbers and Pages open it like any other.
//! data/shapelib.txt: `category <TAB> name <TAB> w h <TAB> path`, the path in absolute
//! M / L / Q / C / Z commands, trimmed to the shape's bounds.
use std::sync::OnceLock;

use kn_format::Msg;

use crate::{Path, PathEl, Point};

static DATA: &str = include_str!("../data/shapelib.txt");

/// One library shape.
#[derive(Clone, Debug)]
pub struct LibShape {
    pub category: &'static str,
    pub name: &'static str,
    pub w: f32,
    pub h: f32,
    path: &'static str,
}

/// Every library shape, in the popover's order (category, then name).
pub fn shapes() -> &'static [LibShape] {
    static ALL: OnceLock<Vec<LibShape>> = OnceLock::new();
    ALL.get_or_init(|| {
        DATA.lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .filter_map(|l| {
                let mut f = l.split('\t');
                let (category, name, size, path) = (f.next()?, f.next()?, f.next()?, f.next()?);
                let (w, h) = size.split_once(' ')?;
                Some(LibShape { category, name, w: w.parse().ok()?, h: h.parse().ok()?, path })
            })
            .collect()
    })
}

/// The categories, in order.
pub fn categories() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for s in shapes() {
        if out.last() != Some(&s.category) && !out.contains(&s.category) {
            out.push(s.category);
        }
    }
    out
}

/// The shapes whose name contains every word of `query` (case-insensitive), any category.
pub fn search(query: &str) -> Vec<&'static LibShape> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    shapes().iter().filter(|s| words.iter().all(|w| s.name.to_lowercase().contains(w.as_str()))).collect()
}

impl LibShape {
    /// The outline in its own coordinates (0..w, 0..h).
    pub fn outline(&self) -> Path {
        let mut els = Vec::new();
        let mut nums: Vec<f32> = Vec::new();
        let mut cmd = ' ';
        let flush = |cmd: char, nums: &mut Vec<f32>, els: &mut Vec<PathEl>| {
            let p = |i: usize| Point::new(nums[i], nums[i + 1]);
            match (cmd, nums.len()) {
                ('M', 2) => els.push(PathEl::Move(p(0))),
                ('L', 2) => els.push(PathEl::Line(p(0))),
                ('Q', 4) => els.push(PathEl::Quad(p(0), p(2))),
                ('C', 6) => els.push(PathEl::Cubic(p(0), p(2), p(4))),
                _ => {}
            }
            nums.clear();
        };
        // (numbers are separated by spaces; a command letter ends the previous command)
        let mut num = String::new();
        for c in self.path.chars().chain(std::iter::once(' ')) {
            if c.is_ascii_digit() || c == '.' || c == '-' {
                num.push(c);
                continue;
            }
            if let Ok(v) = num.parse::<f32>() {
                nums.push(v);
            }
            num.clear();
            if c.is_ascii_alphabetic() {
                flush(cmd, &mut nums, &mut els);
                cmd = c;
                if c == 'Z' {
                    els.push(PathEl::Close);
                }
            }
        }
        flush(cmd, &mut nums, &mut els);
        Path { els }
    }

    /// A TSD.PathSourceArchive for inserting it (a bezier path at its natural size).
    pub fn path_source(&self) -> Msg {
        let el = |ty: &str, pts: &[Point]| {
            let mut e = Msg::new("TSP.Path.Element");
            e.set_enum("type", ty);
            for q in pts {
                let mut p = Msg::new("TSP.Point");
                p.set_f32("x", q.x);
                p.set_f32("y", q.y);
                e.push_msg("points", p);
            }
            e
        };
        let mut path = Msg::new("TSP.Path");
        for e in self.outline().els {
            path.push_msg(
                "elements",
                match e {
                    PathEl::Move(a) => el("moveTo", &[a]),
                    PathEl::Line(a) => el("lineTo", &[a]),
                    PathEl::Quad(a, b) => el("quadCurveTo", &[a, b]),
                    PathEl::Cubic(a, b, c) => el("curveTo", &[a, b, c]),
                    PathEl::Close => el("closeSubpath", &[]),
                },
            );
        }
        let mut ps = Msg::new("TSD.PathSourceArchive");
        ps.set_bool("horizontalFlip", false);
        ps.set_bool("verticalFlip", false);
        let b = ps.msg_mut_or_create("bezier_path_source");
        let ns = b.msg_mut_or_create("naturalSize");
        ns.set_f32("width", self.w);
        ns.set_f32("height", self.h);
        b.push_msg("path", path);
        ps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_reads_and_builds() {
        let all = shapes();
        assert!(all.len() > 1000, "{} shapes", all.len());
        assert_eq!(categories().first(), Some(&"Objects"));
        assert!(!categories().contains(&"Brands"));
        let cat = search("cat").into_iter().find(|s| s.name == "Cat").expect("a cat");
        let o = cat.outline();
        assert!(matches!(o.els.first(), Some(PathEl::Move(_))));
        assert!(o.els.iter().any(|e| matches!(e, PathEl::Cubic(..))));
        assert!(o.els.iter().any(|e| matches!(e, PathEl::Close)));
        // every point inside the natural size
        for s in all.iter().take(200) {
            for e in s.outline().els {
                let pts: Vec<Point> = match e {
                    PathEl::Move(a) | PathEl::Line(a) => vec![a],
                    PathEl::Quad(a, b) => vec![a, b],
                    PathEl::Cubic(a, b, c) => vec![a, b, c],
                    PathEl::Close => vec![],
                };
                for p in pts {
                    assert!(p.x >= -0.2 && p.y >= -0.2 && p.x <= s.w + 0.2 && p.y <= s.h + 0.2, "{} {p:?}", s.name);
                }
            }
        }
        // and it round-trips through the path source the shape is saved with
        let back = crate::path::from_path_source(&cat.path_source(), cat.w, cat.h);
        assert_eq!(back.els.len(), o.els.len());
    }
}
