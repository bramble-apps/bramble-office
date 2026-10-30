//! The look of an Excel workbook, for opening it the way Numbers does: cell styles from
//! xl/styles.xml (fonts, fills, borders, alignment, wrap, number formats; colours as RGB, theme
//! or indexed), and from each worksheet its column widths, row heights, merged cells and frozen
//! header rows / columns. Dates (date number formats on numbers) are shown with their format.

use std::collections::HashMap;

use kn_model::format::{Dash, StrokeSpec};
use kn_model::tablecells::CellAlign;
use kn_model::{Color, VAlign};

/// One border line.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub width: f32,
    pub dash: Dash,
    pub color: Color,
}

impl Edge {
    pub fn stroke(&self) -> StrokeSpec {
        StrokeSpec { color: self.color, width: self.width, dash: self.dash }
    }
}

/// A cell style (one <xf> of cellXfs, its font, fill and border resolved).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XStyle {
    pub font: Option<String>,
    pub size: Option<f32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<Color>,
    pub fill: Option<Color>,
    /// A gradient fill: from, to, angle (Values' degrees: 0 left to right, counter-clockwise).
    pub gradient: Option<(Color, Color, f32)>,
    pub halign: Option<CellAlign>,
    pub valign: Option<VAlign>,
    pub wrap: bool,
    /// left, right, top, bottom
    pub borders: [Option<Edge>; 4],
    /// The number format's code ("mmmm d", "0.00", "[$CAD]#,##0"…), None for General.
    pub numfmt: Option<String>,
    /// Text rotation (Excel's textRotation: 1-90 degrees up, 91-180 down by 90 less, 255
    /// letters stacked), 0 for none. Numbers has no such thing: Values keeps it for Excel files.
    pub rotation: i32,
    /// A diagonal line across the cell: the line, bottom-left to top-right (up), top-left to
    /// bottom-right (down). Numbers has none either.
    pub diag: Option<Diag>,
}

/// A cell's diagonal border.
#[derive(Clone, Debug, PartialEq)]
pub struct Diag {
    pub edge: Edge,
    pub up: bool,
    pub down: bool,
}

/// What an Excel cell has that a Numbers table can't hold: Values keeps it beside the
/// document for an Excel workbook, draws it, and writes it back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extra {
    pub rotation: i32,
    pub diag: Option<Diag>,
    /// The cell's hyperlink (an address, or "#Sheet!A1" inside the workbook).
    pub link: Option<String>,
}

impl XStyle {
    /// A date / time number format.
    pub fn is_date(&self) -> bool {
        self.numfmt.as_deref().is_some_and(is_date_code)
    }
}

/// The worksheet's layout.
#[derive(Clone, Debug, Default)]
pub struct XLayout {
    /// Column widths in points (column index -> width).
    pub widths: HashMap<u32, f32>,
    pub default_width: Option<f32>,
    /// Row heights in points.
    pub heights: HashMap<u32, f32>,
    pub default_height: Option<f32>,
    /// (row, col, rows, cols)
    pub merges: Vec<(u32, u32, u32, u32)>,
    /// Frozen rows / columns (the header rows and columns of a table Numbers exported).
    pub frozen_rows: u32,
    pub frozen_cols: u32,
    /// Each cell's style index (cellXfs).
    pub cell_style: HashMap<(u32, u32), u32>,
    /// A cell whose text carries its own font (rich text: the first run's), over its style's.
    pub run_font: HashMap<(u32, u32), RunFont>,
}

/// The font of a rich text run (<rPr>): Excel writes it whole, so what it leaves out is off.
#[derive(Clone, Debug, PartialEq)]
pub struct RunFont {
    pub font: Option<String>,
    pub size: Option<f32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<Color>,
}

impl RunFont {
    /// The first run's font of an <si> / <is>, if it has runs with properties.
    pub fn of(n: roxmltree::Node, theme: &[Color]) -> Option<RunFont> {
        let rpr = n.children().filter(|c| c.has_tag_name("r")).find_map(|r| r.children().find(|c| c.has_tag_name("rPr")))?;
        let flag = |tag: &str| rpr.children().find(|n| n.has_tag_name(tag)).is_some_and(|n| n.attribute("val").is_none_or(|v| v != "0" && v != "false" && v != "none"));
        Some(RunFont {
            font: rpr.children().find(|n| n.has_tag_name("rFont")).and_then(|n| n.attribute("val")).map(ps_name),
            size: rpr.children().find(|n| n.has_tag_name("sz")).and_then(|n| n.attribute("val")).and_then(|v| v.parse().ok()),
            bold: flag("b"),
            italic: flag("i"),
            underline: flag("u"),
            strike: flag("strike"),
            color: rpr.children().find(|n| n.has_tag_name("color")).and_then(|c| color_of(c, theme)),
        })
    }

    /// `st` with this font.
    pub fn over(&self, st: &XStyle) -> XStyle {
        let mut s = st.clone();
        if self.font.is_some() {
            s.font = self.font.clone();
        }
        if self.size.is_some() {
            s.size = self.size;
        }
        s.bold = self.bold;
        s.italic = self.italic;
        s.underline = self.underline;
        s.strike = self.strike;
        if self.color.is_some() {
            s.color = self.color;
        }
        s
    }
}

/// The theme's colours in Excel's index order (0 lt1, 1 dk1, 2 lt2, 3 dk2, 4.. accents, hlink).
pub fn theme_colors(theme_xml: &str) -> Vec<Color> {
    let Ok(d) = roxmltree::Document::parse(theme_xml) else { return Vec::new() };
    let Some(scheme) = d.descendants().find(|n| n.has_tag_name("clrScheme")) else { return Vec::new() };
    let mut cs: Vec<Color> = scheme
        .children()
        .filter(|n| n.is_element())
        .map(|n| {
            let v = n.children().find(|c| c.is_element());
            let hex = v.and_then(|v| v.attribute("lastClr").or_else(|| v.attribute("val"))).unwrap_or("000000");
            let hex = match hex {
                "windowText" => "000000",
                "window" => "FFFFFF",
                h => h,
            };
            hex_color(hex).unwrap_or(BLACK)
        })
        .collect();
    if cs.len() >= 4 {
        cs.swap(0, 1);
        cs.swap(2, 3);
    }
    cs
}

const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

fn hex_color(h: &str) -> Option<Color> {
    let h = h.trim_start_matches('#');
    let h = if h.len() == 8 { &h[2..] } else { h };
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color { r: ((v >> 16) & 255) as f32 / 255.0, g: ((v >> 8) & 255) as f32 / 255.0, b: (v & 255) as f32 / 255.0, a: 1.0 })
}

/// Excel's tint: towards white (positive) or black (negative).
fn tinted(c: Color, tint: f32) -> Color {
    let f = |x: f32| if tint < 0.0 { x * (1.0 + tint) } else { x + (1.0 - x) * tint };
    Color { r: f(c.r), g: f(c.g), b: f(c.b), a: c.a }
}

/// The first colours of Excel's indexed palette (the ones files use).
const INDEXED: [u32; 8] = [0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF];

fn color_of(n: roxmltree::Node, theme: &[Color]) -> Option<Color> {
    let base = if let Some(rgb) = n.attribute("rgb") {
        hex_color(rgb)?
    } else if let Some(t) = n.attribute("theme").and_then(|t| t.parse::<usize>().ok()) {
        *theme.get(t)?
    } else if let Some(i) = n.attribute("indexed").and_then(|t| t.parse::<usize>().ok()) {
        match i {
            0..=7 => hex_color(&format!("{:06X}", INDEXED[i]))?,
            8..=15 => hex_color(&format!("{:06X}", INDEXED[i - 8]))?,
            64 => BLACK,
            _ => return None,
        }
    } else if n.attribute("auto") == Some("1") {
        BLACK
    } else {
        return None;
    };
    Some(match n.attribute("tint").and_then(|t| t.parse::<f32>().ok()) {
        Some(t) if t != 0.0 => tinted(base, t),
        _ => base,
    })
}

/// Excel's built-in number formats that files refer to by id only.
fn builtin_format(id: u32) -> Option<&'static str> {
    Some(match id {
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        14 => "m/d/yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yyyy h:mm",
        49 => "@",
        _ => return None,
    })
}

/// A border line (<left>, <top>, <diagonal>…) of styles.xml.
fn edge_of(e: roxmltree::Node, theme: &[Color]) -> Option<Edge> {
    // Excel's lines are 1, 2 and 3 pixels at 96 dpi (0.75, 1.5, 2.25 pt), as Excel and
    // OnlyOffice draw them
    let (width, dash) = match e.attribute("style")? {
        "hair" => (0.35, Dash::Solid),
        "thin" => (0.75, Dash::Solid),
        "medium" => (1.5, Dash::Solid),
        "thick" => (2.25, Dash::Solid),
        "double" => (2.25, Dash::Solid),
        "dotted" => (0.75, Dash::Dotted),
        "dashed" | "dashDot" | "dashDotDot" => (0.75, Dash::Dashed),
        "mediumDashed" | "mediumDashDot" | "mediumDashDotDot" | "slantDashDot" => (1.5, Dash::Dashed),
        _ => return None,
    };
    let color = e.children().find(|n| n.has_tag_name("color")).and_then(|c| color_of(c, theme)).unwrap_or(BLACK);
    Some(Edge { width, dash, color })
}

/// The cell styles of xl/styles.xml, by cellXfs index.
pub fn read_styles(styles_xml: &str, theme: &[Color]) -> Vec<XStyle> {
    let Ok(d) = roxmltree::Document::parse(styles_xml) else { return Vec::new() };
    let root = d.root_element();
    let section = |tag: &str| root.children().find(|n| n.has_tag_name(tag));
    let numfmts: HashMap<u32, String> = section("numFmts")
        .map(|s| s.children().filter(|n| n.has_tag_name("numFmt")).filter_map(|n| Some((n.attribute("numFmtId")?.parse().ok()?, n.attribute("formatCode")?.to_string()))).collect())
        .unwrap_or_default();
    struct Font {
        name: Option<String>,
        size: Option<f32>,
        bold: bool,
        italic: bool,
        underline: bool,
        strike: bool,
        color: Option<Color>,
    }
    let flag = |f: roxmltree::Node, tag: &str| f.children().find(|n| n.has_tag_name(tag)).is_some_and(|n| n.attribute("val").is_none_or(|v| v != "0" && v != "false" && v != "none"));
    let fonts: Vec<Font> = section("fonts")
        .map(|s| {
            s.children()
                .filter(|n| n.has_tag_name("font"))
                .map(|f| Font {
                    name: f.children().find(|n| n.has_tag_name("name")).and_then(|n| n.attribute("val")).map(str::to_string),
                    size: f.children().find(|n| n.has_tag_name("sz")).and_then(|n| n.attribute("val")).and_then(|v| v.parse().ok()),
                    bold: flag(f, "b"),
                    italic: flag(f, "i"),
                    underline: flag(f, "u"),
                    strike: flag(f, "strike"),
                    color: f.children().find(|n| n.has_tag_name("color")).and_then(|c| color_of(c, theme)),
                })
                .collect()
        })
        .unwrap_or_default();
    let fills: Vec<Option<Color>> = section("fills")
        .map(|s| {
            s.children()
                .filter(|n| n.has_tag_name("fill"))
                .map(|f| {
                    let p = f.children().find(|n| n.has_tag_name("patternFill"))?;
                    if p.attribute("patternType") != Some("solid") {
                        return None;
                    }
                    p.children().find(|n| n.has_tag_name("fgColor")).and_then(|c| color_of(c, theme))
                })
                .collect()
        })
        .unwrap_or_default();
    // gradient fills (<gradientFill degree=…> with two stops): Excel's degrees run clockwise
    // from left to right, Values' counter-clockwise
    let gradients: Vec<Option<(Color, Color, f32)>> = section("fills")
        .map(|s| {
            s.children()
                .filter(|n| n.has_tag_name("fill"))
                .map(|f| {
                    let g = f.children().find(|n| n.has_tag_name("gradientFill"))?;
                    let stops: Vec<Color> = g.children().filter(|n| n.has_tag_name("stop")).filter_map(|st| st.children().find(|n| n.has_tag_name("color")).and_then(|c| color_of(c, theme))).collect();
                    let (a, b) = (*stops.first()?, *stops.last()?);
                    // (a "path" gradient is drawn along its degree too, as OnlyOffice and
                    // LibreOffice draw it: 90 is top to bottom)
                    let deg: f32 = g.attribute("degree").and_then(|v| v.parse().ok()).unwrap_or(0.0);
                    Some((a, b, (360.0 - deg).rem_euclid(360.0)))
                })
                .collect()
        })
        .unwrap_or_default();
    let diags: Vec<Option<Diag>> = section("borders")
        .map(|s| {
            s.children()
                .filter(|n| n.has_tag_name("border"))
                .map(|b| {
                    let up = matches!(b.attribute("diagonalUp"), Some("1") | Some("true"));
                    let down = matches!(b.attribute("diagonalDown"), Some("1") | Some("true"));
                    let e = b.children().find(|n| n.has_tag_name("diagonal"))?;
                    let edge = edge_of(e, theme)?;
                    (up || down).then_some(Diag { edge, up, down })
                })
                .collect()
        })
        .unwrap_or_default();
    let borders: Vec<[Option<Edge>; 4]> = section("borders")
        .map(|s| {
            s.children()
                .filter(|n| n.has_tag_name("border"))
                .map(|b| {
                    let edge = |tag: &str| -> Option<Edge> { edge_of(b.children().find(|n| n.has_tag_name(tag))?, theme) };
                    [edge("left"), edge("right"), edge("top"), edge("bottom")]
                })
                .collect()
        })
        .unwrap_or_default();
    section("cellXfs")
        .map(|s| {
            s.children()
                .filter(|n| n.has_tag_name("xf"))
                .map(|x| {
                    let id = |a: &str| x.attribute(a).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                    let mut st = XStyle::default();
                    if let Some(f) = fonts.get(id("fontId")) {
                        st.font = f.name.as_deref().map(ps_name);
                        st.size = f.size;
                        st.bold = f.bold;
                        st.italic = f.italic;
                        st.underline = f.underline;
                        st.strike = f.strike;
                        st.color = f.color;
                    }
                    st.fill = fills.get(id("fillId")).copied().flatten();
                    st.gradient = gradients.get(id("fillId")).copied().flatten();
                    st.borders = borders.get(id("borderId")).cloned().unwrap_or_default();
                    st.diag = diags.get(id("borderId")).cloned().flatten();
                    let nf = id("numFmtId") as u32;
                    st.numfmt = numfmts.get(&nf).cloned().or_else(|| builtin_format(nf).map(str::to_string));
                    if let Some(a) = x.children().find(|n| n.has_tag_name("alignment")) {
                        st.halign = match a.attribute("horizontal") {
                            Some("left") => Some(CellAlign::Left),
                            Some("right") => Some(CellAlign::Right),
                            Some("center") | Some("centerContinuous") => Some(CellAlign::Center),
                            Some("justify") | Some("distributed") => Some(CellAlign::Justify),
                            _ => None,
                        };
                        st.valign = match a.attribute("vertical") {
                            Some("top") => Some(VAlign::Top),
                            Some("center") => Some(VAlign::Middle),
                            Some("bottom") => Some(VAlign::Bottom),
                            _ => None,
                        };
                        st.wrap = matches!(a.attribute("wrapText"), Some("1") | Some("true"));
                        st.rotation = a.attribute("textRotation").and_then(|v| v.parse().ok()).unwrap_or(0);
                    }
                    st
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A font as Excel names it ("Poppins Black") as Numbers stores it, by PostScript name
/// ("Poppins-Black"): the family, then the weight or style word after a hyphen.
pub fn ps_name(name: &str) -> String {
    const STYLES: [&str; 14] = ["Thin", "ExtraLight", "Light", "Regular", "Book", "Medium", "SemiBold", "Demi", "Bold", "ExtraBold", "Heavy", "Black", "Italic", "Oblique"];
    let words: Vec<&str> = name.split_whitespace().collect();
    match words.split_last() {
        Some((last, rest)) if !rest.is_empty() && STYLES.iter().any(|s| s.eq_ignore_ascii_case(last)) => format!("{}-{}", rest.concat(), last),
        _ => words.concat(),
    }
}

/// Excel column width (characters of the default font) in points.
pub fn width_points(chars: f32) -> f32 {
    ((chars * 7.0 + 0.5).trunc() * 0.75).max(4.0)
}

/// "C12" -> (row 11, col 2)
pub fn cell_ref_pub(r: &str) -> Option<(u32, u32)> {
    cell_ref(r)
}

fn cell_ref(r: &str) -> Option<(u32, u32)> {
    let split = r.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = r.split_at(split);
    let col = letters.bytes().try_fold(0u32, |a, b| b.is_ascii_uppercase().then(|| a * 26 + (b - b'A') as u32 + 1))?;
    let row: u32 = digits.parse().ok()?;
    (col > 0 && row > 0).then(|| (row - 1, col - 1))
}

/// A worksheet's layout (rows and columns past `max` are left out).
pub fn read_layout(doc: &roxmltree::Document, max: (u32, u32)) -> XLayout {
    let mut l = XLayout::default();
    let root = doc.root_element();
    if let Some(f) = root.children().find(|n| n.has_tag_name("sheetFormatPr")) {
        l.default_width = f.attribute("defaultColWidth").and_then(|v| v.parse::<f32>().ok()).map(width_points);
        l.default_height = f.attribute("defaultRowHeight").and_then(|v| v.parse().ok());
    }
    if let Some(cols) = root.children().find(|n| n.has_tag_name("cols")) {
        for c in cols.children().filter(|n| n.has_tag_name("col")) {
            let (Some(a), Some(b), Some(w)) = (c.attribute("min").and_then(|v| v.parse::<u32>().ok()), c.attribute("max").and_then(|v| v.parse::<u32>().ok()), c.attribute("width").and_then(|v| v.parse::<f32>().ok())) else { continue };
            for k in a..=b.min(max.1) {
                if k >= 1 {
                    l.widths.insert(k - 1, width_points(w));
                }
            }
        }
    }
    if let Some(p) = root.descendants().find(|n| n.has_tag_name("pane")) {
        if matches!(p.attribute("state"), Some("frozen") | Some("frozenSplit")) {
            l.frozen_cols = p.attribute("xSplit").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) as u32;
            l.frozen_rows = p.attribute("ySplit").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) as u32;
        }
    }
    for (auto_r, row) in root.descendants().filter(|n| n.has_tag_name("row")).enumerate() {
        let r = row.attribute("r").and_then(|v| v.parse::<u32>().ok()).map(|v| v - 1).unwrap_or(auto_r as u32);
        if r > max.0 {
            break;
        }
        if let Some(h) = row.attribute("ht").and_then(|v| v.parse::<f32>().ok()) {
            l.heights.insert(r, h);
        }
        for (auto_c, c) in row.children().filter(|n| n.has_tag_name("c")).enumerate() {
            let (rr, cc) = c.attribute("r").and_then(cell_ref).unwrap_or((r, auto_c as u32));
            if rr > max.0 || cc > max.1 {
                continue;
            }
            if let Some(s) = c.attribute("s").and_then(|v| v.parse::<u32>().ok()) {
                l.cell_style.insert((rr, cc), s);
            }
        }
    }
    if let Some(m) = root.children().find(|n| n.has_tag_name("mergeCells")) {
        for mc in m.children().filter(|n| n.has_tag_name("mergeCell")) {
            let Some((a, b)) = mc.attribute("ref").and_then(|r| r.split_once(':')) else { continue };
            if let (Some((r0, c0)), Some((r1, c1))) = (cell_ref(a), cell_ref(b)) {
                if r1 >= r0 && c1 >= c0 && (r1 > r0 || c1 > c0) && r1 <= max.0 && c1 <= max.1 {
                    l.merges.push((r0, c0, r1 - r0 + 1, c1 - c0 + 1));
                }
            }
        }
    }
    l
}

/// Whether a number format code shows a date or time.
pub fn is_date_code(code: &str) -> bool {
    let mut quoted = false;
    let mut prev = ' ';
    let mut s = String::new();
    for ch in code.chars() {
        if ch == '"' {
            quoted = !quoted;
        } else if !quoted && prev != '\\' {
            s.push(ch);
        }
        prev = ch;
    }
    // ([$-409] locale tags and [Red] colours aside)
    let s: String = {
        let mut out = String::new();
        let mut depth = 0;
        for ch in s.chars() {
            match ch {
                '[' => depth += 1,
                ']' => depth -= 1,
                _ if depth == 0 => out.push(ch),
                _ => {}
            }
        }
        out
    };
    let l = s.to_lowercase();
    l.contains('d') || l.contains('y') || (l.contains('h') && !l.contains('#')) || (l.contains('m') && !l.contains('#') && !l.contains('0'))
}

/// A date serial (days since 1899-12-30) shown with an Excel date format.
pub fn format_date(serial: f64, code: &str) -> String {
    let days = serial.floor() as i64;
    let secs = ((serial - serial.floor()) * 86400.0).round() as i64;
    // days since 1970
    let (y, m, d) = civil(days - 25569);
    let (hh, mi, ss) = (secs / 3600, secs / 60 % 60, secs % 60);
    let dow = (days - 25569 + 4).rem_euclid(7) as usize; // 1970-01-01 was a Thursday
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    let code = code.split(';').next().unwrap_or(code);
    let ampm = code.to_lowercase().contains("am/pm") || code.to_lowercase().contains("a/p");
    let chars: Vec<char> = code.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // after h (or before s) "m" means minutes
    let mut last_was_hour = false;
    while i < chars.len() {
        let ch = chars[i];
        let run = |c: char| chars[i..].iter().take_while(|x| x.eq_ignore_ascii_case(&c)).count();
        match ch {
            '"' => {
                let end = chars[i + 1..].iter().position(|c| *c == '"').map(|p| i + 1 + p).unwrap_or(chars.len());
                out.extend(&chars[i + 1..end]);
                i = end + 1;
                continue;
            }
            '\\' => {
                if let Some(c) = chars.get(i + 1) {
                    out.push(*c);
                }
                i += 2;
                continue;
            }
            '[' => {
                let end = chars[i..].iter().position(|c| *c == ']').map(|p| i + p).unwrap_or(chars.len());
                i = end + 1;
                continue;
            }
            '_' | '*' => {
                i += 2;
                continue;
            }
            'y' | 'Y' => {
                let n = run('y');
                out.push_str(&if n <= 2 { format!("{:02}", y % 100) } else { y.to_string() });
                i += n;
                last_was_hour = false;
                continue;
            }
            'd' | 'D' => {
                let n = run('d');
                out.push_str(&match n {
                    1 => d.to_string(),
                    2 => format!("{d:02}"),
                    3 => DAYS[dow][..3].to_string(),
                    _ => DAYS[dow].to_string(),
                });
                i += n;
                last_was_hour = false;
                continue;
            }
            'h' | 'H' => {
                let n = run('h');
                let h = if ampm { (hh + 11) % 12 + 1 } else { hh };
                out.push_str(&if n >= 2 { format!("{h:02}") } else { h.to_string() });
                i += n;
                last_was_hour = true;
                continue;
            }
            's' | 'S' => {
                let n = run('s');
                out.push_str(&if n >= 2 { format!("{ss:02}") } else { ss.to_string() });
                i += n;
                continue;
            }
            'm' | 'M' => {
                let n = run('m');
                let minutes = last_was_hour || chars[i + n..].iter().filter(|c| c.is_alphabetic()).take(2).any(|c| c.eq_ignore_ascii_case(&'s'));
                if minutes && n <= 2 {
                    out.push_str(&if n == 2 { format!("{mi:02}") } else { mi.to_string() });
                } else {
                    out.push_str(&match n {
                        1 => m.to_string(),
                        2 => format!("{m:02}"),
                        3 => MONTHS[(m - 1) as usize][..3].to_string(),
                        5 => MONTHS[(m - 1) as usize][..1].to_string(),
                        _ => MONTHS[(m - 1) as usize].to_string(),
                    });
                }
                i += n;
                last_was_hour = false;
                continue;
            }
            'A' | 'a' if code[code.char_indices().nth(i).map(|(b, _)| b).unwrap_or(0)..].to_lowercase().starts_with("am/pm") => {
                out.push_str(if hh < 12 { "AM" } else { "PM" });
                i += 5;
                continue;
            }
            '@' => {}
            c => out.push(c),
        }
        i += 1;
    }
    out.trim().to_string()
}

/// An Excel date / time format code as a Numbers (ICU) date pattern: "mmmm d" -> "MMMM d",
/// "[$-409]h:mm AM/PM;@" -> "h:mm a"; literal text quoted.
pub fn icu_pattern(code: &str) -> String {
    let code = code.split(';').next().unwrap_or(code);
    let ampm = code.to_lowercase().contains("am/pm");
    let chars: Vec<char> = code.chars().collect();
    let mut out = String::new();
    let mut lit = String::new();
    let flush = |out: &mut String, lit: &mut String| {
        if !lit.is_empty() {
            if lit.chars().all(|c| c == ' ' || c == ',' || c == '/' || c == '-' || c == ':' || c == '.') {
                out.push_str(lit);
            } else {
                out.push_str(&format!("'{}'", lit.replace('\'', "''")));
            }
            lit.clear();
        }
    };
    let mut i = 0;
    let mut last_hour = false;
    while i < chars.len() {
        let ch = chars[i];
        let run = chars[i..].iter().take_while(|x| x.eq_ignore_ascii_case(&ch)).count();
        let tok: Option<String> = match ch.to_ascii_lowercase() {
            '"' => {
                let end = chars[i + 1..].iter().position(|c| *c == '"').map(|p| i + 1 + p).unwrap_or(chars.len());
                lit.extend(&chars[i + 1..end]);
                i = end + 1;
                continue;
            }
            '\\' => {
                if let Some(c) = chars.get(i + 1) {
                    lit.push(*c);
                }
                i += 2;
                continue;
            }
            '[' => {
                i = chars[i..].iter().position(|c| *c == ']').map(|p| i + p + 1).unwrap_or(chars.len());
                continue;
            }
            '_' | '*' => {
                i += 2;
                continue;
            }
            '@' => {
                i += 1;
                continue;
            }
            'y' => Some(if run <= 2 { "yy".into() } else { "y".into() }),
            'd' => Some(match run {
                1 => "d".into(),
                2 => "dd".into(),
                3 => "EEE".into(),
                _ => "EEEE".into(),
            }),
            'h' => {
                last_hour = true;
                let c = if ampm { "h" } else { "H" };
                Some(c.repeat(run.min(2)))
            }
            's' => Some("s".repeat(run.min(2))),
            'm' => {
                let minutes = last_hour || chars[i + run..].iter().filter(|c| c.is_alphabetic()).take(2).any(|c| c.eq_ignore_ascii_case(&'s'));
                Some(if minutes && run <= 2 { "m".repeat(run) } else { "M".repeat(run.min(4)) })
            }
            'a' if code[code.char_indices().nth(i).map(|(b, _)| b).unwrap_or(0)..].to_lowercase().starts_with("am/pm") => {
                flush(&mut out, &mut lit);
                out.push('a');
                i += 5;
                continue;
            }
            _ => None,
        };
        match tok {
            Some(t) => {
                flush(&mut out, &mut lit);
                out.push_str(&t);
                if ch.to_ascii_lowercase() != 'h' && ch.to_ascii_lowercase() != 'm' {
                    last_hour = false;
                }
                i += run;
            }
            None => {
                lit.push(ch);
                i += 1;
            }
        }
    }
    flush(&mut out, &mut lit);
    out.trim().to_string()
}

/// Days since 1970-01-01 -> (year, month, day).
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        // 46025 = 2026-01-03 (a Saturday)
        assert_eq!(format_date(46025.0, "mmmm d"), "January 3");
        assert_eq!(format_date(46025.0, " d"), "3");
        assert_eq!(format_date(46025.0, "m/d/yyyy"), "1/3/2026");
        assert_eq!(format_date(46025.0, "[$-F800]dddd\\,\\ mmmm\\ d\\,\\ yyyy"), "Saturday, January 3, 2026");
        assert_eq!(format_date(46025.0, "[$-40C]d mmmm yyyy;@"), "3 January 2026");
        assert_eq!(format_date(0.75, "[$-409]h:mm AM/PM;@"), "6:00 PM");
        assert_eq!(format_date(46025.5, "h:mm"), "12:00");
        assert!(is_date_code("mmmm d") && is_date_code(" d") && is_date_code("h:mm AM/PM"));
        assert_eq!(icu_pattern("mmmm d"), "MMMM d");
        assert_eq!(icu_pattern(" d"), "d");
        assert_eq!(icu_pattern("m/d/yyyy"), "M/d/y");
        assert_eq!(icu_pattern("[$-409]h:mm AM/PM;@"), "h:mm a");
        assert_eq!(icu_pattern("[$-F800]dddd\\,\\ mmmm\\ d\\,\\ yyyy"), "EEEE, MMMM d, y");
        assert_eq!(icu_pattern("[$-40C]d mmmm yyyy;@"), "d MMMM y");
        assert_eq!(icu_pattern("h:mm:ss"), "H:mm:ss");
        assert!(!is_date_code("[$CAD]#,##0") && !is_date_code("0.00") && !is_date_code("\"$\"\"C\"\"A\"\" \"0.00"));
    }

    #[test]
    fn widths_and_colours() {
        assert_eq!(width_points(5.125), 27.0);
        assert_eq!(ps_name("Poppins Black"), "Poppins-Black");
        assert_eq!(ps_name("Avenir Next Demi"), "AvenirNext-Demi");
        assert_eq!(ps_name("Helvetica Neue"), "HelveticaNeue");
        assert_eq!(ps_name("Arial"), "Arial");
        assert_eq!(hex_color("FFF4F4F4"), Some(Color { r: 244.0 / 255.0, g: 244.0 / 255.0, b: 244.0 / 255.0, a: 1.0 }));
    }
}

// ------------------------------------------------------------------------------- writing

/// The family name Excel shows for a PostScript name ("Poppins-Black" -> "Poppins Black",
/// "HelveticaNeue-Bold" -> "Helvetica Neue Bold"; a plain "-Regular" is dropped).
pub fn family_name(ps: &str) -> String {
    let (fam, style) = ps.split_once('-').unwrap_or((ps, ""));
    let mut f = String::new();
    let mut prev_lower = false;
    for ch in fam.chars() {
        if ch.is_uppercase() && prev_lower {
            f.push(' ');
        }
        prev_lower = ch.is_lowercase();
        f.push(ch);
    }
    if style.is_empty() || style.eq_ignore_ascii_case("Regular") || style.eq_ignore_ascii_case("Roman") {
        f
    } else {
        format!("{f} {style}")
    }
}

fn argb(c: Color) -> String {
    let b = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("FF{:02X}{:02X}{:02X}", b(c.r), b(c.g), b(c.b))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A cell's look, as written to styles.xml.
#[derive(Clone, Debug, Default)]
pub struct CellLookOut {
    pub font: Option<(String, f32, bool, bool, bool, bool, Color)>,
    pub fill: Option<Color>,
    /// from, to, angle (Values' degrees)
    pub gradient: Option<(Color, Color, f32)>,
    pub halign: Option<&'static str>,
    pub valign: Option<&'static str>,
    pub wrap: bool,
    /// left, right, top, bottom: (width in points, dashed / dotted, colour)
    pub borders: [Option<(f32, u8, Color)>; 4],
    pub numfmt: Option<String>,
    /// Excel's textRotation (0 none).
    pub rotation: i32,
    pub diag: Option<Diag>,
}

/// The fonts, fills, borders, number formats and cell styles of a workbook being written,
/// each kept once.
pub struct StyleBook {
    fonts: Vec<String>,
    fills: Vec<String>,
    borders: Vec<String>,
    numfmts: Vec<String>,
    xfs: Vec<String>,
    index: HashMap<String, usize>,
}

impl Default for StyleBook {
    fn default() -> Self {
        let mut b = StyleBook { fonts: vec![r#"<font><sz val="11"/><name val="Calibri"/></font>"#.into()], fills: vec![r#"<fill><patternFill patternType="none"/></fill>"#.into(), r#"<fill><patternFill patternType="gray125"/></fill>"#.into()], borders: vec!["<border><left/><right/><top/><bottom/><diagonal/></border>".into()], numfmts: Vec::new(), xfs: vec![r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#.into()], index: HashMap::new() };
        b.index.insert(b.xfs[0].clone(), 0);
        b
    }
}

fn find_or_push(v: &mut Vec<String>, s: String) -> usize {
    match v.iter().position(|x| *x == s) {
        Some(i) => i,
        None => {
            v.push(s);
            v.len() - 1
        }
    }
}

impl StyleBook {
    /// The cell style index (cellXfs) for a look.
    pub fn xf(&mut self, l: &CellLookOut) -> usize {
        let font = match &l.font {
            Some((name, size, b, i, u, s, col)) => {
                let mut f = String::from("<font>");
                if *b {
                    f.push_str("<b/>");
                }
                if *i {
                    f.push_str("<i/>");
                }
                if *s {
                    f.push_str("<strike/>");
                }
                if *u {
                    f.push_str("<u/>");
                }
                f.push_str(&format!(r#"<sz val="{size}"/><color rgb="{}"/><name val="{}"/></font>"#, argb(*col), xml(&family_name(name))));
                find_or_push(&mut self.fonts, f)
            }
            None => 0,
        };
        let fill = match (l.gradient, l.fill) {
            (Some((a, b, angle)), _) => find_or_push(&mut self.fills, format!(r#"<fill><gradientFill degree="{}"><stop position="0"><color rgb="{}"/></stop><stop position="1"><color rgb="{}"/></stop></gradientFill></fill>"#, ((360.0 - angle).rem_euclid(360.0)).round(), argb(a), argb(b))),
            (None, Some(c)) => find_or_push(&mut self.fills, format!(r#"<fill><patternFill patternType="solid"><fgColor rgb="{}"/><bgColor indexed="64"/></patternFill></fill>"#, argb(c))),
            (None, None) => 0,
        };
        let border = if l.borders.iter().any(Option::is_some) || l.diag.is_some() {
            let side = |tag: &str, e: &Option<(f32, u8, Color)>| match e {
                Some((w, dash, c)) => {
                    // (0.75 / 1.5 / 2.25 pt: thin / medium / thick; Numbers' default 0.35 pt hair)
                    let style = match (dash, w) {
                        (1, w) if *w >= 1.2 => "mediumDashed",
                        (1, _) => "dashed",
                        (2, _) => "dotted",
                        (_, w) if *w >= 1.9 => "thick",
                        (_, w) if *w >= 1.2 => "medium",
                        (_, w) if *w < 0.5 => "hair",
                        _ => "thin",
                    };
                    format!(r#"<{tag} style="{style}"><color rgb="{}"/></{tag}>"#, argb(*c))
                }
                None => format!("<{tag}/>"),
            };
            let [lft, rgt, top, bot] = &l.borders;
            let (attrs, diag) = match &l.diag {
                Some(d) => (
                    format!("{}{}", if d.up { r#" diagonalUp="1""# } else { "" }, if d.down { r#" diagonalDown="1""# } else { "" }),
                    side("diagonal", &Some((d.edge.width, match d.edge.dash { Dash::Solid => 0, Dash::Dashed => 1, Dash::Dotted => 2 }, d.edge.color))),
                ),
                None => (String::new(), "<diagonal/>".into()),
            };
            find_or_push(&mut self.borders, format!("<border{attrs}>{}{}{}{}{diag}</border>", side("left", lft), side("right", rgt), side("top", top), side("bottom", bot)))
        } else {
            0
        };
        let numfmt = match &l.numfmt {
            Some(code) => 164 + find_or_push(&mut self.numfmts, code.clone()),
            None => 0,
        };
        let mut xf = format!(r#"<xf numFmtId="{numfmt}" fontId="{font}" fillId="{fill}" borderId="{border}" xfId="0""#);
        for (on, a) in [(numfmt > 0, "applyNumberFormat"), (font > 0, "applyFont"), (fill > 0, "applyFill"), (border > 0, "applyBorder")] {
            if on {
                xf.push_str(&format!(r#" {a}="1""#));
            }
        }
        if l.halign.is_some() || l.valign.is_some() || l.wrap || l.rotation != 0 {
            xf.push_str(r#" applyAlignment="1"><alignment"#);
            if let Some(h) = l.halign {
                xf.push_str(&format!(r#" horizontal="{h}""#));
            }
            if let Some(v) = l.valign {
                xf.push_str(&format!(r#" vertical="{v}""#));
            }
            if l.wrap {
                xf.push_str(r#" wrapText="1""#);
            }
            if l.rotation != 0 {
                xf.push_str(&format!(r#" textRotation="{}""#, l.rotation));
            }
            xf.push_str("/></xf>");
        } else {
            xf.push_str("/>");
        }
        if let Some(i) = self.index.get(&xf) {
            return *i;
        }
        self.xfs.push(xf.clone());
        self.index.insert(xf, self.xfs.len() - 1);
        self.xfs.len() - 1
    }

    /// xl/styles.xml
    pub fn styles_xml(&self) -> String {
        let fmts: String = self.numfmts.iter().enumerate().map(|(i, c)| format!(r#"<numFmt numFmtId="{}" formatCode="{}"/>"#, 164 + i, xml(c))).collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{}<fonts count="{}">{}</fonts><fills count="{}">{}</fills><borders count="{}">{}</borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="{}">{}</cellXfs></styleSheet>"#,
            if fmts.is_empty() { String::new() } else { format!(r#"<numFmts count="{}">{fmts}</numFmts>"#, self.numfmts.len()) },
            self.fonts.len(),
            self.fonts.concat(),
            self.fills.len(),
            self.fills.concat(),
            self.borders.len(),
            self.borders.concat(),
            self.xfs.len(),
            self.xfs.concat()
        )
    }
}
