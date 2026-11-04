//! Where everything goes for a given window: phone portrait (4×5 keys, or the 6-column
//! scientific keys above smaller basic keys), phone landscape (10×5 scientific), tablets (a
//! history sidebar beside the keys), and the fixed-shape classic calculators. Both apps draw
//! from this, so they match at every size. Units are logical pixels (dp).

use crate::calc::Display;
use crate::theme::{Family, Shape, Theme};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

/// A piece of a key label: `v` = 1 raised, -1 lowered, 0 normal.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Lab {
    pub t: String,
    pub v: i8,
}

fn l(t: &str) -> Vec<Lab> {
    vec![Lab { t: t.into(), v: 0 }]
}

fn lv(parts: &[(&str, i8)]) -> Vec<Lab> {
    parts.iter().map(|(t, v)| Lab { t: (*t).into(), v: *v }).collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct KeyBox {
    /// what the key does (calc.rs press ids; "acback" and "calc" are resolved by the session)
    pub id: String,
    pub label: Vec<Lab>,
    pub icon: Option<&'static str>,
    /// digit / func / op / sci / eq / mem — picks the theme's KeyStyle
    pub kind: &'static str,
    pub rect: Rect,
    pub radius: f32,
    pub font: f32,
    /// lit: 2nd on, the waiting operator on classic themes
    pub active: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolButton {
    pub id: &'static str,
    pub icon: &'static str,
    pub rect: Rect,
}

#[derive(Clone, Debug, Serialize)]
pub struct Layout {
    pub w: f32,
    pub h: f32,
    pub keys: Vec<KeyBox>,
    pub toolbar: Vec<ToolButton>,
    /// the area of the numbers; text is right-aligned and sits on its bottom edge
    pub display: Rect,
    pub main_font: f32,
    pub expr_font: f32,
    /// right edge inset of the numbers inside `display`
    pub text_inset: f32,
    /// Convert mode: the two value rows and the swap button
    pub conv_top: Option<Rect>,
    pub conv_bottom: Option<Rect>,
    pub conv_swap: Option<Rect>,
    /// the history sidebar on tablets
    pub sidebar: Option<Rect>,
    /// the device panel on classic themes
    pub body: Option<Rect>,
    /// the panel's inner margin around display and keys
    pub body_inner: f32,
    /// the 1984 title bar's height at the panel's top
    pub title_h: f32,
    pub scientific: bool,
    pub landscape: bool,
    pub tablet: bool,
    pub max_digits: usize,
    /// where the mode menu opens from
    pub menu_anchor: Rect,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Insets {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize, Default)]
pub enum Mode {
    #[default]
    Basic,
    Scientific,
    Notes,
}

pub struct LayoutIn<'a> {
    pub w: f32,
    pub h: f32,
    pub insets: Insets,
    pub mode: Mode,
    pub convert: bool,
    pub sidebar_open: bool,
    pub theme: &'a Theme,
    pub disp: &'a Display,
    pub decimal: char,
}

const SCI6: [[&str; 6]; 5] = [
    ["lp", "rp", "mc", "mp", "mm", "mr"],
    ["2nd", "sq", "cube", "pow", "exp", "p10"],
    ["recip", "sqrt", "cbrt", "root", "ln", "log10"],
    ["fact", "sin", "cos", "tan", "e", "ee"],
    ["rand", "sinh", "cosh", "tanh", "pi", "rad"],
];

/// The scientific keys in 10 columns (squarish screens): controls and memory, powers and
/// roots, then trigonometry and logs.
const SCI10: [[&str; 10]; 3] = [
    ["2nd", "lp", "rp", "mc", "mp", "mm", "mr", "rad", "rand", "pi"],
    ["sq", "cube", "pow", "exp", "p10", "recip", "sqrt", "cbrt", "root", "e"],
    ["fact", "sin", "cos", "tan", "sinh", "cosh", "tanh", "ln", "log10", "ee"],
];

fn basic_rows(f: Family) -> [[&'static str; 4]; 5] {
    let first = if f == Family::Modern26 { ["back", "ac", "pct", "div"] } else { ["acback", "neg", "pct", "div"] };
    let last = if f == Family::Modern26 { ["neg", "0", "dot", "eq"] } else { ["calc", "0", "dot", "eq"] };
    [first, ["7", "8", "9", "mul"], ["4", "5", "6", "sub"], ["1", "2", "3", "add"], last]
}

/// The basic keys in 5 columns × 4 rows for squarish screens (foldables, iPad): the function
/// keys move into a fourth column beside the digits, as iPadOS and the iPhone Fold do.
fn square_rows(f: Family) -> [[&'static str; 5]; 4] {
    let (a, b, c, d) = if f == Family::Modern26 { ("back", "ac", "pct", "neg") } else { ("acback", "neg", "pct", "calc") };
    [["7", "8", "9", a, "div"], ["4", "5", "6", b, "mul"], ["1", "2", "3", c, "sub"], [d, "0", "dot", "eq", "add"]]
}

/// `square_rows` as cells, each key `span` columns wide.
fn square_cells(f: Family, span: u8) -> Vec<Cell> {
    let mut v = Vec::new();
    for (r, row) in square_rows(f).iter().enumerate() {
        for (c, id) in row.iter().enumerate() {
            v.push((*id, c as u8 * span, r as u8, span, 1));
        }
    }
    v
}

/// Landscape scientific rows (10 columns).
fn wide_rows(f: Family) -> Vec<Vec<&'static str>> {
    let b = basic_rows(f);
    let mut rows: Vec<Vec<&str>> = (0..5).map(|r| SCI6[r].iter().chain(b[r].iter()).copied().collect()).collect();
    if f == Family::Modern18 {
        // iOS 18: [calc] sinh cosh tanh π Rad Rand 0 . =
        rows[4] = vec!["calc", "sinh", "cosh", "tanh", "pi", "rad", "rand", "0", "dot", "eq"];
    }
    rows
}

/// Classic grids: (id, col, row, col span, row span), columns, rows, panel aspect (w/h).
fn classic_grid(f: Family) -> (Vec<(&'static str, u8, u8, u8, u8)>, u8, u8, f32) {
    let g = |ids: &[&'static str], row: u8| -> Vec<(&'static str, u8, u8, u8, u8)> { ids.iter().enumerate().map(|(c, id)| (*id, c as u8, row, 1, 1)).collect() };
    match f {
        Family::Mac2019 => {
            let mut v = g(&["ac", "neg", "pct", "div"], 0);
            v.extend(g(&["7", "8", "9", "mul"], 1));
            v.extend(g(&["4", "5", "6", "sub"], 2));
            v.extend(g(&["1", "2", "3", "add"], 3));
            v.extend([("0", 0, 4, 2, 1), ("dot", 2, 4, 1, 1), ("eq", 3, 4, 1, 1)]);
            (v, 4, 5, 0.62)
        }
        Family::Iphone2007 | Family::Aqua2001 => {
            let mut v = g(&["mc", "mp", "mm", "mr"], 0);
            v.extend(g(&["ac", "neg", "div", "mul"], 1));
            v.extend(g(&["7", "8", "9", "sub"], 2));
            v.extend(g(&["4", "5", "6", "add"], 3));
            v.extend([("1", 0, 4, 1, 1), ("2", 1, 4, 1, 1), ("3", 2, 4, 1, 1), ("eq", 3, 4, 1, 2), ("0", 0, 5, 2, 1), ("dot", 2, 5, 1, 1)]);
            (v, 4, 6, if f == Family::Iphone2007 { 0.66 } else { 0.72 })
        }
        _ => {
            let mut v = g(&["ac", "ce", "eq", "mul"], 0);
            v.extend(g(&["7", "8", "9", "div"], 1));
            v.extend(g(&["4", "5", "6", "sub"], 2));
            v.extend([("1", 0, 3, 1, 1), ("2", 1, 3, 1, 1), ("3", 2, 3, 1, 1), ("add", 3, 3, 1, 2), ("0", 0, 4, 2, 1), ("dot", 2, 4, 1, 1)]);
            (v, 4, 5, 0.78)
        }
    }
}

fn kind(id: &str) -> &'static str {
    match id {
        "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "dot" => "digit",
        "add" | "sub" | "mul" | "div" => "op",
        "eq" => "eq",
        "ac" | "acback" | "back" | "neg" | "pct" | "ce" => "func",
        "calc" => "digit",
        "mc" | "mp" | "mm" | "mr" => "mem",
        _ => "sci",
    }
}

/// The label of a key in the current state.
pub fn label(id: &str, inp: &LayoutIn) -> (Vec<Lab>, Option<&'static str>) {
    let d = inp.disp;
    let s2 = d.second;
    let ascii = inp.theme.ascii_ops;
    let classic = inp.theme.family.classic();
    let lab = match id {
        "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => l(id),
        "dot" => l(&inp.decimal.to_string()),
        "add" => l("+"),
        "sub" => l(if ascii { "-" } else { "−" }),
        "mul" => l(if ascii { "*" } else { "×" }),
        "div" => l(if ascii { "/" } else { "÷" }),
        "eq" => l("="),
        "pct" => l("%"),
        "neg" => {
            if classic {
                l("±")
            } else {
                lv(&[("+", 1), ("/", 0), ("−", -1)])
            }
        }
        "ac" => {
            if inp.theme.family == Family::Mac1984 || matches!(inp.theme.family, Family::Iphone2007 | Family::Aqua2001) {
                l("C")
            } else {
                l(d.clear_label)
            }
        }
        "ce" => l("E"),
        "acback" => {
            if d.first_key == "back" {
                return (vec![], Some("delete"));
            }
            l("AC")
        }
        "back" => return (vec![], Some("delete")),
        "calc" => return (vec![], Some("calculator")),
        "lp" => l("("),
        "rp" => l(")"),
        "mc" => l(if classic && inp.theme.family == Family::Aqua2001 { "MC" } else { "mc" }),
        "mp" => l(if classic && inp.theme.family == Family::Aqua2001 { "M+" } else { "m+" }),
        "mm" => l(if classic && inp.theme.family == Family::Aqua2001 { "M-" } else { "m-" }),
        "mr" => l(if classic && inp.theme.family == Family::Aqua2001 { "MR" } else { "mr" }),
        "2nd" => lv(&[("2", 0), ("nd", 1)]),
        "sq" => lv(&[("x", 0), ("2", 1)]),
        "cube" => lv(&[("x", 0), ("3", 1)]),
        "pow" => lv(&[("x", 0), ("y", 1)]),
        "exp" => {
            if s2 {
                lv(&[("y", 0), ("x", 1)])
            } else {
                lv(&[("e", 0), ("x", 1)])
            }
        }
        "p10" => {
            if s2 {
                lv(&[("2", 0), ("x", 1)])
            } else {
                lv(&[("10", 0), ("x", 1)])
            }
        }
        "recip" => lv(&[("1", 1), ("/", 0), ("x", -1)]),
        "sqrt" => lv(&[("2", 1), ("√x", 0)]),
        "cbrt" => lv(&[("3", 1), ("√x", 0)]),
        "root" => lv(&[("y", 1), ("√x", 0)]),
        "ln" => {
            if s2 {
                lv(&[("log", 0), ("y", -1)])
            } else {
                l("ln")
            }
        }
        "log10" => {
            if s2 {
                lv(&[("log", 0), ("2", -1)])
            } else {
                lv(&[("log", 0), ("10", -1)])
            }
        }
        "fact" => l("x!"),
        "sin" | "cos" | "tan" | "sinh" | "cosh" | "tanh" => {
            if s2 {
                lv(&[(id, 0), ("-1", 1)])
            } else {
                l(id)
            }
        }
        "e" => l("e"),
        "ee" => l("EE"),
        "rand" => l("Rand"),
        "pi" => l("π"),
        "rad" => l(if d.rad { "Deg" } else { "Rad" }),
        _ => l(id),
    };
    (lab, None)
}

fn radius(shape: Shape, w: f32, h: f32) -> f32 {
    match shape {
        Shape::Round => w.min(h) / 2.0,
        Shape::RoundRect(f) => w.min(h) * f,
        Shape::Rect => 0.0,
    }
}

fn push_key(out: &mut Vec<KeyBox>, id: &str, r: Rect, inp: &LayoutIn) {
    let (label, icon) = label(id, inp);
    let d = inp.disp;
    let t = inp.theme;
    let active = match id {
        "2nd" => d.second,
        "add" | "sub" | "mul" | "div" => t.family.classic() && d.pending_op == Some(id),
        _ => false,
    };
    // iOS 26 moved +/- to the bottom row, where it takes the digit keys' colour
    let k = if id == "neg" && t.family == Family::Modern26 {
        "digit"
    } else if id == "dot" && t.dot_op {
        "op"
    } else {
        kind(id)
    };
    // labels follow the key: big keys 45% of their height, scientific and memory keys smaller
    let font = if k == "sci" || k == "mem" {
        (r.h * 0.38).min(r.w * 0.3)
    } else if t.family.classic() {
        r.h.min(r.w) * if t.family == Family::Mac1984 { 0.4 } else { 0.43 }
    } else {
        r.h.min(r.w * 0.8) * 0.45
    };
    out.push(KeyBox { id: id.into(), label, icon, kind: k, rect: r, radius: radius(t.shape, r.w, r.h), font, active });
}

/// A key's place in a grid: column, row, column span, row span.
type Cell = (&'static str, u8, u8, u8, u8);

fn cells_of(rows: &[Vec<&'static str>]) -> Vec<Cell> {
    let mut v = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        for (c, id) in row.iter().enumerate() {
            v.push((*id, c as u8, r as u8, 1, 1));
        }
    }
    v
}

/// The 30 scientific keys in `rows` rows (6 columns for 5 rows, 5 columns for 6 rows).
fn sci_cells(rows: u8) -> (Vec<Cell>, u8) {
    let ids: Vec<&'static str> = SCI6.iter().flat_map(|r| r.iter().copied()).collect();
    let cols = 30u8.div_ceil(rows);
    (ids.iter().enumerate().map(|(i, id)| (*id, i as u8 % cols, i as u8 / cols, 1, 1)).collect(), cols)
}

/// The basic keypad of a theme: cells, columns, rows. Beside the scientific keys (which have
/// their own mc m+ m- mr) a classic memory row is left out.
fn basic_cells(f: Family, scientific: bool) -> (Vec<Cell>, u8, u8) {
    if f.classic() {
        let (c, cols, rows, _) = classic_grid(f);
        if scientific && c.iter().any(|k| k.0 == "mc") {
            let c = c.into_iter().filter(|k| k.2 > 0).map(|(id, x, y, cs, rs)| (id, x, y - 1, cs, rs)).collect();
            return (c, cols, rows - 1);
        }
        (c, cols, rows)
    } else {
        let rows: Vec<Vec<&'static str>> = basic_rows(f).iter().map(|r| r.to_vec()).collect();
        (cells_of(&rows), 4, 5)
    }
}

/// Key size and grid size for `cols`×`rows` in `aw`×`ah`. Round keys stay circles (or
/// capsules up to 1.7× wide where `capsules`); classic keys may stretch a little.
struct Fit {
    kw: f32,
    kh: f32,
    gx: f32,
    gy: f32,
}

impl Fit {
    fn w(&self, cols: u8) -> f32 {
        cols as f32 * self.kw + (cols as f32 - 1.0) * self.gx
    }
    fn h(&self, rows: u8) -> f32 {
        rows as f32 * self.kh + (rows as f32 - 1.0) * self.gy
    }
}

fn fit(t: &Theme, cols: u8, rows: u8, aw: f32, ah: f32, capsules: bool) -> Fit {
    let g = t.gap;
    let (c, r) = (cols as f32, rows as f32);
    let kw0 = aw / (c + (c - 1.0) * g);
    let kh0 = ah / (r + (r - 1.0) * g);
    let (kw, kh) = match t.shape {
        Shape::Round => {
            let kh = kw0.min(kh0);
            // capsules take the width the (height-based) gaps leave
            (if capsules { ((aw - (c - 1.0) * g * kh) / c).min(kh * 1.7) } else { kh }, kh)
        }
        _ => {
            let kh = kh0.min(kw0 * 1.1);
            (kw0.min(kh * 2.0), kh)
        }
    };
    let gx = if matches!(t.shape, Shape::Round) { g * kh } else { g * kw.min(kh) };
    Fit { kw, kh, gx, gy: g * kh.min(kw) }
}

/// Places cells with their top-left at (x, y).
fn place(out: &mut Vec<KeyBox>, cells: &[Cell], x: f32, y: f32, f: &Fit, col0: u8, inp: &LayoutIn) {
    for (id, c, r, cs, rs) in cells {
        let c = c + col0;
        let rect = Rect::new(
            x + c as f32 * (f.kw + f.gx),
            y + *r as f32 * (f.kh + f.gy),
            *cs as f32 * f.kw + (*cs as f32 - 1.0) * f.gx,
            *rs as f32 * f.kh + (*rs as f32 - 1.0) * f.gy,
        );
        push_key(out, id, rect, inp);
    }
}

/// The keypad arrangements a window can take.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Arr {
    Basic,
    /// scientific keys left of the basic ones (landscape, iPad)
    Wide,
    /// scientific keys above smaller basic keys (iPhone portrait)
    Stacked,
    /// squarish screens: the scientific keys in 10 columns above the 5×4 basic keys, each
    /// basic key two columns wide, all filling the width
    Square,
}

pub fn layout(inp: &LayoutIn) -> Layout {
    let (w, h) = (inp.w.max(1.0), inp.h.max(1.0));
    let ins = inp.insets;
    let t = inp.theme;
    let fam = t.family;
    let tablet = w.min(h) >= 600.0;
    let landscape = w > h * 1.15;
    let mut out = Layout {
        w,
        h,
        keys: vec![],
        toolbar: vec![],
        display: Rect::default(),
        main_font: 80.0,
        expr_font: 28.0,
        text_inset: 2.0,
        conv_top: None,
        conv_bottom: None,
        conv_swap: None,
        sidebar: None,
        body: None,
        body_inner: 0.0,
        title_h: 0.0,
        scientific: false,
        landscape,
        tablet,
        max_digits: 9,
        menu_anchor: Rect::default(),
    };

    // one padding for every edge; toolbar buttons are always the same size
    let pad = (w.min(h) * 0.035).clamp(12.0, 24.0);
    let bsize = 53.0;
    let (left, right, top0, bottom0) = (ins.left + pad, w - ins.right - pad, ins.top + pad, h - ins.bottom - pad);
    let mut cx = left;
    if tablet && inp.sidebar_open && w >= 820.0 {
        let sw = (w * 0.27).clamp(260.0, 340.0);
        let card = Rect::new(left, top0, sw, bottom0 - top0);
        out.sidebar = Some(card);
        cx = card.x + card.w + pad;
    }
    let cw = right - cx;
    let bar_y = top0;
    match out.sidebar {
        // the toggle sits in the card's header while it is open
        Some(sb) => out.toolbar.push(ToolButton { id: "history", icon: "sidebar", rect: Rect::new(sb.x + sb.w - 8.0 - bsize, sb.y + 8.0, bsize, bsize) }),
        None => {
            let icon = match (fam, tablet) {
                (_, true) => "sidebar",
                (Family::Modern26, false) => "clock",
                _ => "list",
            };
            out.toolbar.push(ToolButton { id: "history", icon, rect: Rect::new(left, bar_y, bsize, bsize) });
        }
    }
    if fam != Family::Modern18 || inp.mode == Mode::Notes {
        let r = Rect::new(right - bsize, bar_y, bsize, bsize);
        out.toolbar.push(ToolButton { id: "mode", icon: "calculator", rect: r });
        out.menu_anchor = r;
    }
    let top = bar_y + bsize + pad * 0.5;
    let bottom = bottom0;
    let ch = bottom - top;

    // which arrangement: phones in landscape are always scientific; Scientific takes whichever
    // of side-by-side or stacked gives the bigger keys in the space left beside the sidebar
    let classic = fam.classic();
    let capsules = !tablet;
    let sci = inp.mode == Mode::Scientific || (landscape && !tablet);
    let (bcells, bcols, brows) = basic_cells(fam, sci);
    // a squarish space (a foldable open, an iPad) takes the 5×4 keys stretched to the full width
    // instead of a narrow 4×5 column in the middle
    let square = !classic && ch < cw * 1.5;
    let disp_min = |frac: f32| (ch * frac).max(if classic { 80.0 } else { 96.0 });
    let arr = if !sci {
        Arr::Basic
    } else if square && ch >= cw * 0.85 {
        Arr::Square
    } else {
        let (_, scols) = sci_cells(brows);
        let wide = fit(t, scols + bcols, brows, cw, ch - disp_min(0.2), capsules);
        let stacked = fit(t, 6, 10, cw, ch - disp_min(0.16), false);
        if !tablet && landscape || wide.kh >= stacked.kh * 0.85 {
            Arr::Wide
        } else {
            Arr::Stacked
        }
    };
    out.scientific = arr != Arr::Basic;

    // classic calculators sit in a device panel around their display and keys
    let inner = if classic && fam != Family::Mac2019 { (cw.min(ch) * 0.04).clamp(8.0, 22.0) } else { 0.0 };
    let title_h = if fam == Family::Mac1984 { (ch * 0.045).clamp(18.0, 26.0) } else { 0.0 };
    out.body_inner = inner;
    out.title_h = title_h;
    let chrome_h = 2.0 * inner + title_h;
    let disp_gap = if classic { inner.max(4.0) } else { 0.0 };

    let (grid, kh_main) = match arr {
        Arr::Basic if square => {
            let dh = disp_min(if tablet { 0.3 } else { 0.28 });
            let f = fit(t, 5, 4, cw, ch - dh, true);
            let (gw, gh) = (f.w(5), f.h(4));
            let x = cx + (cw - gw) / 2.0;
            let y = bottom - gh;
            place(&mut out.keys, &square_cells(fam, 1), x, y, &f, 0, inp);
            (Rect::new(x, y, gw, gh), f.kh)
        }
        Arr::Basic => {
            let dh = if classic { disp_min(0.2) } else { disp_min(if tablet { 0.3 } else { 0.28 }) };
            let f = fit(t, bcols, brows, cw - 2.0 * inner, ch - dh - chrome_h - disp_gap, capsules && !classic && landscape);
            let gw = f.w(bcols);
            let gh = f.h(brows);
            let x = cx + (cw - gw) / 2.0;
            let y = bottom - inner - gh;
            place(&mut out.keys, &bcells, x, y, &f, 0, inp);
            (Rect::new(x, y, gw, gh), f.kh)
        }
        Arr::Wide => {
            let (scells, scols) = sci_cells(brows);
            let cols = scols + bcols;
            let f = fit(t, cols, brows, cw - 2.0 * inner, ch - disp_min(0.2) - chrome_h - disp_gap, capsules);
            let gw = f.w(cols);
            let gh = f.h(brows);
            let x = cx + (cw - gw) / 2.0;
            let y = bottom - inner - gh;
            if fam == Family::Modern18 {
                place(&mut out.keys, &cells_of(&wide_rows(fam)), x, y, &f, 0, inp);
            } else {
                place(&mut out.keys, &scells, x, y, &f, 0, inp);
                place(&mut out.keys, &bcells, x, y, &f, scols, inp);
            }
            (Rect::new(x, y, gw, gh), f.kh)
        }
        Arr::Stacked => {
            // 6 round scientific keys per row above the basic keys, all rows the same height
            let rows = 5 + brows;
            let mut f = fit(t, 6, rows, cw - 2.0 * inner, ch - disp_min(0.16) - chrome_h - disp_gap, false);
            let kh = f.kh;
            let gw = (cw - 2.0 * inner).min(f.w(6) * 1.25);
            let x = cx + (cw - gw) / 2.0;
            let gh = f.h(rows);
            let y = bottom - inner - gh;
            let sgx = (gw - 6.0 * f.kw) / 5.0;
            let sf = Fit { kw: f.kw, kh, gx: sgx, gy: f.gy };
            place(&mut out.keys, &sci_cells(5).0, x, y, &sf, 0, inp);
            f.gx = f.gy.max(sgx * 0.6).min(sgx);
            f.kw = (gw - (bcols as f32 - 1.0) * f.gx) / bcols as f32;
            place(&mut out.keys, &bcells, x, y + 5.0 * (kh + f.gy), &f, 0, inp);
            (Rect::new(x, y, gw, gh), kh)
        }
        Arr::Square => {
            // the 3 scientific rows keep round keys; the 4 basic rows take what height is left
            // (up to 1.4× taller) so the keys reach up the screen instead of hugging the bottom
            let avail = ch - disp_min(0.16);
            let f = fit(t, 10, 7, cw, avail, true);
            let hb = (f.kh + (avail - f.h(7)) / 4.0).min(f.kh * 1.4).min(2.0 * f.kw + f.gx);
            let gw = f.w(10);
            let gh = 3.0 * (f.kh + f.gy) + 4.0 * hb + 3.0 * f.gy;
            let x = cx + (cw - gw) / 2.0;
            let y = bottom - gh;
            place(&mut out.keys, &cells_of(&SCI10.iter().map(|r| r.to_vec()).collect::<Vec<_>>()), x, y, &f, 0, inp);
            let bf = Fit { kw: f.kw, kh: hb, gx: f.gx, gy: f.gy };
            place(&mut out.keys, &square_cells(fam, 2), x, y + 3.0 * (f.kh + f.gy), &bf, 0, inp);
            (Rect::new(x, y, gw, gh), hb)
        }
    };

    // the numbers end at the keys' right edge; on modern themes the display's bottom edge is
    // the number's BASELINE, which sits ~15–21 pt above the keys at any key size (iPhone and
    // iPad screenshots: 0.2 of the number's size)
    let disp_bottom = if classic {
        grid.y - disp_gap
    } else {
        let f = main_font_for(arr, kh_main, grid.y - top, tablet);
        grid.y - (f * 0.22).clamp(14.0, 26.0)
    };
    let disp_top = if classic {
        let dh = (disp_bottom - top - inner - title_h).min(grid.h * if arr == Arr::Basic { 0.32 } else { 0.4 }).max(40.0);
        disp_bottom - dh
    } else {
        top
    };
    out.display = Rect::new(grid.x, disp_top, grid.w, disp_bottom - disp_top);
    if classic {
        let b = Rect::new(grid.x - inner, disp_top - inner - title_h, grid.w + 2.0 * inner, grid.y + grid.h + inner - (disp_top - inner - title_h));
        // centred in the space when the keys could not grow taller
        let shift = -((b.y - top).max(0.0) * 0.5);
        let b = Rect::new(b.x, b.y + shift, b.w, b.h);
        if shift != 0.0 {
            out.display.y += shift;
            for k in out.keys.iter_mut() {
                k.rect.y += shift;
            }
        }
        out.body = Some(b);
    }

    let d = out.display;
    out.text_inset = if classic { (d.h * 0.12).clamp(6.0, 16.0) } else { 2.0 };
    out.max_digits = if out.scientific || tablet { 16 } else { 9 };
    out.main_font = if classic { (d.h * if t.display.boxed { 0.5 } else { 0.62 }).max(24.0) } else { main_font_for(arr, kh_main, grid.y - top, tablet) };
    out.expr_font = (out.main_font * 0.36).clamp(12.0, 40.0);
    if inp.convert {
        convert_rows(&mut out, classic);
    }
    if fam == Family::Modern18 && inp.mode != Mode::Notes {
        if let Some(k) = out.keys.iter().find(|k| k.id == "calc") {
            out.menu_anchor = k.rect;
        }
    }
    out
}

/// The big number's size: ~1.1 key heights on the basic keypad, less when space is short.
fn main_font_for(arr: Arr, kh: f32, space: f32, tablet: bool) -> f32 {
    let f = if arr == Arr::Basic { (kh * 1.12).min(space * 0.55).min(if tablet { 120.0 } else { 96.0 }) } else { (kh * 1.3).min(space * 0.5) };
    f.max(24.0)
}

fn convert_rows(out: &mut Layout, classic: bool) {
    let d = out.display;
    if classic {
        let f = d.h * 0.32;
        out.main_font = f;
        out.conv_top = Some(Rect::new(d.x, d.y + d.h * 0.06, d.w, d.h * 0.44));
        out.conv_bottom = Some(Rect::new(d.x, d.y + d.h * 0.5, d.w, d.h * 0.44));
        out.conv_swap = Some(Rect::new(d.x + 4.0, d.y + d.h * 0.5 - 14.0, 28.0, 28.0));
        return;
    }
    let f = (out.main_font * 0.78).min(d.h * 0.36).max(24.0);
    out.main_font = f;
    let row_h = f * 1.25;
    let bottom = Rect::new(d.x, d.y + d.h - row_h, d.w, row_h);
    let top = Rect::new(d.x, bottom.y - row_h - f * 0.22, d.w, row_h);
    out.conv_top = Some(top);
    out.conv_bottom = Some(bottom);
    out.conv_swap = Some(Rect::new(d.x - 4.0, bottom.y - f * 0.11 - 18.0, 36.0, 36.0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::Calc;
    use crate::format::NumberStyle;
    use crate::theme::theme;

    fn lay(w: f32, h: f32, theme_id: &str, mode: Mode) -> Layout {
        let t = theme(theme_id);
        let d = Calc::default().display(NumberStyle::Comma, false);
        layout(&LayoutIn { w, h, insets: Insets { top: 47.0, bottom: 34.0, left: 0.0, right: 0.0 }, mode, convert: false, sidebar_open: true, theme: &t, disp: &d, decimal: '.' })
    }

    fn no_overlap(l: &Layout) {
        for (i, a) in l.keys.iter().enumerate() {
            assert!(a.rect.x >= -0.5 && a.rect.x + a.rect.w <= l.w + 0.5, "{} off screen", a.id);
            assert!(a.rect.y >= l.display.y - 0.5 && a.rect.y + a.rect.h <= l.h + 0.5, "{} off screen", a.id);
            for b in &l.keys[i + 1..] {
                let o = a.rect.x < b.rect.x + b.rect.w - 0.5 && b.rect.x < a.rect.x + a.rect.w - 0.5 && a.rect.y < b.rect.y + b.rect.h - 0.5 && b.rect.y < a.rect.y + a.rect.h - 0.5;
                assert!(!o, "{} overlaps {}", a.id, b.id);
            }
        }
    }

    #[test]
    fn phone_portrait_basic() {
        let l = lay(393.0, 852.0, "ios26", Mode::Basic);
        assert_eq!(l.keys.len(), 20);
        let k = &l.keys[0];
        assert!((k.rect.w - k.rect.h).abs() < 0.5, "basic keys are circles");
        assert!(k.rect.w > 70.0 && k.rect.w < 90.0);
        no_overlap(&l);
    }

    #[test]
    fn phone_portrait_scientific_and_landscape() {
        let l = lay(393.0, 852.0, "ios26", Mode::Scientific);
        assert_eq!(l.keys.len(), 50);
        no_overlap(&l);
        let l = lay(852.0, 393.0, "ios18", Mode::Basic);
        assert!(l.scientific && l.keys.len() == 50);
        no_overlap(&l);
    }

    fn lay_full(w: f32, h: f32, theme_id: &str, mode: Mode, sidebar: bool, convert: bool) -> Layout {
        let t = theme(theme_id);
        let d = Calc::default().display(NumberStyle::Comma, false);
        layout(&LayoutIn { w, h, insets: Insets::default(), mode, convert, sidebar_open: sidebar, theme: &t, disp: &d, decimal: '.' })
    }

    /// Every theme, mode and a spread of window shapes: keys inside the window and apart, the
    /// numbers ending at the keys' right edge, toolbar buttons one size and evenly padded.
    #[test]
    fn every_theme_every_size() {
        let sizes = [(320.0, 568.0), (393.0, 852.0), (430.0, 932.0), (390.0, 760.0), (852.0, 393.0), (568.0, 320.0), (700.0, 700.0), (820.0, 1180.0), (1180.0, 820.0), (1440.0, 1000.0), (1000.0, 1060.0), (1920.0, 1080.0), (600.0, 480.0)];
        for th in crate::theme::themes() {
            for (w, h) in sizes {
                for mode in [Mode::Basic, Mode::Scientific] {
                    for sidebar in [false, true] {
                        for convert in [false, true] {
                            let l = lay_full(w, h, th.id, mode, sidebar, convert);
                            let ctx = format!("{} {w}x{h} {mode:?} sidebar={sidebar}", th.id);
                            no_overlap(&l);
                            let mut ids: Vec<&str> = l.keys.iter().map(|k| k.id.as_str()).collect();
                            ids.sort();
                            let n = ids.len();
                            ids.dedup();
                            assert_eq!(ids.len(), n, "{ctx}: a key appears twice");
                            let right = l.keys.iter().map(|k| k.rect.x + k.rect.w).fold(0.0, f32::max);
                            let left = l.keys.iter().map(|k| k.rect.x).fold(f32::MAX, f32::min);
                            assert!((l.display.x + l.display.w - right).abs() < 0.5, "{ctx}: numbers not aligned to keys");
                            assert!((l.display.x - left).abs() < 0.5, "{ctx}");
                            assert!(l.display.h > 30.0, "{ctx}: display {}", l.display.h);
                            for b in &l.toolbar {
                                assert_eq!((b.rect.w, b.rect.h), (53.0, 53.0), "{ctx}");
                            }
                            if let Some(b) = l.toolbar.iter().find(|b| b.id == "mode") {
                                let pad_r = w - (b.rect.x + b.rect.w);
                                assert!((pad_r - b.rect.y).abs() < 0.5, "{ctx}: top and side padding differ");
                            }
                            if let Some(body) = l.body {
                                assert!(body.y >= l.toolbar[0].rect.y + 53.0 - 0.5, "{ctx}: panel under the toolbar");
                            }
                            for k in &l.keys {
                                assert!(k.font > 6.0 && k.font < k.rect.h, "{ctx}: {} font {}", k.id, k.font);
                            }
                            if mode == Mode::Scientific {
                                assert!(l.scientific && l.keys.iter().any(|k| k.id == "sin"), "{ctx}: no scientific keys");
                            }
                        }
                    }
                }
            }
        }
    }

    /// An open foldable: 5×4 basic keys across the full width; Scientific stacks its keys above
    /// them and climbs up the screen instead of five rows at the bottom.
    #[test]
    fn foldable_square() {
        for th in ["ios26", "ios18"] {
            let l = lay(690.0, 800.0, th, Mode::Basic);
            assert_eq!(l.keys.len(), 20);
            let left = l.keys.iter().map(|k| k.rect.x).fold(f32::MAX, f32::min);
            let right = l.keys.iter().map(|k| k.rect.x + k.rect.w).fold(0.0, f32::max);
            assert!(left < 30.0 && right > 660.0, "{th}: keys {left}..{right} don't fill the width");
            let r = |id: &str| l.keys.iter().find(|k| k.id == id).unwrap().rect;
            assert!(r("9").y == r("div").y && r("eq").y == r("add").y && r("eq").x < r("add").x);
            no_overlap(&l);
            let l = lay(690.0, 800.0, th, Mode::Scientific);
            assert_eq!(l.keys.len(), 50);
            let top = l.keys.iter().map(|k| k.rect.y).fold(f32::MAX, f32::min);
            assert!(top < 800.0 * 0.45, "{th}: keys start at {top}");
            no_overlap(&l);
        }
    }

    #[test]
    fn tablet_and_classic() {
        for (w, h, m) in [(1180.0, 820.0, Mode::Scientific), (1180.0, 820.0, Mode::Basic), (1366.0, 1024.0, Mode::Scientific), (1024.0, 768.0, Mode::Scientific)] {
            let l = lay(w, h, "ios26", m);
            let right = l.keys.iter().map(|k| k.rect.x + k.rect.w).fold(0.0, f32::max);
            assert!(right <= w - 8.0, "{w}x{h} keys end at {right}");
            no_overlap(&l);
        }
        let l = lay(1180.0, 820.0, "ios26", Mode::Scientific);
        assert!(l.sidebar.is_some());
        assert!(l.keys.iter().all(|k| (k.rect.w - k.rect.h).abs() < 0.5), "iPad keys are circles");
        let k = |id: &str| l.keys.iter().find(|k| k.id == id).unwrap().rect;
        assert!(k("back").x < k("ac").x && k("back").y == k("ac").y && k("back").x > k("mr").x);
        assert!(k("neg").y == k("0").y && k("neg").x < k("0").x);
        no_overlap(&l);
        for id in ["mac2019", "iphone2007", "aqua2001", "mac1984"] {
            let l = lay(393.0, 852.0, id, Mode::Basic);
            assert!(l.body.is_some());
            no_overlap(&l);
            let l = lay(852.0, 393.0, id, Mode::Basic);
            no_overlap(&l);
        }
    }
}
