//! Word (.docx) in: the document read into Letters' own terms ([`WordDoc`]) and built into a
//! Pages document ([`build`]). Kept: page size and margins (header / footer distances, columns,
//! orientation), the page colour, headers and footers (left / centre / right fields, page number
//! and page count fields, a different first page), paragraph styles (resolved through Word's
//! style chain and theme fonts), spacing, line spacing, indents, alignment, page breaks, lists
//! (bullet or numbered, from numbering.xml), run formatting with highlights, links, footnotes,
//! comments, tables (column widths, row heights, header rows, merged cells, cell fills, borders,
//! bold text), inline pictures in place and floating pictures and text boxes where Word put them,
//! with their text wrap. Tracked changes come in accepted.

use std::collections::HashMap;

use kn_model::edit::History;
use kn_model::{Align, Color, PathEl};
use roxmltree::Node;

use crate::import::{InPara, RunFmt};
use crate::Pages;

/// A field in the text (Word's PAGE / NUMPAGES).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Field {
    Page,
    PageCount,
}

/// Paragraph settings beyond [`InPara`]'s (points; None: Word left it to the style default).
#[derive(Clone, Debug, Default)]
pub struct ParaExtra {
    pub space_before: Option<f32>,
    pub space_after: Option<f32>,
    /// (mode, amount): "kRelativeLineSpacing" (lines), "kExactLineSpacing" / "kMinimumLineSpacing" (pt).
    pub line: Option<(&'static str, f32)>,
    pub left: Option<f32>,
    pub right: Option<f32>,
    pub first: Option<f32>,
    pub page_break_before: bool,
    /// A column break after the paragraph (Word's br type="column").
    pub column_break_after: bool,
    /// Byte offsets in the paragraph's text of inline pictures (parallel to InPara::images).
    pub image_at: Vec<usize>,
    /// The size Word gives each inline picture (wp:extent), points (parallel to image_at).
    pub image_size: Vec<Option<(f32, f32)>>,
    /// An empty paragraph's mark formatting (its size sets the empty line's height in Word).
    pub mark: Option<RunFmt>,
    /// A header / footer paragraph in a table cell: the cell's text box across the text column
    /// (left, right in points from its left edge), made the field's indents.
    pub cell_box: Option<(f32, f32, Table)>,
    /// Paragraph border (w:pBdr): colour, width, distance from the text (pt), edges (1 top,
    /// 2 right, 4 bottom, 8 left).
    pub border: Option<ParaBorder>,
    /// Fields: (byte offset, field); a one-character stand-in sits in the text there.
    pub fields: Vec<(usize, Field)>,
    /// Footnotes: (byte offset, the note's text).
    pub footnotes: Vec<(usize, String)>,
    /// Comments: (start, end, text, author) in bytes.
    pub comments: Vec<(usize, usize, String, String)>,
    pub anchors: Vec<Anchor>,
    /// Shapes and text boxes in the text (wp:inline): byte offset and the object.
    pub inline_shapes: Vec<(usize, Anchor)>,
    pub highlights: Vec<(usize, usize, Color)>,
    /// The Word paragraph style's id (w:pStyle): contextual spacing compares neighbours by it.
    pub style_id: String,
    /// w:contextualSpacing: no space before / after next to a paragraph of the same style.
    pub contextual: bool,
    /// A list paragraph's label position, its other lines' text position and its first line's
    /// text position, points from the text column's left edge (Word's indents from the
    /// paragraph and its numbering level, and where the tab after the label goes).
    pub list_geom: Option<(f32, f32, f32)>,
    /// A list paragraph's mark formatting (the paragraph style's, and the mark's own w:rPr):
    /// Word draws the label in it.
    pub label_fmt: Option<RunFmt>,
    /// The label's size against the mark's (the level's own w:sz).
    pub label_scale: f32,
    /// A Word list paragraph's list (its w:numId) and every level of that list, as Letters'
    /// list style levels.
    pub num_id: Option<String>,
    pub list_levels: Vec<ListLevelIn>,
    /// The paragraph mark's formatting (the style's, and the mark's own w:rPr): an empty
    /// paragraph's line height, and a list label's look.
    pub mark_fmt: Option<RunFmt>,
    /// The paragraph ends a Word section (its w:sectPr): a section break after it.
    pub section_end: bool,
    /// A bullet's own character (a symbol font's U+F0xx code), when the label isn't the list
    /// style's "•".
    pub label_text: Option<String>,
    /// w:keepNext / w:keepLines: on the next paragraph's page; not split over pages.
    pub keep_next: bool,
    pub keep_lines: bool,
    /// Tab stops: (position pt from the text's left edge, 0 left / 1 centre / 2 right /
    /// 3 decimal, leader).
    pub tabs: Vec<(f32, u8, Option<char>)>,
}

/// One level of a Word list in Letters' terms.
#[derive(Clone, Debug, Default)]
pub struct ListLevelIn {
    pub level: u32,
    /// The text's distance after the label, points (the other lines' indent less the label's).
    pub gap: f32,
    pub numbered: bool,
    /// Pages' number type (decimal / roman / alphabetic, with "1." "(1)" "1)"), tiered (1.1).
    pub number: u64,
    pub tiered: bool,
    pub scale: f32,
    /// A symbol font's bullet character.
    pub label_text: Option<String>,
}

/// A Word level's number format as Pages' number type, and whether it shows the higher levels'
/// numbers too ("%1.%2.").
fn word_number_type(fmt: &str, text: &str) -> (u64, bool) {
    let base = match fmt {
        "upperRoman" => 3,
        "lowerRoman" => 6,
        "upperLetter" => 9,
        "lowerLetter" => 12,
        _ => 0,
    };
    let t = text.trim();
    let style = if t.starts_with('(') && t.ends_with(')') {
        1
    } else if t.ends_with(')') {
        2
    } else {
        0
    };
    (base + style, t.matches('%').count() >= 2)
}

/// A floating picture or text box (wp:anchor).
#[derive(Clone, Debug)]
pub struct Anchor {
    pub kind: AnchorKind,
    pub w: f32,
    pub h: f32,
    /// Offsets (pt) and what they are from: "page", "margin", "column", "paragraph", "line", "character".
    pub x: f32,
    pub x_from: String,
    pub x_align: Option<String>,
    pub y: f32,
    pub y_from: String,
    pub y_align: Option<String>,
    /// Pages' wrap: 5 around, 3 above and below, 2 none.
    pub wrap: u64,
    pub behind: bool,
}

#[derive(Clone, Debug)]
pub enum AnchorKind {
    Picture(Vec<u8>, String),
    TextBox(Vec<InPara>),
    Shape(ShapeIn),
    Chart(ChartIn),
}

/// A Word chart (c:chartSpace): its type as Letters', its data (each series' name and
/// values over the categories, from the chart's cached values), title, legend and colours.
#[derive(Clone, Debug, Default)]
pub struct ChartIn {
    /// Letters' chart type ("columnChartType2D" …).
    pub ty: String,
    pub series: Vec<String>,
    pub categories: Vec<String>,
    /// series x categories; NaN: no value.
    pub values: Vec<Vec<f64>>,
    pub title: Option<String>,
    pub legend: bool,
    pub colors: Vec<Option<Color>>,
}

/// A chart part read into Letters' terms (`accents`: the theme's accent colours).
fn chart_of(xml: &str, accents: &[Color]) -> Option<ChartIn> {
    let d = roxmltree::Document::parse(xml).ok()?;
    let chart = d.descendants().find(|n| n.tag_name().name() == "chart")?;
    let plot = child(&chart, "plotArea")?;
    let kinds = ["barChart", "bar3DChart", "lineChart", "line3DChart", "areaChart", "area3DChart", "pieChart", "pie3DChart", "doughnutChart", "radarChart", "scatterChart", "bubbleChart"];
    let group = plot.children().find(|n| kinds.contains(&n.tag_name().name()))?;
    let name = group.tag_name().name();
    let val = |tag: &str| child(&group, tag).and_then(|n| attr(&n, "val")).unwrap_or("");
    let stacked = matches!(val("grouping"), "stacked" | "percentStacked");
    let three = name.contains("3D");
    let ty = match name {
        "barChart" | "bar3DChart" => {
            let bar = val("barDir") == "bar";
            format!("{}{}ChartType{}", if stacked { "stacked" } else { "" }, match (bar, stacked) {
                (true, true) => "Bar",
                (true, false) => "bar",
                (false, true) => "Column",
                (false, false) => "column",
            }, if three { "3D" } else { "2D" })
        }
        "lineChart" | "line3DChart" | "scatterChart" | "bubbleChart" => format!("lineChartType{}", if three { "3D" } else { "2D" }),
        "areaChart" | "area3DChart" => format!("{}ChartType{}", if stacked { "stackedArea" } else { "area" }, if three { "3D" } else { "2D" }),
        "pieChart" => "pieChartType2D".into(),
        "pie3DChart" => "pieChartType3D".into(),
        "doughnutChart" => "donutChartType2D".into(),
        "radarChart" => "radarChartType2D".into(),
        _ => "columnChartType2D".into(),
    };
    // a cache's points (string or number) by index
    let points = |n: Node| -> Vec<String> {
        let mut v: Vec<(usize, String)> = n.descendants().filter(|p| p.tag_name().name() == "pt").map(|p| (attr(&p, "idx").and_then(|i| i.parse().ok()).unwrap_or(0), child(&p, "v").and_then(|v| v.text()).unwrap_or("").to_string())).collect();
        v.sort_by_key(|p| p.0);
        let len = n.descendants().find(|p| p.tag_name().name() == "ptCount").and_then(|p| attr(&p, "val")).and_then(|v| v.parse().ok()).unwrap_or(v.len());
        let mut out = vec![String::new(); len.max(v.iter().map(|p| p.0 + 1).max().unwrap_or(0))];
        for (i, t) in v {
            out[i] = t;
        }
        out
    };
    let mut out = ChartIn { ty, legend: child(&chart, "legend").is_some(), ..Default::default() };
    for ser in group.children().filter(|n| n.tag_name().name() == "ser") {
        out.series.push(child(&ser, "tx").map(|t| points(t).join("")).filter(|t| !t.is_empty()).unwrap_or_else(|| format!("Series {}", out.series.len() + 1)));
        if out.categories.is_empty() {
            if let Some(c) = child(&ser, "cat").or_else(|| child(&ser, "xVal")) {
                out.categories = points(c);
            }
        }
        let vals = child(&ser, "val").or_else(|| child(&ser, "yVal")).map(points).unwrap_or_default();
        out.values.push(vals.iter().map(|v| v.trim().parse::<f64>().unwrap_or(f64::NAN)).collect());
        let fill = child(&ser, "spPr").and_then(|sp| child(&sp, "solidFill"));
        let color = fill.and_then(|f| srgb(Some(f))).or_else(|| {
            let sc = fill.and_then(|f| child(&f, "schemeClr")).and_then(|c| attr(&c, "val"))?;
            let i: usize = sc.strip_prefix("accent")?.parse().ok()?;
            accents.get(i.checked_sub(1)?).copied()
        });
        out.colors.push(color);
    }
    let n = out.values.iter().map(Vec::len).max().unwrap_or(0).max(out.categories.len());
    out.categories.resize(n, String::new());
    for v in out.values.iter_mut() {
        v.resize(n, f64::NAN);
    }
    // the title: its text, unless Word deleted the automatic one
    let deleted = child(&chart, "autoTitleDeleted").and_then(|n| attr(&n, "val")).is_some_and(|v| v == "1" || v == "true");
    out.title = child(&chart, "title").map(|t| t.descendants().filter(|n| n.tag_name().name() == "t").filter_map(|n| n.text()).collect::<String>()).filter(|t| !t.is_empty() && !deleted);
    Some(out)
}

/// A Word shape (wps:wsp): its preset or exact outline, fill and border.
#[derive(Clone, Debug, Default)]
pub struct ShapeIn {
    pub preset: Option<String>,
    /// Custom outline in the frame's points.
    pub path: Option<kn_model::Path>,
    /// Some(None): no fill.
    pub fill: Option<Option<Color>>,
    /// Some(None): no border.
    pub stroke: Option<Option<(Color, f32)>>,
    pub line: bool,
    pub rot: f32,
    pub flip_h: bool,
    pub flip_v: bool,
}

fn srgb(n: Option<Node>) -> Option<Color> {
    let s = n?.descendants().find(|c| c.tag_name().name() == "srgbClr")?;
    hex_color(attr(&s, "val")?)
}

/// A wps:wsp read: preset or custom geometry (scaled to `w` x `h` points), fill, line.
fn shape_of(wsp: Node, w: f32, h: f32) -> ShapeIn {
    let mut s = ShapeIn::default();
    let sppr = child(&wsp, "spPr");
    if let Some(x) = sppr.and_then(|p| child(&p, "xfrm")) {
        s.rot = attr(&x, "rot").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) / 60000.0;
        s.flip_h = attr(&x, "flipH") == Some("1");
        s.flip_v = attr(&x, "flipV") == Some("1");
    }
    if let Some(p) = sppr.and_then(|p| child(&p, "prstGeom")) {
        s.preset = attr(&p, "prst").map(str::to_string);
    }
    if let Some(cg) = sppr.and_then(|p| child(&p, "custGeom")) {
        let mut path = kn_model::Path { els: Vec::new() };
        let mut open = true;
        let mut points = 0;
        for pp in cg.descendants().filter(|n| n.tag_name().name() == "path") {
            let pw = attr(&pp, "w").and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0).unwrap_or(w * 12700.0);
            let ph = attr(&pp, "h").and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0).unwrap_or(h * 12700.0);
            let pts = |n: Node| -> Vec<kn_model::Point> {
                n.children().filter(|c| c.tag_name().name() == "pt").map(|c| kn_model::Point { x: attr(&c, "x").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) / pw * w, y: attr(&c, "y").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) / ph * h }).collect()
            };
            for e in pp.children().filter(|c| c.is_element()) {
                let v = pts(e);
                points += v.len();
                match (e.tag_name().name(), v.as_slice()) {
                    ("moveTo", [a]) => path.els.push(PathEl::Move(*a)),
                    ("lnTo", [a]) => path.els.push(PathEl::Line(*a)),
                    ("quadBezTo", [c, a]) => path.els.push(PathEl::Quad(*c, *a)),
                    ("cubicBezTo", [c1, c2, a]) => path.els.push(PathEl::Cubic(*c1, *c2, *a)),
                    ("close", _) => {
                        path.els.push(PathEl::Close);
                        open = false;
                    }
                    _ => {}
                }
            }
        }
        if open && points == 2 {
            s.line = true;
        }
        if !path.els.is_empty() {
            s.path = Some(path);
        }
    }
    if matches!(s.preset.as_deref(), Some("line" | "straightConnector1")) {
        s.line = true;
    }
    if let Some(p) = sppr {
        if child(&p, "noFill").is_some() {
            s.fill = Some(None);
        } else if let Some(c) = srgb(child(&p, "solidFill")) {
            s.fill = Some(Some(c));
        }
        if let Some(ln) = child(&p, "ln") {
            if child(&ln, "noFill").is_some() {
                s.stroke = Some(None);
            } else {
                let wpt = attr(&ln, "w").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 12700.0).unwrap_or(0.75);
                s.stroke = Some(srgb(child(&ln, "solidFill")).map(|c| (c, wpt)));
            }
        }
    }
    // a Word style reference without explicit colours: Office's default (accent 1)
    if child(&wsp, "style").is_some() {
        let accent = Color { r: 0x44 as f32 / 255.0, g: 0x72 as f32 / 255.0, b: 0xc4 as f32 / 255.0, a: 1.0 };
        let dark = Color { r: 0x2f as f32 / 255.0, g: 0x52 as f32 / 255.0, b: 0x8f as f32 / 255.0, a: 1.0 };
        if s.fill.is_none() && !s.line {
            s.fill = Some(Some(accent));
        }
        if s.stroke.is_none() {
            s.stroke = Some(Some((if s.line { accent } else { dark }, 1.0)));
        }
    }
    s
}

#[derive(Clone, Debug, Default)]
pub struct Para {
    pub p: InPara,
    pub x: ParaExtra,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParaBorder {
    pub color: Color,
    pub width: f32,
    pub space: f32,
    pub edges: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Cell {
    pub paras: Vec<Para>,
    pub fill: Option<Color>,
    /// Top, left, bottom, right: None no border given, Some(None) none, Some(Some(colour, width)).
    pub borders: [Option<Option<(Color, f32)>>; 4],
    /// Columns this cell spans (w:gridSpan).
    pub span: u32,
    /// Vertical merge: Some(true) starts one, Some(false) continues the one above.
    pub vmerge: Option<bool>,
    /// The cell's own margins (w:tcMar: left, top, right, bottom), over the table's.
    pub margins: [Option<f32>; 4],
    /// Its preferred width (w:tcW in twips), points.
    pub width: Option<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub col_widths: Vec<f32>,
    pub rows: Vec<(Option<f32>, bool, Vec<Cell>)>,
    /// Rows whose height is exact (w:hRule="exact": the whole row, margins and borders in it).
    pub exact_rows: Vec<bool>,
    /// Every row unsplittable and every cell paragraph kept with the next: Word keeps the whole
    /// table on one page, and a paragraph kept with it moves with it.
    pub keep_together: bool,
    pub borders: bool,
    /// The table's border line (w:tblBorders: colour, width), when it gives one.
    pub border_line: Option<(Color, f32)>,
    /// Cell margins left, top, right, bottom (w:tblCellMar; Word's default 5.4 / 0 / 5.4 / 0).
    pub margins: [f32; 4],
    /// Across the text column (w:jc): 0 left, 1 centred, 2 right.
    pub align: u8,
    /// An autofit table (w:tblLayout not fixed): Word and OnlyOffice shrink its columns to the
    /// room it has.
    pub autofit: bool,
    /// What it may hang out past the text column (before Word 2013 the cell margins).
    pub overhang: f32,
    /// A left-aligned table's left edge from the text's (w:tblInd; before Word 2013 the cell
    /// margin hangs out, so the cell text lines up with the margin).
    pub indent: f32,
}

impl Table {
    /// The table's left edge from the text column's, in a column `tw` wide.
    pub fn left(&self, tw: f32) -> f32 {
        let w: f32 = self.col_widths.iter().sum();
        match self.align {
            1 => (tw - w) / 2.0,
            2 => tw - w,
            _ => self.indent,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Block {
    Para(Para),
    Table(Table),
}

#[derive(Clone, Debug, Default)]
pub struct HeaderFooter {
    /// The fields' paragraphs: left, centre, right.
    pub fields: [Vec<Para>; 3],
    /// Pictures and shapes anchored in it (they repeat on every page: Pages' section template
    /// objects).
    pub anchors: Vec<Anchor>,
    /// Its height as Word lays it out (estimate, points): placing rules from its top.
    pub height: f32,
}

impl HeaderFooter {
    fn is_empty(&self) -> bool {
        self.anchors.is_empty() && self.fields.iter().all(|f| f.iter().all(|p| p.p.runs.iter().all(|r| r.0.trim().is_empty()) && p.x.fields.is_empty()))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Setup {
    pub width: f32,
    pub height: f32,
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
    pub header: f32,
    pub footer: f32,
    pub landscape: bool,
    pub columns: u32,
    pub column_gap: f32,
}

#[derive(Clone, Debug, Default)]
pub struct WordDoc {
    pub blocks: Vec<Block>,
    pub setup: Option<Setup>,
    pub header: Option<HeaderFooter>,
    pub footer: Option<HeaderFooter>,
    pub first_header: Option<HeaderFooter>,
    pub first_footer: Option<HeaderFooter>,
    /// Even pages' header and footer (when `even_odd`; none there: blank).
    pub even_header: Option<HeaderFooter>,
    pub even_footer: Option<HeaderFooter>,
    /// settings.xml w:evenAndOddHeaders: even pages have their own header and footer.
    pub even_odd: bool,
    pub title_page: bool,
    pub background: Option<Color>,
    /// Each section's page setup but the last's (that one is `setup`), in order.
    pub section_setups: Vec<Setup>,
    /// w:defaultTabStop, points.
    pub default_tab: f32,
}

// ---------------------------------------------------------------------- reading

fn attr<'a>(n: &Node<'a, '_>, local: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == local).map(|a| a.value())
}

fn child<'a, 'i>(n: &Node<'a, 'i>, local: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.tag_name().name() == local)
}

fn twips(v: Option<&str>) -> Option<f32> {
    v.and_then(|v| v.parse::<f32>().ok()).map(|v| v / 20.0)
}

fn emu(v: Option<&str>) -> Option<f32> {
    v.and_then(|v| v.parse::<f64>().ok()).map(|v| (v / 12700.0) as f32)
}

fn hex_color(v: &str) -> Option<Color> {
    if v.len() != 6 || v.eq_ignore_ascii_case("auto") {
        return None;
    }
    let h = |i: usize| u8::from_str_radix(&v[i..i + 2], 16).ok().map(|b| b as f32 / 255.0);
    Some(Color { r: h(0)?, g: h(2)?, b: h(4)?, a: 1.0 })
}

/// Word's named highlight colours.
fn highlight_color(name: &str) -> Option<Color> {
    let rgb = match name {
        "yellow" => (255, 255, 0),
        "green" => (0, 255, 0),
        "cyan" => (0, 255, 255),
        "magenta" => (255, 0, 255),
        "blue" => (0, 0, 255),
        "red" => (255, 0, 0),
        "darkBlue" => (0, 0, 139),
        "darkCyan" => (0, 139, 139),
        "darkGreen" => (0, 100, 0),
        "darkMagenta" => (139, 0, 139),
        "darkRed" => (139, 0, 0),
        "darkYellow" => (128, 128, 0),
        "darkGray" => (169, 169, 169),
        "lightGray" => (211, 211, 211),
        "black" => (0, 0, 0),
        _ => return None,
    };
    Some(Color { r: rgb.0 as f32 / 255.0, g: rgb.1 as f32 / 255.0, b: rgb.2 as f32 / 255.0, a: 1.0 })
}

/// Run properties as Word layers them (each layer overrides the ones before it).
#[derive(Clone, Debug, Default)]
struct RunProps {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    size: Option<f32>,
    color: Option<Color>,
    font: Option<String>,
    script: Option<i8>,
    highlight: Option<Color>,
    caps: Option<bool>,
    small_caps: Option<bool>,
    /// w:spacing: points added after each character.
    spacing: Option<f32>,
}

impl RunProps {
    fn over(&mut self, o: &RunProps) {
        macro_rules! take {
            ($($f:ident),*) => {$( if o.$f.is_some() { self.$f = o.$f.clone(); } )*};
        }
        take!(bold, italic, underline, strike, size, color, font, script, highlight, caps, small_caps, spacing);
    }
}

#[derive(Clone, Debug, Default)]
struct ParaProps {
    space_before: Option<f32>,
    space_after: Option<f32>,
    line: Option<(&'static str, f32)>,
    left: Option<f32>,
    right: Option<f32>,
    first: Option<f32>,
    align: Option<Align>,
    num: Option<(String, u32)>,
    page_break_before: Option<bool>,
    /// Some(None): borders turned off.
    border: Option<Option<ParaBorder>>,
    contextual: Option<bool>,
    /// w:framePr (even an empty one in the document defaults, as Pages writes them): OnlyOffice
    /// then tabs after a list label to a tab stop, not to the hanging indent.
    frame: Option<bool>,
    keep_next: Option<bool>,
    keep_lines: Option<bool>,
    /// w:tabs: (position pt, alignment 0 left / 1 centre / 2 right / 3 decimal / 4 clear,
    /// leader). Layers add to and clear the ones below them.
    tabs: Option<Vec<(f32, u8, Option<char>)>>,
}

impl ParaProps {
    fn over(&mut self, o: &ParaProps) {
        macro_rules! take {
            ($($f:ident),*) => {$( if o.$f.is_some() { self.$f = o.$f.clone(); } )*};
        }
        take!(space_before, space_after, line, left, right, first, align, num, page_break_before, border, contextual, frame, keep_next, keep_lines);
        // tab stops add up through the layers; a clear stop removes the one at its position
        if let Some(t) = &o.tabs {
            let mut cur = self.tabs.take().unwrap_or_default();
            for &(pos, al, lead) in t {
                cur.retain(|c| (c.0 - pos).abs() > 0.05);
                if al != 4 {
                    cur.push((pos, al, lead));
                }
            }
            cur.sort_by(|a, b| a.0.total_cmp(&b.0));
            self.tabs = Some(cur);
        }
    }
}

/// A list level as Word draws it.
#[derive(Clone, Debug, Default)]
struct Level {
    numbered: bool,
    /// w:numFmt ("decimal", "lowerRoman", "upperLetter" …).
    fmt: String,
    pp: ParaProps,
    rp: RunProps,
    /// w:lvlText ("%1." or the bullet).
    text: String,
    /// What follows the label: "tab", "space" or "nothing".
    suffix: String,
    /// The level's own tab stop for the text after the label (w:tab val="num"), points.
    tab: Option<f32>,
}

struct Style {
    name: String,
    based_on: Option<String>,
    rp: RunProps,
    pp: ParaProps,
}

struct Ctx {
    styles: HashMap<String, Style>,
    default_para_style: Option<String>,
    default_table_style: Option<String>,
    defaults_r: RunProps,
    defaults_p: ParaProps,
    theme_minor: Option<String>,
    theme_major: Option<String>,
    /// The theme's accent colours (accent1 … accent6).
    accents: Vec<Color>,
    /// numId -> (abstract levels: ilvl -> numbered).
    numbering: HashMap<String, HashMap<u32, bool>>,
    /// numId -> (ilvl -> the level: its indents, label formatting and text, what follows it).
    levels: HashMap<String, HashMap<u32, Level>>,
    /// w:defaultTabStop, points.
    default_tab: f32,
    rels: HashMap<String, String>,
    footnotes: HashMap<String, String>,
    comments: HashMap<String, (String, String)>,
    /// Word's compatibility mode (settings.xml; 12 Word 2007 … 15 Word 2013+): how tables sit.
    compat: u32,
    /// settings.xml w:evenAndOddHeaders
    even_odd: bool,
}

impl Ctx {
    fn run_props(&self, rpr: Option<Node>) -> RunProps {
        let mut f = RunProps::default();
        let Some(rpr) = rpr else { return f };
        for c in rpr.children() {
            let val = attr(&c, "val");
            let on = val.is_none_or(|v| v != "0" && v != "false" && v != "none");
            match c.tag_name().name() {
                "b" => f.bold = Some(on),
                "i" => f.italic = Some(on),
                "u" => f.underline = Some(on),
                "strike" | "dstrike" => f.strike = Some(on),
                "caps" => f.caps = Some(on),
                "spacing" => f.spacing = twips(val),
                "smallCaps" => f.small_caps = Some(on),
                "sz" => f.size = val.and_then(|v| v.parse::<f32>().ok()).map(|v| v / 2.0),
                "color" => f.color = val.and_then(hex_color),
                "rFonts" => {
                    f.font = attr(&c, "ascii").or(attr(&c, "hAnsi")).map(str::to_string).or_else(|| {
                        let theme = attr(&c, "asciiTheme").or(attr(&c, "hAnsiTheme"))?;
                        if theme.starts_with("major") { self.theme_major.clone() } else { self.theme_minor.clone() }
                    })
                }
                "vertAlign" => {
                    f.script = Some(match val {
                        Some("superscript") => 1,
                        Some("subscript") => -1,
                        _ => 0,
                    })
                }
                "highlight" => f.highlight = val.and_then(highlight_color),
                "shd" => {
                    if let Some(c) = attr(&c, "fill").and_then(hex_color) {
                        f.highlight = Some(c);
                    }
                }
                _ => {}
            }
        }
        f
    }

    fn para_props(&self, ppr: Option<Node>) -> ParaProps {
        let mut p = ParaProps::default();
        let Some(ppr) = ppr else { return p };
        for c in ppr.children() {
            match c.tag_name().name() {
                "pBdr" => {
                    // w:sz in eighths of a point, w:space in points; the first visible edge
                    // gives the colour and width
                    let mut b: Option<ParaBorder> = None;
                    for (tag, bit) in [("top", 1u8), ("right", 2), ("bottom", 4), ("left", 8)] {
                        let Some(e) = child(&c, tag) else { continue };
                        if matches!(attr(&e, "val"), None | Some("nil" | "none")) {
                            continue;
                        }
                        let color = attr(&e, "color").and_then(hex_color).unwrap_or(Color::BLACK);
                        let width = attr(&e, "sz").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 8.0).unwrap_or(0.5).max(0.25);
                        let space = attr(&e, "space").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                        let x = b.get_or_insert(ParaBorder { color, width, space, edges: 0 });
                        x.edges |= bit;
                    }
                    // (an empty w:pBdr names no edge: the style's borders stay)
                    if b.is_some() || c.children().any(|e| e.is_element()) {
                        p.border = Some(b);
                    }
                }
                "spacing" => {
                    p.space_before = twips(attr(&c, "before"));
                    p.space_after = twips(attr(&c, "after"));
                    if let Some(l) = attr(&c, "line").and_then(|v| v.parse::<f32>().ok()) {
                        p.line = Some(match attr(&c, "lineRule") {
                            Some("exact") => ("kExactLineSpacing", l / 20.0),
                            Some("atLeast") => ("kMinimumLineSpacing", l / 20.0),
                            _ => ("kRelativeLineSpacing", l / 240.0),
                        });
                    }
                }
                "ind" => {
                    p.left = twips(attr(&c, "left").or(attr(&c, "start")));
                    p.right = twips(attr(&c, "right").or(attr(&c, "end")));
                    if let Some(h) = twips(attr(&c, "hanging")) {
                        p.first = Some(-h);
                    } else if let Some(f) = twips(attr(&c, "firstLine")) {
                        p.first = Some(f);
                    }
                }
                "jc" => {
                    p.align = Some(match attr(&c, "val") {
                        Some("center") => Align::Center,
                        Some("right") | Some("end") => Align::Right,
                        Some("both") | Some("distribute") => Align::Justify,
                        _ => Align::Left,
                    })
                }
                "numPr" => {
                    let lvl = child(&c, "ilvl").and_then(|x| attr(&x, "val")).and_then(|v| v.parse().ok()).unwrap_or(0);
                    if let Some(id) = child(&c, "numId").and_then(|x| attr(&x, "val")) {
                        p.num = Some((id.to_string(), lvl));
                    }
                }
                "framePr" => p.frame = Some(true),
                "tabs" => {
                    p.tabs = Some(
                        c.children()
                            .filter(|t| t.tag_name().name() == "tab")
                            .filter_map(|t| {
                                let pos = twips(attr(&t, "pos"))?;
                                let al = match attr(&t, "val") {
                                    Some("center") => 1,
                                    Some("right") | Some("end") => 2,
                                    Some("decimal") => 3,
                                    Some("clear") => 4,
                                    Some("num") | Some("bar") => return None,
                                    _ => 0,
                                };
                                let lead = match attr(&t, "leader") {
                                    Some("dot") => Some('.'),
                                    Some("hyphen") => Some('-'),
                                    Some("underscore") => Some('_'),
                                    Some("middleDot") => Some('·'),
                                    _ => None,
                                };
                                Some((pos, al, lead))
                            })
                            .collect(),
                    )
                }
                "keepNext" => p.keep_next = Some(attr(&c, "val").is_none_or(|v| v != "0" && v != "false")),
                "keepLines" => p.keep_lines = Some(attr(&c, "val").is_none_or(|v| v != "0" && v != "false")),
                "contextualSpacing" => p.contextual = Some(attr(&c, "val").is_none_or(|v| v != "0" && v != "false")),
                "pageBreakBefore" => p.page_break_before = Some(attr(&c, "val").is_none_or(|v| v != "0" && v != "false")),
                _ => {}
            }
        }
        p
    }

    /// A style's properties with everything it is based on (in a table, Word layers the
    /// document defaults, then the table style, then the paragraph style).
    fn style_chain_over(&self, id: &str, table_style: Option<&str>) -> (RunProps, ParaProps) {
        let (mut r, mut p) = (self.defaults_r.clone(), self.defaults_p.clone());
        for s in table_style.into_iter().map(|t| self.chain_ids(t)).chain(std::iter::once(self.chain_ids(id))) {
            for c in s.iter().rev() {
                if let Some(s) = self.styles.get(c) {
                    r.over(&s.rp);
                    p.over(&s.pp);
                }
            }
        }
        (r, p)
    }

    /// A style and the ones it is based on, nearest first.
    fn chain_ids(&self, id: &str) -> Vec<String> {
        let mut chain = Vec::new();
        let mut cur = Some(id.to_string());
        while let Some(c) = cur {
            if chain.contains(&c) || chain.len() > 20 {
                break;
            }
            let next = self.styles.get(&c).and_then(|s| s.based_on.clone());
            chain.push(c);
            cur = next;
        }
        chain
    }

    fn pages_style(&self, id: &str) -> String {
        let n = self.styles.get(id).map(|s| s.name.to_lowercase()).unwrap_or_else(|| id.to_lowercase());
        match n.as_str() {
            "title" => "Title".into(),
            "subtitle" => "Subtitle".into(),
            "heading 1" | "heading1" | "heading" => "Heading".into(),
            "heading 2" | "heading2" => "Heading 2".into(),
            "heading 3" | "heading3" | "heading 4" | "heading4" | "heading 5" | "heading 6" => "Heading 3".into(),
            "caption" => "Caption".into(),
            "quote" | "intense quote" => "Quote".into(),
            _ => "Body".into(),
        }
    }

    fn numbered(&self, num: &str, lvl: u32) -> Option<bool> {
        let levels = self.numbering.get(num)?;
        levels.get(&lvl).or_else(|| levels.get(&0)).copied()
    }
}

fn read_zip(path: &std::path::Path) -> Result<HashMap<String, Vec<u8>>, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut out = HashMap::new();
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| e.to_string())?;
        if e.is_dir() {
            continue;
        }
        let Ok(name) = e.name().map(|n| n.to_string()) else { continue };
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut e, &mut b).map_err(|e| e.to_string())?;
        out.insert(name, b);
    }
    Ok(out)
}

fn text_of(files: &HashMap<String, Vec<u8>>, name: &str) -> Option<String> {
    files.get(name).and_then(|b| String::from_utf8(b.clone()).ok())
}

/// The part a relationship points at, from word/.
fn part_name(target: &str) -> String {
    let t = target.trim_start_matches("../");
    if let Some(abs) = t.strip_prefix('/') {
        abs.to_string()
    } else {
        format!("word/{t}")
    }
}

fn rels_of(files: &HashMap<String, Vec<u8>>, part: &str) -> HashMap<String, String> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let mut out = HashMap::new();
    if let Some(x) = text_of(files, &format!("{dir}/_rels/{file}.rels")) {
        if let Ok(d) = roxmltree::Document::parse(&x) {
            for r in d.descendants().filter(|n| n.tag_name().name() == "Relationship") {
                if let (Some(id), Some(t)) = (r.attribute("Id"), r.attribute("Target")) {
                    let ext = r.attribute("TargetMode") == Some("External");
                    out.insert(id.to_string(), if ext { t.to_string() } else { part_name(t) });
                }
            }
        }
    }
    out
}

/// Plain text of the paragraphs under `n` (footnotes, comments).
fn plain(n: Node) -> String {
    let mut out = String::new();
    for p in n.descendants().filter(|c| c.tag_name().name() == "p") {
        if !out.is_empty() {
            out.push('\n');
        }
        for t in p.descendants() {
            match t.tag_name().name() {
                "t" => out.push_str(t.text().unwrap_or("")),
                "tab" => out.push('\t'),
                _ => {}
            }
        }
    }
    out.trim().to_string()
}

pub fn read(path: &std::path::Path) -> Result<WordDoc, String> {
    let files = read_zip(path)?;
    // the main part (usually word/document.xml)
    let main = text_of(&files, "_rels/.rels")
        .and_then(|x| {
            let d = roxmltree::Document::parse(&x).ok()?;
            d.descendants().find(|n| n.attribute("Type").is_some_and(|t| t.ends_with("/officeDocument"))).and_then(|n| n.attribute("Target")).map(|t| t.trim_start_matches('/').to_string())
        })
        .unwrap_or_else(|| "word/document.xml".into());
    let doc_xml = text_of(&files, &main).ok_or("not a Word document")?;
    let mut ctx = Ctx {
        styles: HashMap::new(),
        default_para_style: None,
        default_table_style: None,
        defaults_r: RunProps::default(),
        defaults_p: ParaProps::default(),
        theme_minor: None,
        theme_major: None,
        accents: Vec::new(),
        numbering: HashMap::new(),
        levels: HashMap::new(),
        default_tab: 36.0,
        rels: rels_of(&files, &main),
        footnotes: HashMap::new(),
        comments: HashMap::new(),
        compat: 12,
        even_odd: false,
    };
    if let Some(x) = text_of(&files, "word/settings.xml") {
        if let Ok(d) = roxmltree::Document::parse(&x) {
            let v = d.descendants().find(|n| n.tag_name().name() == "compatSetting" && attr(n, "name") == Some("compatibilityMode")).and_then(|n| attr(&n, "val")).and_then(|v| v.parse().ok());
            if let Some(v) = v {
                ctx.compat = v;
            }
            if let Some(t) = d.descendants().find(|n| n.tag_name().name() == "defaultTabStop").and_then(|n| twips(attr(&n, "val"))).filter(|t| *t > 0.0) {
                ctx.default_tab = t;
            }
            ctx.even_odd = d.descendants().find(|n| n.tag_name().name() == "evenAndOddHeaders").is_some_and(|n| attr(&n, "val").is_none_or(|v| v != "0" && v != "false"));
        }
    }
    // theme fonts
    let theme_part = ctx.rels.values().find(|t| t.contains("theme/")).cloned().unwrap_or_else(|| "word/theme/theme1.xml".into());
    if let Some(x) = text_of(&files, &theme_part) {
        if let Ok(d) = roxmltree::Document::parse(&x) {
            let font = |which: &str| d.descendants().find(|n| n.tag_name().name() == which).and_then(|n| child(&n, "latin")).and_then(|l| attr(&l, "typeface")).filter(|t| !t.is_empty()).map(str::to_string);
            ctx.theme_major = font("majorFont");
            ctx.theme_minor = font("minorFont");
            ctx.accents = (1..=6).filter_map(|i| d.descendants().find(|n| n.tag_name().name() == format!("accent{i}")).and_then(|n| srgb(Some(n)))).collect();
        }
    }
    // styles (Word's own defaults under the document's: 10 pt Times New Roman)
    ctx.defaults_r.size = Some(10.0);
    ctx.defaults_r.font = Some("Times New Roman".into());
    if let Some(x) = text_of(&files, "word/styles.xml") {
        if let Ok(d) = roxmltree::Document::parse(&x) {
            if let Some(dd) = d.descendants().find(|n| n.tag_name().name() == "docDefaults") {
                let rpr = dd.descendants().find(|n| n.tag_name().name() == "rPr");
                let ppr = dd.descendants().find(|n| n.tag_name().name() == "pPr");
                let mut r = ctx.defaults_r.clone();
                r.over(&ctx.run_props(rpr));
                ctx.defaults_r = r;
                ctx.defaults_p = ctx.para_props(ppr);
            }
            let mut styles = HashMap::new();
            for s in d.descendants().filter(|n| n.tag_name().name() == "style") {
                let id = attr(&s, "styleId").unwrap_or("").to_string();
                let ty = attr(&s, "type").unwrap_or("paragraph");
                if attr(&s, "default").is_some_and(|v| v == "1" || v == "true") {
                    match ty {
                        "paragraph" => ctx.default_para_style = Some(id.clone()),
                        "table" => ctx.default_table_style = Some(id.clone()),
                        _ => {}
                    }
                }
                let st = Style {
                    name: child(&s, "name").and_then(|n| attr(&n, "val")).unwrap_or(&id).to_string(),
                    based_on: child(&s, "basedOn").and_then(|n| attr(&n, "val")).map(str::to_string),
                    rp: ctx.run_props(child(&s, "rPr")),
                    pp: ctx.para_props(child(&s, "pPr")),
                };
                styles.insert(id, st);
            }
            ctx.styles = styles;
        }
    }
    // numbering: numId -> its levels (an abstract definition, through a list style's link,
    // with the num's own level overrides)
    if let Some(x) = text_of(&files, "word/numbering.xml") {
        if let Ok(d) = roxmltree::Document::parse(&x) {
            let level = |l: Node| Level {
                numbered: !matches!(child(&l, "numFmt").and_then(|n| attr(&n, "val")).unwrap_or("bullet"), "bullet" | "none"),
                fmt: child(&l, "numFmt").and_then(|n| attr(&n, "val")).unwrap_or("bullet").to_string(),
                pp: ctx.para_props(child(&l, "pPr")),
                rp: ctx.run_props(child(&l, "rPr")),
                text: child(&l, "lvlText").and_then(|n| attr(&n, "val")).unwrap_or("").to_string(),
                suffix: child(&l, "suff").and_then(|n| attr(&n, "val")).unwrap_or("tab").to_string(),
                tab: child(&l, "pPr").and_then(|p| child(&p, "tabs")).and_then(|t| t.children().find(|c| attr(c, "val") == Some("num"))).and_then(|t| twips(attr(&t, "pos"))),
            };
            let mut abs: HashMap<String, HashMap<u32, Level>> = HashMap::new();
            let mut links: HashMap<String, String> = HashMap::new();
            for a in d.descendants().filter(|n| n.tag_name().name() == "abstractNum") {
                let id = attr(&a, "abstractNumId").unwrap_or("").to_string();
                if let Some(l) = child(&a, "numStyleLink").and_then(|n| attr(&n, "val")) {
                    links.insert(id.clone(), l.to_string());
                }
                let lv = a.children().filter(|n| n.tag_name().name() == "lvl").map(|l| (attr(&l, "ilvl").and_then(|v| v.parse().ok()).unwrap_or(0), level(l))).collect();
                abs.insert(id, lv);
            }
            let num_abs: HashMap<String, String> =
                d.descendants().filter(|n| n.tag_name().name() == "num").filter_map(|n| Some((attr(&n, "numId")?.to_string(), child(&n, "abstractNumId").and_then(|x| attr(&x, "val"))?.to_string()))).collect();
            // a definition that only links a list style: the levels of the definition the
            // style's own list names
            for (a, style) in &links {
                let target = ctx.styles.get(style).and_then(|s| s.pp.num.as_ref()).and_then(|(n, _)| num_abs.get(n)).filter(|t| *t != a).and_then(|t| abs.get(t)).cloned();
                if let Some(t) = target {
                    abs.insert(a.clone(), t);
                }
            }
            let mut nums: Vec<(String, HashMap<u32, Level>)> = Vec::new();
            for n in d.descendants().filter(|n| n.tag_name().name() == "num") {
                let Some(id) = attr(&n, "numId") else { continue };
                let mut lv = num_abs.get(id).and_then(|a| abs.get(a)).cloned().unwrap_or_default();
                // a w:lvlOverride's own level replaces the definition's
                for o in n.children().filter(|c| c.tag_name().name() == "lvlOverride") {
                    let i = attr(&o, "ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                    if let Some(l) = child(&o, "lvl") {
                        lv.insert(i, level(l));
                    }
                }
                nums.push((id.to_string(), lv));
            }
            for (id, lv) in nums {
                ctx.numbering.insert(id.clone(), lv.iter().map(|(i, l)| (*i, l.numbered)).collect());
                ctx.levels.insert(id, lv);
            }
        }
    }
    // footnotes and comments
    for (part, tag, is_note) in [("word/footnotes.xml", "footnote", true), ("word/comments.xml", "comment", false)] {
        if let Some(x) = text_of(&files, part) {
            if let Ok(d) = roxmltree::Document::parse(&x) {
                for n in d.descendants().filter(|n| n.tag_name().name() == tag) {
                    let id = attr(&n, "id").unwrap_or("").to_string();
                    if is_note {
                        if attr(&n, "type").is_none_or(|t| t == "normal") {
                            ctx.footnotes.insert(id, plain(n));
                        }
                    } else {
                        ctx.comments.insert(id, (plain(n), attr(&n, "author").unwrap_or("Author").to_string()));
                    }
                }
            }
        }
    }
    let x = roxmltree::Document::parse(&doc_xml).map_err(|e| e.to_string())?;
    let mut wd = WordDoc::default();
    if let Some(bg) = x.descendants().find(|n| n.tag_name().name() == "background") {
        wd.background = attr(&bg, "color").and_then(hex_color);
    }
    let body = x.descendants().find(|n| n.tag_name().name() == "body").ok_or("no body")?;
    let r = Reader { ctx: &ctx, files: &files };
    wd.default_tab = ctx.default_tab;
    wd.even_odd = ctx.even_odd;
    for n in body.children() {
        match n.tag_name().name() {
            "p" => {
                let p = r.para(n, &ctx.rels);
                // a section break inside the text: a new section (on a new page) after it, with
                // the page setup this sectPr gives the section it ends
                let sect = child(&n, "pPr").and_then(|ppr| child(&ppr, "sectPr"));
                let mut p = p;
                if let Some(sp) = sect {
                    p.p.page_break_after = true;
                    p.x.section_end = true;
                    wd.section_setups.push(setup_of(sp));
                }
                wd.blocks.push(Block::Para(p));
            }
            "tbl" => wd.blocks.push(Block::Table(r.table(n, &ctx.rels))),
            "sdt" => {
                // a content control at body level (a table of contents, a cover page): its paragraphs
                if let Some(c) = child(&n, "sdtContent") {
                    for p in c.children() {
                        match p.tag_name().name() {
                            "p" => wd.blocks.push(Block::Para(r.para(p, &ctx.rels))),
                            "tbl" => wd.blocks.push(Block::Table(r.table(p, &ctx.rels))),
                            _ => {}
                        }
                    }
                }
            }
            "sectPr" => {
                wd.setup = Some(setup_of(n));
                wd.title_page = child(&n, "titlePg").is_some_and(|t| attr(&t, "val").is_none_or(|v| v != "0" && v != "false"));
                for (tag, footer) in [("headerReference", false), ("footerReference", true)] {
                    for h in n.children().filter(|c| c.tag_name().name() == tag) {
                        let Some(target) = attr(&h, "id").and_then(|id| ctx.rels.get(id)) else { continue };
                        let Some(xml) = text_of(&files, target) else { continue };
                        let Ok(hd) = roxmltree::Document::parse(&xml) else { continue };
                        let hrels = rels_of(&files, target);
                        let hf = r.header_footer(hd.root_element(), &hrels);
                        if hf.is_empty() {
                            continue;
                        }
                        let slot = match (attr(&h, "type").unwrap_or("default"), footer) {
                            ("first", false) => &mut wd.first_header,
                            ("first", true) => &mut wd.first_footer,
                            ("even", false) => &mut wd.even_header,
                            ("even", true) => &mut wd.even_footer,
                            (_, false) => &mut wd.header,
                            (_, true) => &mut wd.footer,
                        };
                        *slot = Some(hf);
                    }
                }
            }
            _ => {}
        }
    }
    contextual_blocks(&mut wd.blocks);
    for hf in [&mut wd.header, &mut wd.footer, &mut wd.first_header, &mut wd.first_footer, &mut wd.even_header, &mut wd.even_footer].into_iter().flatten() {
        for f in hf.fields.iter_mut() {
            contextual_spacing(f);
        }
    }
    Ok(wd)
}

/// Word's w:contextualSpacing: a paragraph drops its space before (after) when the paragraph
/// before (after) it has the same style. A table between two paragraphs separates them.
fn contextual_spacing(paras: &mut [Para]) {
    for i in 1..paras.len() {
        if paras[i - 1].x.style_id != paras[i].x.style_id {
            continue;
        }
        if paras[i - 1].x.contextual {
            paras[i - 1].x.space_after = Some(0.0);
        }
        if paras[i].x.contextual {
            paras[i].x.space_before = Some(0.0);
        }
    }
}

fn contextual_blocks(blocks: &mut [Block]) {
    let mut run: Vec<usize> = Vec::new();
    let flush = |blocks: &mut [Block], run: &mut Vec<usize>| {
        let mut ps: Vec<Para> = run.iter().filter_map(|&i| if let Block::Para(p) = &blocks[i] { Some(p.clone()) } else { None }).collect();
        contextual_spacing(&mut ps);
        for (&i, p) in run.iter().zip(ps) {
            blocks[i] = Block::Para(p);
        }
        run.clear();
    };
    for i in 0..blocks.len() {
        match &mut blocks[i] {
            Block::Para(_) => run.push(i),
            Block::Table(t) => {
                for row in t.rows.iter_mut() {
                    for cell in row.2.iter_mut() {
                        contextual_spacing(&mut cell.paras);
                    }
                }
                flush(blocks, &mut run);
            }
        }
    }
    flush(blocks, &mut run);
}

fn setup_of(s: Node) -> Setup {
    let mut st = Setup { width: 612.0, height: 792.0, top: 72.0, bottom: 72.0, left: 72.0, right: 72.0, header: 36.0, footer: 36.0, landscape: false, columns: 1, column_gap: 36.0 };
    if let Some(sz) = child(&s, "pgSz") {
        st.width = twips(attr(&sz, "w")).unwrap_or(612.0);
        st.height = twips(attr(&sz, "h")).unwrap_or(792.0);
        st.landscape = attr(&sz, "orient") == Some("landscape") || st.width > st.height;
    }
    if let Some(m) = child(&s, "pgMar") {
        st.top = twips(attr(&m, "top")).unwrap_or(72.0).abs();
        st.bottom = twips(attr(&m, "bottom")).unwrap_or(72.0).abs();
        st.left = twips(attr(&m, "left").or(attr(&m, "start"))).unwrap_or(72.0);
        st.right = twips(attr(&m, "right").or(attr(&m, "end"))).unwrap_or(72.0);
        st.header = twips(attr(&m, "header")).unwrap_or(36.0);
        st.footer = twips(attr(&m, "footer")).unwrap_or(36.0);
    }
    if let Some(c) = child(&s, "cols") {
        st.columns = attr(&c, "num").and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
        st.column_gap = twips(attr(&c, "space")).unwrap_or(36.0);
    }
    st
}

struct Reader<'a> {
    ctx: &'a Ctx,
    files: &'a HashMap<String, Vec<u8>>,
}

/// What a run walk collects for one paragraph.
#[derive(Default)]
struct Collect {
    p: Para,
    /// Inside a complex field: (instruction so far, past its separator).
    field: Option<(String, bool)>,
    open_comments: HashMap<String, usize>,
}

impl Collect {
    fn len(&self) -> usize {
        self.p.p.runs.iter().map(|r| r.0.len()).sum()
    }

    fn push(&mut self, t: &str, f: &RunFmt) {
        if t.is_empty() {
            return;
        }
        // line breaks as stored (U+2028) now, so every offset counted is a stored one
        let t = &t.replace(['\n', '\r'], "\u{2028}");
        match self.p.p.runs.last_mut() {
            Some((s, g)) if g == f => s.push_str(t),
            _ => self.p.p.runs.push((t.to_string(), f.clone())),
        }
    }

    fn field(&mut self, f: Field, fmt: &RunFmt) {
        let at = self.len();
        self.p.x.fields.push((at, f));
        // a stand-in character, replaced by the field when built
        self.push("#", fmt);
    }
}

/// A field whose shown result is left out (a table of contents: Pages builds its own).
fn skipped_field(instr: &str) -> bool {
    matches!(instr.split_whitespace().next().map(|s| s.to_ascii_uppercase()).as_deref(), Some("TOC"))
}

fn field_of(instr: &str) -> Option<Field> {
    match instr.split_whitespace().next()?.to_ascii_uppercase().as_str() {
        "PAGE" => Some(Field::Page),
        "NUMPAGES" | "SECTIONPAGES" => Some(Field::PageCount),
        _ => None,
    }
}

impl Reader<'_> {
    fn para(&self, p: Node, rels: &HashMap<String, String>) -> Para {
        let ctx = self.ctx;
        let ppr = child(&p, "pPr");
        let style_id = ppr.and_then(|x| child(&x, "pStyle")).and_then(|s| attr(&s, "val")).map(str::to_string).or_else(|| ctx.default_para_style.clone()).unwrap_or_default();
        // in a table cell: the table's style (or the default table style) under the paragraph's
        let table_style = p.ancestors().skip(1).find(|a| a.tag_name().name() == "tbl").map(|t| {
            child(&t, "tblPr").and_then(|x| child(&x, "tblStyle")).and_then(|s| attr(&s, "val")).map(str::to_string).or_else(|| ctx.default_table_style.clone())
        });
        let (mut base_r, mut pp) = ctx.style_chain_over(&style_id, table_style.flatten().as_deref());
        // Word layers style, then the numbering level's indents, then the paragraph's own
        let own = ctx.para_props(ppr);
        if let Some((num, lvl)) = own.num.as_ref().or(pp.num.as_ref()) {
            if let Some(lp) = ctx.levels.get(num).and_then(|l| l.get(lvl)) {
                pp.left = lp.pp.left.or(pp.left);
                pp.first = lp.pp.first.or(pp.first);
            }
        }
        pp.over(&own);
        // the paragraph mark's run properties don't style the runs
        let _ = &mut base_r;
        let mut c = Collect::default();
        c.p.p.style = ctx.pages_style(&style_id);
        if let Some((num, lvl)) = &pp.num {
            if num != "0" {
                c.p.p.list = Some((ctx.numbered(num, *lvl).unwrap_or(false), *lvl));
            }
        }
        c.p.p.align = pp.align;
        c.p.x.space_before = pp.space_before;
        c.p.x.space_after = pp.space_after;
        c.p.x.line = pp.line;
        c.p.x.left = pp.left;
        c.p.x.right = pp.right;
        c.p.x.first = pp.first;
        c.p.x.page_break_before = pp.page_break_before.unwrap_or(false);
        c.p.x.border = pp.border.flatten();
        c.p.x.style_id = style_id.clone();
        {
            let mut mark = base_r.clone();
            mark.over(&ctx.run_props(ppr.and_then(|x| child(&x, "rPr"))));
            c.p.x.mark_fmt = Some(self.run_fmt(&mark, p, &None).0);
        }
        c.p.x.keep_next = pp.keep_next.unwrap_or(false);
        c.p.x.tabs = pp.tabs.clone().unwrap_or_default();
        c.p.x.keep_lines = pp.keep_lines.unwrap_or(false);
        c.p.x.contextual = pp.contextual.unwrap_or(false);
        if let Some((num, lvl)) = pp.num.clone().filter(|_| c.p.p.list.is_some()) {
            let level = ctx.levels.get(&num).and_then(|l| l.get(&lvl)).cloned().unwrap_or_default();
            // the label: the paragraph mark's formatting with the level's over it
            let mut mark = base_r.clone();
            mark.over(&ctx.run_props(ppr.and_then(|x| child(&x, "rPr"))));
            let mut fmt = self.run_fmt(&mark, p, &None).0;
            // a Symbol / Wingdings bullet: OnlyOffice draws it with Standard Symbols PS (the
            // text's own runs carry their fonts, so only the label takes this)
            if level.rp.font.as_deref().is_some_and(|f| matches!(f.to_ascii_lowercase().as_str(), "symbol" | "wingdings" | "standard symbols ps")) {
                fmt.font = Some("Standard Symbols PS".into());
                // (a symbol font's own code for the character, U+F0xx, which it maps)
                if !c.p.p.list.is_some_and(|l| l.0) && !level.text.is_empty() {
                    c.p.x.label_text = Some(level.text.clone());
                }
            }
            // (the level's own size: the label's scale; its font is the bullet's, not the text's)
            let mark_size = fmt.size.unwrap_or(11.0).max(1.0);
            let size = level.rp.size.unwrap_or(mark_size);
            c.p.x.label_scale = size / mark_size;
            // its width, near enough to find the tab stop after it (bullets and digits)
            let width: f32 = level.text.replace(|c: char| c == '%', "").chars().map(|c| if c.is_ascii_digit() { 0.5 } else if c.is_alphabetic() { 0.55 } else if matches!(c, '.' | ')' | ',' | ':') { 0.28 } else { 0.35 }).sum::<f32>() * size;
            let left = pp.left.unwrap_or(0.0);
            let label = left + pp.first.unwrap_or(0.0);
            let end = label + width;
            let next_stop = |x: f32| ((x / ctx.default_tab).floor() + 1.0) * ctx.default_tab;
            // the text after the label: at a tab stop after it (the hanging indent first, unless
            // the paragraph is in a frame; then the level's own tab, then the default stops),
            // after a space, or right after it
            let first_text = match level.suffix.as_str() {
                "nothing" => end,
                "space" => end + 0.25 * size,
                _ => {
                    let indent = (left > end && pp.frame != Some(true)).then_some(left);
                    let own = level.tab.filter(|t| *t > end);
                    indent.into_iter().chain(own).fold(None, |m: Option<f32>, v| Some(m.map_or(v, |m| m.min(v)))).unwrap_or_else(|| next_stop(end))
                }
            };
            // the other lines at the left indent
            c.p.x.list_geom = Some((label, left, first_text));
            // the whole list's levels (one Letters list style per Word list, so each list counts
            // on its own and its lower levels restart under a higher one)
            c.p.x.num_id = Some(num.clone());
            let mut lvls: Vec<(u32, Level)> = ctx.levels.get(&num).map(|l| l.iter().map(|(i, v)| (*i, v.clone())).collect()).unwrap_or_default();
            lvls.sort_by_key(|l| l.0);
            c.p.x.list_levels = lvls
                .iter()
                .filter(|(i, _)| *i < 9)
                .map(|(i, lv)| {
                    let (l_left, l_label) = if *i == lvl { (left, label) } else { let ll = lv.pp.left.unwrap_or(0.0); (ll, ll + lv.pp.first.unwrap_or(0.0)) };
                    let (number, tiered) = word_number_type(&lv.fmt, &lv.text);
                    let symbol = lv.rp.font.as_deref().is_some_and(|f| matches!(f.to_ascii_lowercase().as_str(), "symbol" | "wingdings")) && !lv.numbered && !lv.text.is_empty();
                    ListLevelIn { level: *i, gap: l_left - l_label, numbered: lv.numbered, number, tiered, scale: lv.rp.size.map(|s| s / mark_size).unwrap_or(1.0), label_text: symbol.then(|| lv.text.clone()) }
                })
                .collect();
            c.p.x.label_fmt = Some(fmt);
        }
        self.walk(p, None, &base_r, &mut c, rels);
        if c.p.p.runs.iter().all(|r| r.0.is_empty()) {
            // (the paragraph mark's own w:rPr is in w:pPr)
            c.p.x.mark = Some(self.run_fmt(&base_r, ppr.unwrap_or(p), &None).0);
        }
        // highlights collected per run
        let mut off = 0;
        for (t, f) in &c.p.p.runs {
            let _ = f;
            off += t.len();
        }
        let _ = off;
        c.p
    }

    fn run_fmt(&self, base: &RunProps, r: Node, link: &Option<String>) -> (RunFmt, Option<Color>) {
        let ctx = self.ctx;
        let mut rp = base.clone();
        let rpr = child(&r, "rPr");
        if let Some(cs) = rpr.and_then(|x| child(&x, "rStyle")).and_then(|s| attr(&s, "val")) {
            if let Some(s) = ctx.styles.get(cs) {
                rp.over(&s.rp);
            }
        }
        rp.over(&ctx.run_props(rpr));
        let f = RunFmt {
            bold: rp.bold.unwrap_or(false),
            italic: rp.italic.unwrap_or(false),
            underline: rp.underline.unwrap_or(false),
            strike: rp.strike.unwrap_or(false),
            size: rp.size,
            color: rp.color,
            font: rp.font.clone(),
            script: rp.script.unwrap_or(0),
            link: link.clone(),
            spacing: rp.spacing.filter(|v| *v != 0.0),
            caps: if rp.caps == Some(true) {
                1
            } else if rp.small_caps == Some(true) {
                2
            } else {
                0
            },
        };
        (f, rp.highlight)
    }

    fn walk(&self, n: Node, link: Option<String>, base: &RunProps, c: &mut Collect, rels: &HashMap<String, String>) {
        for e in n.children() {
            match e.tag_name().name() {
                "r" => self.run(e, &link, base, c, rels),
                "hyperlink" => {
                    let url = attr(&e, "id").and_then(|id| rels.get(id)).cloned().or_else(|| attr(&e, "anchor").map(|a| format!("#{a}")));
                    self.walk(e, url, base, c, rels);
                }
                "fldSimple" => {
                    let instr = attr(&e, "instr").unwrap_or("");
                    match field_of(instr) {
                        Some(f) => {
                            let fmt = e.children().find(|r| r.tag_name().name() == "r").map(|r| self.run_fmt(base, r, &link).0).unwrap_or_default();
                            c.field(f, &fmt);
                        }
                        None => self.walk(e, link.clone(), base, c, rels),
                    }
                }
                "ins" | "smartTag" | "sdt" | "sdtContent" | "customXml" | "moveTo" => self.walk(e, link.clone(), base, c, rels),
                "commentRangeStart" => {
                    if let Some(id) = attr(&e, "id") {
                        let at = c.len();
                        c.open_comments.insert(id.to_string(), at);
                    }
                }
                "commentRangeEnd" => {
                    if let Some(id) = attr(&e, "id") {
                        if let (Some(a), Some((t, who))) = (c.open_comments.remove(id), self.ctx.comments.get(id)) {
                            let b = c.len();
                            c.p.x.comments.push((a, b.max(a), t.clone(), who.clone()));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn run(&self, r: Node, link: &Option<String>, base: &RunProps, c: &mut Collect, rels: &HashMap<String, String>) {
        let (f, hl) = self.run_fmt(base, r, link);
        let start = c.len();
        for e in r.children() {
            // a complex field: its instruction, then its shown result (skipped for PAGE / NUMPAGES)
            match e.tag_name().name() {
                "fldChar" => {
                    match attr(&e, "fldCharType") {
                        Some("begin") => c.field = Some((String::new(), false)),
                        Some("separate") => {
                            if let Some((instr, sep)) = c.field.as_mut() {
                                *sep = true;
                                if skipped_field(instr) {
                                    c.field = Some(("\u{0}skip".into(), true));
                                } else if let Some(fl) = field_of(instr) {
                                    c.field = None;
                                    c.field(fl, &f);
                                    // the cached result that follows up to "end" is dropped
                                    c.field = Some(("\u{0}skip".into(), true));
                                }
                            }
                        }
                        Some("end") => c.field = None,
                        _ => {}
                    }
                    continue;
                }
                "instrText" => {
                    if let Some((instr, false)) = c.field.as_mut() {
                        instr.push_str(e.text().unwrap_or(""));
                    }
                    continue;
                }
                _ => {}
            }
            if c.field.as_ref().is_some_and(|(i, sep)| !*sep || i == "\u{0}skip") {
                // inside a field's instruction, or a skipped result
                continue;
            }
            match e.tag_name().name() {
                "t" => c.push(e.text().unwrap_or(""), &f),
                "tab" | "ptab" => c.push("\t", &f),
                "br" | "cr" => {
                    if attr(&e, "type") == Some("page") {
                        c.p.p.page_break_after = true;
                    } else if attr(&e, "type") == Some("column") {
                        c.p.x.column_break_after = true;
                    } else {
                        c.push("\n", &f);
                    }
                }
                "noBreakHyphen" => c.push("\u{2011}", &f),
                "sym" => {
                    if let Some(ch) = attr(&e, "char").and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32) {
                        c.push(&ch.to_string(), &f);
                    }
                }
                "footnoteReference" => {
                    if let Some(t) = attr(&e, "id").and_then(|id| self.ctx.footnotes.get(id)) {
                        let at = c.len();
                        c.p.x.footnotes.push((at, t.clone()));
                    }
                }
                "drawing" => self.drawing(e, c, rels),
                "pict" | "object" => {
                    // VML: an image (v:imagedata) or a text box
                    if let Some(id) = e.descendants().find(|n| n.tag_name().name() == "imagedata").and_then(|n| attr(&n, "id")) {
                        if let Some(bytes) = rels.get(id).and_then(|t| self.files.get(t)) {
                            let at = c.len();
                            c.p.p.images.push((bytes.clone(), rels[id].rsplit('/').next().unwrap_or("image.png").to_string()));
                            c.p.x.image_at.push(at);
                            c.p.x.image_size.push(None);
                        }
                    }
                }
                "AlternateContent" => {
                    // the newer choice (DrawingML) over the VML fallback
                    if let Some(d) = e.descendants().find(|n| n.tag_name().name() == "drawing") {
                        self.drawing(d, c, rels);
                    }
                }
                _ => {}
            }
        }
        if let Some(col) = hl {
            let end = c.len();
            if end > start {
                c.p.x.highlights.push((start, end, col));
            }
        }
    }

    fn drawing(&self, d: Node, c: &mut Collect, rels: &HashMap<String, String>) {
        let Some(holder) = d.children().find(|n| matches!(n.tag_name().name(), "inline" | "anchor")) else { return };
        let ext = child(&holder, "extent");
        let (w, h) = (emu(ext.and_then(|e| attr(&e, "cx"))).unwrap_or(100.0), emu(ext.and_then(|e| attr(&e, "cy"))).unwrap_or(100.0));
        let pic = holder.descendants().find(|n| n.tag_name().name() == "blip").and_then(|b| attr(&b, "embed")).and_then(|id| {
            let t = rels.get(id)?;
            Some((self.files.get(t)?.clone(), t.rsplit('/').next().unwrap_or("image.png").to_string()))
        });
        let textbox = holder.descendants().find(|n| n.tag_name().name() == "txbxContent").map(|t| {
            let ps: Vec<InPara> = t.children().filter(|p| p.tag_name().name() == "p").map(|p| self.para(p, rels).p).collect();
            ps
        });
        let wsp = holder.descendants().find(|n| n.tag_name().name() == "wsp");
        // a chart (c:chart naming its part): Letters' own chart
        let chart = holder
            .descendants()
            .find(|n| n.tag_name().name() == "chart" && n.tag_name().namespace().is_some_and(|ns| ns.ends_with("/chart")))
            .and_then(|n| n.attributes().find(|a| a.name() == "id").map(|a| a.value().to_string()))
            .and_then(|id| rels.get(&id).cloned())
            .and_then(|t| text_of(self.files, &t))
            .and_then(|x| chart_of(&x, &self.ctx.accents));
        if let Some(ci) = chart {
            let kind = AnchorKind::Chart(ci);
            let a = self.anchor_of(holder, kind, w, h);
            if holder.tag_name().name() == "inline" {
                let at = c.len();
                c.p.x.inline_shapes.push((at, a));
            } else {
                c.p.x.anchors.push(a);
            }
            return;
        }
        if holder.tag_name().name() == "inline" {
            if let Some((bytes, name)) = pic {
                let at = c.len();
                c.p.p.images.push((bytes, name));
                c.p.x.image_at.push(at);
                c.p.x.image_size.push(ext.map(|_| (w, h)));
                return;
            }
            // an inline shape: kept as a floating one at the paragraph
            if wsp.is_none() {
                return;
            }
        }
        let kind = match (pic, textbox) {
            (_, Some(tb)) if tb.iter().any(|p| p.runs.iter().any(|r| !r.0.trim().is_empty())) => AnchorKind::TextBox(tb),
            (Some((b, n)), _) => AnchorKind::Picture(b, n),
            _ => match wsp {
                Some(s) => AnchorKind::Shape(shape_of(s, w, h)),
                None => return,
            },
        };
        let a = self.anchor_of(holder, kind, w, h);
        if holder.tag_name().name() == "inline" {
            let at = c.len();
            c.p.x.inline_shapes.push((at, a));
        } else {
            c.p.x.anchors.push(a);
        }
    }

    /// A drawing's placement (position, wrap, behind the text) around what it holds.
    fn anchor_of(&self, holder: Node, kind: AnchorKind, w: f32, h: f32) -> Anchor {
        let pos = |tag: &str| -> (f32, String, Option<String>) {
            let Some(p) = child(&holder, tag) else { return (0.0, "column".into(), None) };
            let from = attr(&p, "relativeFrom").unwrap_or("column").to_string();
            let off = child(&p, "posOffset").and_then(|o| o.text()).and_then(|t| emu(Some(t))).unwrap_or(0.0);
            let align = child(&p, "align").and_then(|o| o.text()).map(str::to_string);
            (off, from, align)
        };
        let (x, x_from, x_align) = pos("positionH");
        let (y, y_from, y_align) = pos("positionV");
        let wrap = if holder.children().any(|n| n.tag_name().name() == "wrapTopAndBottom") {
            3
        } else if holder.children().any(|n| matches!(n.tag_name().name(), "wrapSquare" | "wrapTight" | "wrapThrough")) {
            5
        } else {
            2
        };
        let behind = attr(&holder, "behindDoc").is_some_and(|v| v == "1" || v == "true");
        Anchor { kind, w, h, x, x_from, x_align, y, y_from, y_align, wrap, behind }
    }

    fn table(&self, t: Node, rels: &HashMap<String, String>) -> Table {
        let mut out = Table::default();
        if let Some(g) = child(&t, "tblGrid") {
            out.col_widths = g.children().filter(|c| c.tag_name().name() == "gridCol").map(|c| twips(attr(&c, "w")).unwrap_or(72.0).max(12.0)).collect();
        }
        let tblpr = child(&t, "tblPr");
        out.margins = [CELL_PAD, 0.0, CELL_PAD, 0.0];
        out.autofit = tblpr.and_then(|p| child(&p, "tblLayout")).and_then(|l| attr(&l, "type")) != Some("fixed");
        if let Some(m) = tblpr.and_then(|p| child(&p, "tblCellMar")) {
            for (i, sides) in [["left", "start"], ["top", "top"], ["right", "end"], ["bottom", "bottom"]].iter().enumerate() {
                if let Some(v) = sides.iter().find_map(|s| child(&m, s)).and_then(|e| twips(attr(&e, "w"))) {
                    out.margins[i] = v;
                }
            }
        }
        out.align = match tblpr.and_then(|p| child(&p, "jc")).and_then(|j| attr(&j, "val")) {
            Some("center") => 1,
            Some("right" | "end") => 2,
            _ => 0,
        };
        let ind = tblpr.and_then(|p| child(&p, "tblInd")).and_then(|e| twips(attr(&e, "w"))).unwrap_or(0.0);
        out.indent = if self.ctx.compat < 15 { ind - out.margins[0] } else { ind };
        let style_named_grid = tblpr.and_then(|p| child(&p, "tblStyle")).and_then(|s| attr(&s, "val")).is_some_and(|v| {
            let n = self.ctx.styles.get(v).map(|s| s.name.to_lowercase()).unwrap_or_else(|| v.to_lowercase());
            n.contains("grid") || n.contains("table normal") && false
        });
        let has_borders = tblpr.and_then(|p| child(&p, "tblBorders")).is_some_and(|b| b.children().any(|e| e.is_element() && attr(&e, "val").is_some_and(|v| v != "nil" && v != "none")));
        out.borders = style_named_grid || has_borders;
        // its colour and width (the inside lines', else the outside ones')
        out.border_line = tblpr.and_then(|p| child(&p, "tblBorders")).and_then(|b| {
            ["insideH", "insideV", "top", "left", "bottom", "right"].iter().filter_map(|s| child(&b, s)).find(|e| attr(e, "val").is_some_and(|v| v != "nil" && v != "none")).map(|e| {
                (attr(&e, "color").and_then(hex_color).unwrap_or(Color::BLACK), attr(&e, "sz").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 8.0).unwrap_or(0.5).max(0.25))
            })
        });
        for tr in t.children().filter(|c| c.tag_name().name() == "tr") {
            let trpr = child(&tr, "trPr");
            let height = trpr.and_then(|p| child(&p, "trHeight")).and_then(|h| twips(attr(&h, "val")));
            out.exact_rows.push(trpr.and_then(|p| child(&p, "trHeight")).and_then(|h| attr(&h, "hRule")) == Some("exact"));
            let header = trpr.and_then(|p| child(&p, "tblHeader")).is_some();
            let mut cells = Vec::new();
            for tc in tr.children().filter(|c| c.tag_name().name() == "tc") {
                let tcpr = child(&tc, "tcPr");
                let span = tcpr.and_then(|p| child(&p, "gridSpan")).and_then(|s| attr(&s, "val")).and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
                let vmerge = tcpr.and_then(|p| child(&p, "vMerge")).map(|v| attr(&v, "val") == Some("restart"));
                let fill = tcpr.and_then(|p| child(&p, "shd")).and_then(|s| attr(&s, "fill")).and_then(hex_color);
                let mut margins = [None; 4];
                if let Some(m) = tcpr.and_then(|p| child(&p, "tcMar")) {
                    for (i, sides) in [["left", "start"], ["top", "top"], ["right", "end"], ["bottom", "bottom"]].iter().enumerate() {
                        margins[i] = sides.iter().find_map(|s| child(&m, s)).and_then(|e| twips(attr(&e, "w")));
                    }
                }
                let paras = tc.children().filter(|p| p.tag_name().name() == "p").map(|p| self.para(p, rels)).collect();
                let mut borders: [Option<Option<(Color, f32)>>; 4] = Default::default();
                if let Some(b) = tcpr.and_then(|p| child(&p, "tcBorders")) {
                    for (i, side) in ["top", "left", "bottom", "right"].iter().enumerate() {
                        let e = child(&b, side).or_else(|| child(&b, match *side { "left" => "start", "right" => "end", _ => side }));
                        if let Some(e) = e {
                            let val = attr(&e, "val").unwrap_or("single");
                            borders[i] = Some(if val == "nil" || val == "none" {
                                None
                            } else {
                                let w = attr(&e, "sz").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 8.0).unwrap_or(0.5);
                                Some((attr(&e, "color").and_then(hex_color).unwrap_or(Color::BLACK), w.max(0.25)))
                            });
                        }
                    }
                }
                let width = tcpr.and_then(|p| child(&p, "tcW")).filter(|w| attr(w, "type").is_none_or(|t| t == "dxa")).and_then(|w| twips(attr(&w, "w"))).filter(|w| *w > 0.0);
                cells.push(Cell { paras, fill, borders, span, vmerge, margins, width });
            }
            out.rows.push((height, header, cells));
        }
        let cols = out.rows.iter().map(|r| r.2.iter().map(|c| c.span).sum::<u32>()).max().unwrap_or(1) as usize;
        if out.col_widths.len() < cols {
            out.col_widths.resize(cols, 72.0);
        }
        out.overhang = if self.ctx.compat < 15 { out.margins[0] + out.margins[2] } else { 0.0 };
        out.keep_together = !out.rows.is_empty()
            && t.children().filter(|r| r.tag_name().name() == "tr").all(|r| child(&r, "trPr").and_then(|p| child(&p, "cantSplit")).is_some())
            && out.rows.iter().all(|r| r.2.iter().all(|c| c.paras.iter().all(|q| q.x.keep_next)));
        // a row's cell widths over the grid when they disagree (Word and OnlyOffice lay
        // the columns out from them; generated files often leave an even grid behind): a cell
        // spanning columns shares its width out as the grid does
        // (the first row with a cell per column: a spanning cell's width is often off)
        let row_of_cells = out.rows.iter().map(|r| &r.2).find(|r| r.len() == out.col_widths.len() && r.iter().all(|c| c.span == 1));
        if let Some(first) = row_of_cells {
            let covers = first.iter().map(|c| c.span as usize).sum::<usize>() == out.col_widths.len();
            if covers && first.iter().all(|c| c.width.is_some()) {
                let mut widths = out.col_widths.clone();
                let mut at = 0usize;
                for c in first {
                    let span = c.span as usize;
                    let grid: f32 = out.col_widths[at..at + span].iter().sum();
                    for k in at..at + span {
                        widths[k] = c.width.unwrap() * if grid > 0.0 { out.col_widths[k] / grid } else { 1.0 / span as f32 };
                    }
                    at += span;
                }
                // (even a few twips: OnlyOffice lays the cells out at their own widths, and a
                // line that just fits there must fit here)
                if widths.iter().zip(&out.col_widths).any(|(a, b)| (a - b).abs() > 0.001) {
                    out.col_widths = widths;
                }
            }
        }
        out
    }

    /// A header / footer part: its paragraphs sorted into Pages' left / centre / right fields by
    /// their tabs and alignment.
    fn header_footer(&self, root: Node, rels: &HashMap<String, String>) -> HeaderFooter {
        let mut hf = HeaderFooter::default();
        for p in root.descendants().filter(|n| n.tag_name().name() == "p" && !n.ancestors().skip(1).any(|a| a.tag_name().name() == "txbxContent")) {
            let para = self.para(p, rels);
            hf.anchors.extend(para.x.anchors.iter().cloned());
            // split at tabs: text after one tab is the centre field, after two the right
            let mut parts: [Para; 3] = Default::default();
            // in a table cell: where Word puts its text across the column (the table's grid and
            // cell margins)
            let cell_box = p.ancestors().find(|a| a.tag_name().name() == "tc").and_then(|tc| {
                let tbl = tc.ancestors().find(|a| a.tag_name().name() == "tbl")?;
                let grid: Vec<f32> = child(&tbl, "tblGrid").map(|g| g.children().filter(|c| c.tag_name().name() == "gridCol").map(|c| twips(attr(&c, "w")).unwrap_or(0.0)).collect()).unwrap_or_default();
                let t = self.table(tbl, rels);
                let mut col = 0usize;
                for c in tc.parent()?.children().filter(|c| c.tag_name().name() == "tc") {
                    let span = child(&c, "tcPr").and_then(|q| child(&q, "gridSpan")).and_then(|s| attr(&s, "val")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
                    if c == tc {
                        let x0: f32 = grid.iter().take(col).sum();
                        let x1: f32 = grid.iter().take(col + span).sum();
                        return Some((x0 + t.margins[0], x1 - t.margins[2], t.clone()));
                    }
                    col += span;
                }
                None
            });
            for part in &mut parts {
                part.p.style = "Body".into();
                // the paragraph's own spacing (Word's line height, the space after)
                part.x.line = para.x.line;
                part.x.space_before = para.x.space_before;
                part.x.space_after = para.x.space_after;
                part.x.cell_box = cell_box.clone();
            }
            let mut slot = match para.p.align {
                Some(Align::Center) => 1usize,
                Some(Align::Right) => 2,
                _ => 0,
            };
            let mut off = 0usize;
            let mut fields = para.x.fields.clone();
            fields.sort_by_key(|f| f.0);
            for (t, f) in &para.p.runs {
                for (i, seg) in t.split('\t').enumerate() {
                    if i > 0 {
                        slot = (slot + 1).min(2);
                        off += 1;
                    }
                    let tgt = &mut parts[slot];
                    let base = tgt.p.runs.iter().map(|r| r.0.len()).sum::<usize>();
                    for (at, fl) in &fields {
                        if *at >= off && *at < off + seg.len() {
                            tgt.x.fields.push((base + (at - off), *fl));
                        }
                    }
                    if !seg.is_empty() {
                        tgt.p.runs.push((seg.to_string(), f.clone()));
                    }
                    off += seg.len();
                }
            }
            for (i, part) in parts.into_iter().enumerate() {
                if !part.p.runs.is_empty() {
                    hf.fields[i].push(part);
                }
            }
        }
        // paragraph borders (a rule above or below the footer's text): lines on the section
        // template, placed from the block's top (y_from "hf-top"; full text width)
        let mut y = 0.0;
        let height = |n: Node| -> f32 {
            match n.tag_name().name() {
                "p" => para_height(&self.para(n, rels)),
                "tbl" => n.children().filter(|r| r.tag_name().name() == "tr").map(|tr| tr.children().filter(|c| c.tag_name().name() == "tc").map(|tc| tc.children().filter(|p| p.tag_name().name() == "p").map(|p| para_height(&self.para(p, rels))).sum::<f32>()).fold(0.0f32, f32::max)).sum(),
                _ => 0.0,
            }
        };
        for n in root.children().filter(|n| n.is_element()) {
            let mut hgt = height(n);
            if n.tag_name().name() == "p" {
                if let Some(b) = self.para(n, rels).x.border.filter(|b| b.edges & 5 != 0) {
                    hgt += if b.edges & 1 != 0 { b.space + b.width } else { 0.0 } + if b.edges & 4 != 0 { b.space + b.width } else { 0.0 };
                    let line = |at: f32| Anchor {
                        kind: AnchorKind::Shape(ShapeIn { line: true, stroke: Some(Some((b.color, b.width))), ..Default::default() }),
                        w: 0.0,
                        h: 0.0,
                        x: 0.0,
                        x_from: "column".into(),
                        x_align: None,
                        y: at,
                        y_from: "hf-top".into(),
                        y_align: None,
                        wrap: 2,
                        behind: true,
                    };
                    let before = self.para(n, rels).x.space_before.unwrap_or(0.0);
                    if b.edges & 1 != 0 {
                        hf.anchors.push(line(y + before));
                    }
                    if b.edges & 4 != 0 {
                        hf.anchors.push(line(y + hgt - self.para(n, rels).x.space_after.unwrap_or(0.0)));
                    }
                }
            }
            y += hgt;
        }
        hf.height = y;
        hf
    }
}

// ---------------------------------------------------------------------- building

/// The Pages paragraph styles Word styles are mapped onto, stripped of what a Word style can't
/// say and so never meant: Pages' rules (borders), tracking, caps, shadows, outlines, indents,
/// spacing. Whatever the Word document does set is written over them per paragraph.
fn word_neutral_styles(p: &mut Pages, h: &mut History) {
    for name in ["Body", "Title", "Subtitle", "Heading", "Heading 2", "Heading 3", "Caption", "Quote"] {
        let Some(id) = crate::edit::para_style_named(&p.doc, name) else { continue };
        p.doc.touch(h, id);
        let Some(m) = p.doc.file.obj_mut(id) else { continue };
        let cp = m.msg_mut_or_create("char_properties");
        cp.set_f32("tracking", 0.0);
        cp.set_f32("baseline_shift", 0.0);
        cp.set_enum("capitalization", "kNoCaps");
        cp.set_enum("underline", "kNoUnderline");
        cp.set_enum("strikethru", "kNoStrikethru");
        cp.set_enum("superscript", "kNoScript");
        cp.set_bool("background_color_null", true);
        cp.set_bool("shadow_null", true);
        cp.set_bool("tsd_stroke_null", true);
        let pp = m.msg_mut_or_create("para_properties");
        pp.remove("stroke");
        pp.set_u64("border_positions", 0);
        for f in ["left_indent", "right_indent", "first_line_indent", "space_before", "space_after"] {
            pp.set_f32(f, 0.0);
        }
        pp.remove("line_spacing");
        let ls = pp.msg_mut_or_create("line_spacing");
        ls.set_enum("mode", "kRelativeLineSpacing");
        ls.set_f32("amount", 1.0);
        kn_format::objects::fix_override_count(m);
    }
}

/// Paragraph and character properties of `paras` written over `storage` (whose text is already
/// `text`), with `starts` the byte offset of each paragraph.
fn format_storage(p: &mut Pages, h: &mut History, storage: kn_format::ObjId, paras: &[Para], starts: &[usize], body: bool) {
    let lists = crate::edit::list_styles(&p.doc);
    let bullet = lists.iter().find(|l| l.1 == "Bullet").map(|l| l.0);
    let numbered = lists.iter().find(|l| l.1 == "Numbered").map(|l| l.0);
    // Word's list geometry as list style variations, one per (kind, level, label, gap)
    // Word lists by w:numId: the list style made for each
    let mut lists_of: HashMap<String, kn_format::ObjId> = HashMap::new();
    for (i, para) in paras.iter().enumerate() {
        let at = starts[i];
        let ip = &para.p;
        if body && ip.style != "Body" && !ip.style.is_empty() {
            if let Some(s) = crate::edit::para_style_named(&p.doc, &ip.style) {
                crate::edit::set_para_style(&mut p.doc, h, storage, at, at, s);
            }
        }
        if let (true, Some((num, level))) = (body, ip.list) {
            let l = if num { numbered } else { bullet };
            if l.is_some() {
                // the label's gap to the text is in ems of the label's own size (the mark's)
                let em = para.x.label_fmt.as_ref().and_then(|f| f.size).or_else(|| ip.runs.first().and_then(|r| r.1.size)).unwrap_or(11.0).max(1.0);
                // (one list style per Word list; a bullet paragraph with a mark of another size
                // gets its own, as the gap is in ems of the mark; numbered ones share one, so
                // they count on)
                let list_key = para.x.num_id.as_ref().map(|k| if num { k.clone() } else { format!("{k}@{em}") });
                match list_key.as_ref().and_then(|k| lists_of.get(k).copied()) {
                    // a later paragraph of a Word list: its list's style (counting on)
                    Some(v) => {
                        crate::edit::set_list_style(&mut p.doc, h, storage, at, at, Some(v));
                        if level > 0 {
                            crate::edit::change_level(&mut p.doc, h, storage, at, at, level as i32);
                        }
                    }
                    None => {
                        crate::edit::set_list_style(&mut p.doc, h, storage, at, at, l);
                        if level > 0 {
                            crate::edit::change_level(&mut p.doc, h, storage, at, at, level as i32);
                        }
                        // the first: a list style holding all the Word list's levels
                        for lv_in in &para.x.list_levels {
                            let lv_in = lv_in.clone();
                            crate::edit::set_list_level(&mut p.doc, h, storage, at, at, lv_in.level as usize, &|lv| {
                                lv.indent = 0.0;
                                lv.text_indent = lv_in.gap / em;
                                lv.scale = if lv_in.scale > 0.0 { lv_in.scale } else { 1.0 };
                                lv.kind = if lv_in.numbered { 3 } else { 2 };
                                lv.number = lv_in.number;
                                lv.tiered = lv_in.tiered;
                                if let Some(t) = &lv_in.label_text {
                                    lv.string = t.clone();
                                } else if !lv_in.numbered && (lv.string.is_empty() || lv.string == "\u{f0b7}") {
                                    lv.string = "•".into();
                                }
                            });
                        }
                        if let (Some(k), Some(v)) = (list_key, crate::edit::list_at(&p.doc, storage, at).0) {
                            lists_of.insert(k, v);
                        }
                    }
                }
            }
        }
        let x = para.x.clone();
        let align = ip.align;
        let list = ip.list.is_some() && body;
        let geom = para.x.list_geom.filter(|_| list);
        crate::edit::set_para_props(&mut p.doc, h, storage, at, at, &|pp| {
            if let Some(a) = align {
                pp.set_enum(
                    "alignment",
                    match a {
                        Align::Right => "TATvalue1",
                        Align::Center => "TATvalue2",
                        Align::Justify => "TATvalue3",
                        _ => "TATvalue0",
                    },
                );
            }
            if let Some(v) = x.space_before {
                pp.set_f32("space_before", v);
            }
            if let Some(v) = x.space_after {
                pp.set_f32("space_after", v);
            }
            // paragraph border: Pages' edges (1 top, 2 bottom; 15 a box, as its older
            // DeprecatedParagraphBorderType counts them) and Word's distance from the text
            if let Some(b) = x.border.filter(|b| b.edges & 5 != 0) {
                pp.remove("stroke");
                pp.push_msg("stroke", crate::edit::para_stroke_msg(&kn_model::format::StrokeSpec { color: b.color, width: b.width, dash: kn_model::format::Dash::Solid }));
                let edges = if b.edges == 15 { 15 } else { u64::from(b.edges & 1) | if b.edges & 4 != 0 { 2 } else { 0 } };
                pp.set_u64("border_positions", edges);
                let off = pp.msg_mut_or_create("historical_rule_offset");
                off.set_f32("x", 0.0);
                off.set_f32("y", b.space);
            }
            if let Some((mode, amount)) = x.line {
                pp.remove("line_spacing");
                let ls = pp.msg_mut_or_create("line_spacing");
                ls.set_enum("mode", mode);
                ls.set_f32("amount", amount);
            }
            // a Word list: the label at the paragraph's indent, the text a list-style gap after it
            // (the first line's text elsewhere than the other lines': where the first line
            // starts, as Pages measures it, from the edge)
            if let Some((label, cont, first)) = geom {
                pp.set_f32("left_indent", label);
                pp.set_f32("first_line_indent", if (first - cont).abs() > 0.01 { first } else { label });
            }
            // other lists keep Pages' indents (the bullet sits in them)
            if !list {
                // (Word allows negative indents: a table hanging into the margin, a paragraph
                // outdented)
                if let Some(v) = x.left {
                    pp.set_f32("left_indent", v);
                }
                // Pages measures the first line from the edge (Word: from the left indent, which
                // a paragraph with no first-line indent of its own starts at too)
                if x.first.is_some() || x.left.is_some() {
                    pp.set_f32("first_line_indent", x.left.unwrap_or(0.0) + x.first.unwrap_or(0.0));
                }
            }
            if let Some(v) = x.right {
                pp.set_f32("right_indent", v);
            }
            pp.set_bool("keep_with_next", x.keep_next);
            // Word's tab stops (Pages': the same positions, alignments and leaders)
            if !x.tabs.is_empty() {
                pp.remove("tabs");
                let t = pp.msg_mut_or_create("tabs");
                for &(pos, al, lead) in &x.tabs {
                    let e = t.push_new("tabs");
                    e.set_f32("position", pos);
                    e.set_enum("alignment", ["kTabAlignmentLeft", "kTabAlignmentCenter", "kTabAlignmentRight", "kTabAlignmentDecimal"][al.min(3) as usize]);
                    if let Some(c) = lead {
                        e.set_str("leader", &c.to_string());
                    }
                }
            }
            pp.set_bool("keep_lines_together", x.keep_lines);
        });
        // a list paragraph: its mark's formatting on the paragraph style, where a Word list's
        // label takes it from
        if let Some(f) = para.x.label_fmt.clone().filter(|_| list) {
            crate::edit::set_para_char_props(&mut p.doc, h, storage, at, at, &|cp| {
                cp.set_bool("bold", f.bold);
                cp.set_bool("italic", f.italic);
                if let Some(s) = f.size {
                    cp.set_f32("font_size", s.clamp(1.0, 500.0));
                }
                if let Some(c) = f.color {
                    cp.remove("font_color");
                    cp.push_msg("font_color", kn_model::format::color_msg(c));
                }
                if let Some(fnt) = &f.font {
                    cp.set_str("font_name", &crate::import::ps_name(fnt));
                }
            });
        }
        let mut off = at;
        // an empty paragraph: its mark's formatting on the paragraph break (the line's height)
        let marked: Vec<(String, RunFmt)>;
        let runs = match &para.x.mark {
            Some(m) if ip.runs.iter().all(|r| r.0.is_empty()) && *m != RunFmt::default() && at < p.doc.storage_text(storage).len() => {
                marked = vec![("\n".to_string(), m.clone())];
                &marked
            }
            _ => &ip.runs,
        };
        for (t, f) in runs {
            let len = if t == "\n" && para.x.mark.is_some() { 1 } else { t.replace(['\n', '\r'], "\u{2028}").len() };
            if *f != RunFmt::default() && len > 0 {
                let f2 = f.clone();
                crate::edit::set_char_props(&mut p.doc, h, storage, off, off + len, &|cp| {
                    cp.set_bool("bold", f2.bold);
                    cp.set_bool("italic", f2.italic);
                    if f2.underline {
                        cp.set_enum("underline", "kSingleUnderline");
                    }
                    if f2.strike {
                        cp.set_enum("strikethru", "kSingleStrikethru");
                    }
                    if let Some(s) = f2.size {
                        cp.set_f32("font_size", s.clamp(1.0, 500.0));
                    }
                    if let Some(c) = f2.color {
                        cp.remove("font_color");
                        cp.push_msg("font_color", kn_model::format::color_msg(c));
                    }
                    if let Some(fnt) = &f2.font {
                        cp.set_str("font_name", &crate::import::ps_name(fnt));
                    }
                    if f2.script != 0 {
                        cp.set_enum("superscript", if f2.script > 0 { "kSuperscript" } else { "kSubscript" });
                    }
                    if f2.caps != 0 {
                        cp.set_enum("capitalization", if f2.caps == 1 { "kAllCaps" } else { "kSmallCaps" });
                    }
                    // Word's spacing in points, Pages' tracking in ems of the size
                    if let Some(sp) = f2.spacing {
                        cp.set_f32("tracking", sp / f2.size.unwrap_or(11.0).max(1.0));
                    }
                });
                if let Some(url) = &f.link {
                    p.add_link(h, storage, off, off + len, url);
                }
            }
            off += len;
        }
        // a paragraph ending in a line break: its mark's formatting on the paragraph break (the
        // empty last line's height)
        if let Some(m) = para.x.mark_fmt.as_ref().filter(|m| **m != RunFmt::default() && runs.last().is_some_and(|r| r.0.ends_with(['\n', '\u{2028}']) && r.1 != **m)) {
            if off < p.doc.storage_text(storage).len() {
                let m = m.clone();
                crate::edit::set_char_props(&mut p.doc, h, storage, off, off + 1, &|cp| {
                    cp.set_bool("bold", m.bold);
                    cp.set_bool("italic", m.italic);
                    if let Some(s) = m.size {
                        cp.set_f32("font_size", s.clamp(1.0, 500.0));
                    }
                    if let Some(fnt) = &m.font {
                        cp.set_str("font_name", &crate::import::ps_name(fnt));
                    }
                });
            }
        }
        for (a, b, col) in &para.x.highlights {
            let col = *col;
            crate::edit::set_char_props(&mut p.doc, h, storage, at + a, at + b, &|cp| {
                cp.remove("background_color");
                cp.push_msg("background_color", kn_model::format::color_msg(col));
            });
        }
    }
}

/// Paragraphs as storage text ("\n" between paragraphs, line breaks as U+2028, a page break
/// after one as U+0005) and each paragraph's start.
fn storage_text(paras: &[Para]) -> (String, Vec<usize>) {
    let mut text = String::new();
    let mut starts = Vec::new();
    for (i, para) in paras.iter().enumerate() {
        starts.push(text.len());
        for (t, _) in &para.p.runs {
            text.push_str(&t.replace(['\n', '\r'], "\u{2028}"));
        }
        if i + 1 < paras.len() {
            let brk = para.p.page_break_after || paras[i + 1].x.page_break_before;
            text.push(if brk {
                '\u{5}'
            } else if para.x.column_break_after {
                '\u{c}'
            } else {
                '\n'
            });
        }
    }
    (text, starts)
}

/// Word's sections: each section-ending page break (U+0005) becomes a section break (U+0004,
/// the same length, so offsets hold), and a section whose page differs from the document's
/// (the first section's) keeps its own size, orientation and margins.
fn word_sections(p: &mut Pages, h: &mut History, w: &WordDoc, paras: &[Para]) {
    if w.section_setups.is_empty() {
        return;
    }
    let Some(body) = p.body_storage() else { return };
    // which of the text's page breaks end a section, in order
    let ends: Vec<bool> = (0..paras.len().saturating_sub(1)).filter(|&i| paras[i].p.page_break_after || paras[i + 1].x.page_break_before).map(|i| paras[i].x.section_end).collect();
    let text = p.doc.storage_text(body);
    let breaks: Vec<usize> = text.match_indices('\u{5}').map(|(i, _)| i).collect();
    for (&at, _) in breaks.iter().zip(&ends).filter(|(_, e)| **e).rev() {
        crate::edit::replace(&mut p.doc, h, body, at, at + 1, "");
        let _ = p.insert_section_break(h, at);
    }
    let base = p.setup();
    let page = |s: &Setup| {
        let mut ps = base.clone();
        ps.width = s.width;
        ps.height = s.height;
        ps.landscape = s.landscape;
        ps.top = s.top;
        ps.bottom = s.bottom;
        ps.left = s.left;
        ps.right = s.right;
        ps.header_margin = s.header;
        ps.footer_margin = s.footer;
        ps
    };
    let setups: Vec<&Setup> = w.section_setups.iter().chain(w.setup.as_ref()).collect();
    for (sec, s) in p.sections().iter().zip(setups) {
        let ps = page(s);
        if ps != base {
            p.section_pages.insert(sec.id, ps);
        }
    }
}

fn word_fonts() -> kn_render::fonts::Fonts {
    let mut f = kn_render::fonts::Fonts::new();
    f.word = true;
    f
}

/// An autofit table's columns fitted to `room` as OnlyOffice fits them: no column narrower
/// than its longest word (Word breaks at spaces and after hyphens) and its cell margins; the
/// columns give up width in proportion to what they have above that.
fn autofit_widths(t: &Table, room: f32, fonts: &mut kn_render::fonts::Fonts) -> Vec<f32> {
    let mut mins = vec![0.0f32; t.col_widths.len()];
    for row in &t.rows {
        let mut col = 0usize;
        for cell in &row.2 {
            if cell.span == 1 && col < mins.len() {
                let pad = cell.margins[0].unwrap_or(t.margins[0]) + cell.margins[2].unwrap_or(t.margins[2]);
                for q in &cell.paras {
                    for (text, f) in &q.p.runs {
                        let style = kn_model::CharStyle {
                            font_name: f.font.as_deref().map(crate::import::ps_name).unwrap_or_else(|| "TimesNewRomanPSMT".into()),
                            size: f.size.unwrap_or(10.0),
                            bold: f.bold,
                            italic: f.italic,
                            ..Default::default()
                        };
                        for word in text.split_whitespace().flat_map(|w| w.split_inclusive('-')) {
                            let tb = kn_render::simple_text_box(word, style.clone(), kn_model::Align::Left, kn_model::VAlign::Top);
                            let tl = kn_render::text::layout(fonts, &tb, 1.0e5, 1.0e5, true, true);
                            let w = tl.glyphs.iter().map(|g| g.x + g.w).fold(0.0f32, f32::max);
                            mins[col] = mins[col].max(w + pad);
                        }
                    }
                }
            }
            col += cell.span as usize;
        }
    }
    // a column narrower than its words widens to them; the others share what's left
    let w = &t.col_widths;
    let fixed: f32 = (0..w.len()).filter(|&i| w[i] <= mins[i]).map(|i| mins[i]).sum();
    let flex_min: f32 = (0..w.len()).filter(|&i| w[i] > mins[i]).map(|i| mins[i]).sum();
    let slack: f32 = (0..w.len()).filter(|&i| w[i] > mins[i]).map(|i| w[i] - mins[i]).sum();
    let k = if slack > 0.0 { ((room - fixed - flex_min) / slack).clamp(0.0, 1.0) } else { 0.0 };
    (0..w.len()).map(|i| if w[i] <= mins[i] { mins[i] } else { mins[i] + (w[i] - mins[i]) * k }).collect()
}

/// Fields written in place of their stand-in characters (from the end, so offsets hold).
fn write_fields(p: &mut Pages, h: &mut History, storage: kn_format::ObjId, paras: &[Para], starts: &[usize]) {
    let mut all: Vec<(usize, Field)> = paras.iter().enumerate().flat_map(|(i, para)| para.x.fields.iter().map(move |(at, f)| (starts[i] + at, *f))).collect();
    all.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (at, f) in all {
        crate::edit::replace(&mut p.doc, h, storage, at, at + 1, "");
        crate::edit::insert_page_field(&mut p.doc, h, storage, at, f == Field::PageCount);
    }
}

fn write_header_footer(p: &mut Pages, h: &mut History, master: kn_format::ObjId, footer: bool, hf: &HeaderFooter) {
    let setup = p.setup();
    place_header_anchors(p, h, master, hf, &setup, footer);
    let storages = p.header_footer_storages(master, footer);
    for (i, sid) in storages.iter().enumerate().take(3) {
        let paras = &hf.fields[i];
        let old = p.doc.storage_text(*sid);
        if !old.is_empty() {
            crate::edit::replace(&mut p.doc, h, *sid, 0, old.len(), "");
        }
        if paras.is_empty() {
            continue;
        }
        // a field from a Word table cell: indented to the cell's place across the text
        let tw = setup.width - setup.left - setup.right;
        let paras: Vec<Para> = paras
            .iter()
            .cloned()
            .map(|mut q| {
                if let Some((x0, x1, t)) = q.x.cell_box.take() {
                    let at = t.left(tw);
                    q.x.left = Some((at + x0).max(0.0));
                    q.x.right = Some((tw - at - x1).max(0.0));
                }
                q
            })
            .collect();
        let paras = &paras;
        let (text, starts) = storage_text(paras);
        crate::edit::replace(&mut p.doc, h, *sid, 0, 0, &text);
        format_storage(p, h, *sid, paras, &starts, false);
        write_fields(p, h, *sid, paras, &starts);
    }
}

/// Word's default cell padding left and right (108 twips).
const CELL_PAD: f32 = 5.4;

/// A paragraph's height as Word lays out single-spaced lines (estimate, points).
fn para_height(p: &Para) -> f32 {
    para_height_with(p, None)
}

/// A paragraph's height on one line, Word's: its runs' and its mark's tallest line (with the
/// fonts' own metrics when `fonts` is given), its line spacing, its spacing.
fn para_height_with(p: &Para, fonts: Option<&mut kn_render::fonts::Fonts>) -> f32 {
    // an empty paragraph: its mark's size and font; else its runs' and its mark's (the last line)
    // (a paragraph with text: its runs only; OnlyOffice doesn't size it by its mark)
    let fmts: Vec<&RunFmt> = if p.p.runs.iter().all(|r| r.0.is_empty()) { p.x.mark.iter().chain(p.x.mark_fmt.iter()).take(1).collect() } else { p.p.runs.iter().filter(|r| !r.0.is_empty()).map(|r| &r.1).collect() };
    if let Some(fonts) = fonts {
        // each format's single line from the face Word draws it with
        let line = match p.x.line {
            Some(("kRelativeLineSpacing", k)) => k,
            _ => 1.0,
        };
        let tallest = fmts
            .iter()
            .map(|f| {
                let single = fonts.word_line(&f.font.as_deref().map(crate::import::ps_name).unwrap_or_default(), f.bold, f.italic).map(|(a, d, _)| a + d).unwrap_or(1.17);
                f.size.unwrap_or(10.0) * single
            })
            .fold(0.0f32, f32::max);
        let pics = p.x.image_size.iter().flatten().map(|s| s.1).fold(0.0f32, f32::max);
        let h = match p.x.line {
            Some(("kExactLineSpacing", v)) => v,
            Some(("kMinimumLineSpacing", v)) => tallest.max(v),
            _ => tallest * line,
        };
        return h.max(pics) + p.x.space_before.unwrap_or(0.0) + p.x.space_after.unwrap_or(0.0);
    }
    let size = fmts.iter().filter_map(|f| f.size).fold(0.0f32, f32::max);
    let size = if size > 0.0 { size } else { 11.0 };
    // Word's single line per point of size (OS/2 win height + external leading) of the fonts it
    // is drawn with (OnlyOffice's stand-ins)
    let font = fmts.iter().find_map(|f| f.font.clone()).unwrap_or_default().to_ascii_lowercase().replace([' ', '-'], "");
    let single = match font.as_str() {
        f if f.starts_with("calibri") || f.starts_with("carlito") => 1.2207,
        f if f.starts_with("cambria") || f.starts_with("caladea") => 1.1719,
        f if f.starts_with("courier") || f.starts_with("liberationmono") => 1.1328,
        f if f.starts_with("helvetica") || f.starts_with("arial") || f.starts_with("liberation") || f.starts_with("times") => 1.1499,
        _ => 1.17,
    };
    let line = match p.x.line {
        Some(("kRelativeLineSpacing", k)) => k,
        _ => 1.0,
    };
    let pics = p.x.image_size.iter().flatten().map(|s| s.1).fold(0.0f32, f32::max);
    (size * single * line).max(pics) + p.x.space_before.unwrap_or(0.0) + p.x.space_after.unwrap_or(0.0)
}

/// A table row's height: Word's, or what its tallest cell needs.
fn row_height(row: &(Option<f32>, bool, Vec<Cell>)) -> f32 {
    let need = row.2.iter().map(|c| c.paras.iter().map(para_height).sum::<f32>()).fold(0.0f32, f32::max);
    row.0.unwrap_or(0.0).max(need)
}

/// Pictures in a table's cells as anchors from the table's paragraph (offsets from the table's
/// top left: x_from / y_from "table").
fn cell_pictures(t: &Table) -> Vec<Anchor> {
    let mut out = Vec::new();
    let mut y = 0.0;
    for row in &t.rows {
        let mut col = 0usize;
        for cell in &row.2 {
            let x = t.col_widths.iter().take(col).sum::<f32>();
            let mut py = 0.0;
            for para in &cell.paras {
                for (k, (bytes, name)) in para.p.images.iter().enumerate() {
                    let (w, h) = para.x.image_size.get(k).copied().flatten().unwrap_or((72.0, 72.0));
                    out.push(Anchor {
                        kind: AnchorKind::Picture(bytes.clone(), name.clone()),
                        w,
                        h,
                        x: x + t.margins[0],
                        x_from: "table".into(),
                        x_align: None,
                        y: y + t.margins[1] + py + para.x.space_before.unwrap_or(0.0),
                        y_from: "table".into(),
                        y_align: None,
                        wrap: 2,
                        behind: false,
                    });
                }
                py += para_height(para);
            }
            col += cell.span.max(1) as usize;
        }
        y += row_height(row);
    }
    out
}

/// The blocks as body paragraphs: a table gets an empty paragraph of its own.
fn body_paras(blocks: &[Block]) -> (Vec<Para>, Vec<(usize, &Table)>) {
    let mut paras = Vec::new();
    let mut tables = Vec::new();
    for b in blocks {
        match b {
            Block::Para(p) => paras.push(p.clone()),
            Block::Table(t) => {
                tables.push((paras.len(), t));
                // pictures in its cells (a Pages cell holds text only): floating over their cell
                let anchors = cell_pictures(t);
                // where Word puts it across the text: its alignment, or its indent
                let align = match t.align {
                    1 => Some(Align::Center),
                    2 => Some(Align::Right),
                    _ => None,
                };
                let left = (t.align == 0 && t.indent.abs() > 0.05).then_some(t.indent);
                paras.push(Para { p: InPara { style: "Body".into(), align, ..Default::default() }, x: ParaExtra { space_before: Some(0.0), space_after: Some(0.0), left, anchors, ..Default::default() } });
            }
        }
    }
    (paras, tables)
}

fn fill_table(p: &mut Pages, h: &mut History, id: kn_format::ObjId, t: &Table, fonts: &mut kn_render::fonts::Fonts) {
    use kn_model::tablecells::CellInput;
    use kn_model::tablerows::Axis;
    let rows = t.rows.len() as u32;
    let cols = t.col_widths.len().max(1) as u32;
    let Some(cur) = p.file().obj(id).and_then(|m| kn_model::table::table(p.file(), m)) else { return };
    let (mut r0, mut c0) = (cur.rows, cur.cols);
    // headers off first (the donor's header row / column would style ours)
    for which in [kn_model::tablerows::HeaderKind::Rows, kn_model::tablerows::HeaderKind::Columns, kn_model::tablerows::HeaderKind::Footer] {
        let _ = p.doc.set_table_header_count(h, id, which, 0);
    }
    while r0 < rows.max(1) {
        if p.doc.table_insert(h, id, Axis::Row, r0, r0.saturating_sub(1)).is_err() {
            break;
        }
        r0 += 1;
    }
    while r0 > rows.max(1) {
        if p.doc.table_delete(h, id, Axis::Row, r0 - 1).is_err() {
            break;
        }
        r0 -= 1;
    }
    while c0 < cols {
        if p.doc.table_insert(h, id, Axis::Column, c0, c0.saturating_sub(1)).is_err() {
            break;
        }
        c0 += 1;
    }
    while c0 > cols {
        if p.doc.table_delete(h, id, Axis::Column, c0 - 1).is_err() {
            break;
        }
        c0 -= 1;
    }
    let widths: Vec<(u32, f32)> = t.col_widths.iter().enumerate().map(|(i, w)| (i as u32, *w)).collect();
    p.doc.set_table_sizes(h, id, false, &widths);
    // (a picture in a cell floats over it: its row keeps room for it)
    let heights: Vec<(u32, f32)> = t
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let pics = r.2.iter().flat_map(|c| c.paras.iter()).flat_map(|q| q.x.image_size.iter().flatten().map(|s| s.1)).fold(0.0f32, f32::max);
            // (no height of its own: as tall as its text, which the layout grows it to; a least
            // height is the text's, the cells' margins and top border on top, as OnlyOffice
            // lays it out)
            let around = r.2.iter().map(|c| {
                let m = |k: usize| c.margins[k].unwrap_or(t.margins[k]);
                let top = c.borders[0].flatten().map(|b| b.1).unwrap_or(if t.borders && c.borders[0].is_none() { t.border_line.map(|l| l.1).unwrap_or(0.5) } else { 0.0 });
                m(1) + m(3) + top
            }).fold(0.0f32, f32::max);
            // the cells' paragraphs on a line each, their marks too (Word's line heights, which
            // an empty cell keeps and the layout only grows from)
            let need = r.2.iter().map(|c| c.paras.iter().map(|q| para_height_with(q, Some(&mut *fonts))).sum::<f32>()).fold(0.0f32, f32::max);
            let least = if need > 0.0 { need + around } else { 4.0 };
            // (an exact height is the whole row's)
            let exact = t.exact_rows.get(i).copied().unwrap_or(false);
            (i as u32, r.0.map(|h| if exact { h.max(1.0) } else { (h + around).max(4.0) }).unwrap_or(least).max(pics))
        })
        .collect();
    p.doc.set_table_sizes(h, id, true, &heights);
    // the frame as tall and wide as its rows and columns (the sizes above move it by their
    // change, from the template table's, which needn't have matched)
    let (tw, th): (f32, f32) = (widths.iter().map(|w| w.1).sum(), heights.iter().map(|r| r.1).sum());
    p.doc.touch(h, id);
    if let Some(sz) = p.doc.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.DrawableArchive")).and_then(|d| d.msg_mut("geometry")).and_then(|g| g.msg_mut("size")) {
        sz.set_f32("width", tw.max(1.0));
        sz.set_f32("height", th.max(1.0));
    }
    // no fills but Word's, the borders Word drew
    let _ = p.doc.set_cells_fill(h, id, (0, 0, r0 - 1, c0 - 1), None);
    let _ = p.doc.set_table_banded(h, id, false);
    // Word's cell margins (text sits where Word puts it)
    let _ = p.doc.set_cells_padding(h, id, (0, 0, r0 - 1, c0 - 1), t.margins);
    let (bc, bw) = t.border_line.unwrap_or((Color::BLACK, 0.5));
    let line = t.borders.then(|| kn_model::format::StrokeSpec { color: bc, width: bw, dash: kn_model::format::Dash::Solid });
    let _ = p.doc.set_cells_border(h, id, (0, 0, r0 - 1, c0 - 1), kn_model::tablefmt::BorderEdges::All, line.as_ref());
    // cells: text, fill, bold; merges after
    let mut merges: Vec<kn_model::CellRange> = Vec::new();
    let mut vstarts: HashMap<u32, (u32, u32)> = HashMap::new(); // col -> (start row, span)
    for (ri, (_, _, cells)) in t.rows.iter().enumerate() {
        let mut col = 0u32;
        for cell in cells {
            let (r, c) = (ri as u32, col);
            if c >= c0 {
                break;
            }
            match cell.vmerge {
                Some(false) => {
                    // continues the merge above
                    if let Some((sr, sp)) = vstarts.get(&c).copied() {
                        if let Some(m) = merges.iter_mut().find(|m| m.row == sr && m.col == c) {
                            m.rows = r - sr + 1;
                        } else {
                            merges.push(kn_model::CellRange { row: sr, col: c, rows: r - sr + 1, cols: sp });
                        }
                    }
                    col += cell.span;
                    continue;
                }
                Some(true) => {
                    vstarts.insert(c, (r, cell.span));
                }
                None => {
                    vstarts.remove(&c);
                }
            }
            if cell.span > 1 && cell.vmerge.is_none() {
                merges.push(kn_model::CellRange { row: r, col: c, rows: 1, cols: cell.span.min(c0 - c) });
            }
            let text: String = cell.paras.iter().map(|p| p.p.runs.iter().map(|r| r.0.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n");
            // mixed formatting (a bold name over plain address lines, paragraphs spaced
            // differently): a rich text cell, styled run by run as the body text is
            let fmts: Vec<&RunFmt> = cell.paras.iter().flat_map(|q| q.p.runs.iter().filter(|r| !r.0.trim().is_empty()).map(|r| &r.1)).collect();
            let para_look = |q: &Para| (q.p.align, q.x.space_before, q.x.space_after, q.x.line, q.x.left, q.x.first);
            // (any spacing, line height or indent of its own too: a plain cell has only the
            // table's look, and Word cells nearly always set some)
            let own_look = cell.paras.iter().any(|q| q.x.space_before.unwrap_or(0.0) > 0.0 || q.x.space_after.unwrap_or(0.0) > 0.0 || q.x.line.is_some() || q.x.left.unwrap_or(0.0) != 0.0 || q.x.first.unwrap_or(0.0) != 0.0);
            let mixed = own_look || fmts.windows(2).any(|w| w[0] != w[1]) || cell.paras.windows(2).any(|w| para_look(&w[0]) != para_look(&w[1]));
            let mut rich = false;
            if mixed && !text.trim().is_empty() {
                let (rt, starts) = storage_text(&cell.paras);
                if let Ok(sid) = p.doc.set_cell_rich(h, id, r, c, &rt) {
                    format_storage(p, h, sid, &cell.paras, &starts, false);
                    rich = true;
                }
            }
            if !text.is_empty() && !rich {
                let _ = p.doc.set_cell(h, id, r, c, &CellInput::Text(text.clone()));
            }
            // an empty cell: its paragraph mark's font and size and its paragraph's spacing on the
            // cell's style (its line height, and what a save writes back)
            if text.trim().is_empty() {
                if let Some(q) = cell.paras.first() {
                    let range = (r, c, r, c);
                    if let Some(m) = q.x.mark.as_ref().or(q.x.mark_fmt.as_ref()) {
                        if let Some(f) = &m.font {
                            let _ = p.doc.set_cells_text_prop(h, id, range, &kn_model::format::TextProp::Font(crate::import::ps_name(f)));
                        }
                        if let Some(s) = m.size {
                            let _ = p.doc.set_cells_text_prop(h, id, range, &kn_model::format::TextProp::Size(s));
                        }
                    }
                    if let Some(("kRelativeLineSpacing", k)) = q.x.line {
                        let _ = p.doc.set_cells_text_prop(h, id, range, &kn_model::format::TextProp::LineSpacing(k));
                    }
                    let _ = p.doc.set_cells_text_prop(h, id, range, &kn_model::format::TextProp::SpaceBefore(q.x.space_before.unwrap_or(0.0)));
                    let _ = p.doc.set_cells_text_prop(h, id, range, &kn_model::format::TextProp::SpaceAfter(q.x.space_after.unwrap_or(0.0)));
                }
            }
            if let Some(f) = cell.fill {
                let _ = p.doc.set_cells_fill(h, id, (r, c, r, c), Some(&kn_model::format::FillSpec::Color(f)));
            }
            // its own margins; Word lays a row out with its borders' width too (the top one, and
            // the bottom one under the last row)
            let bw = |b: &Option<Option<(Color, f32)>>| b.flatten().map(|x| x.1).unwrap_or(if t.borders && b.is_none() { t.border_line.map(|l| l.1).unwrap_or(0.5) } else { 0.0 });
            let (top_w, bottom_w) = (bw(&cell.borders[0]), if ri + 1 == t.rows.len() { bw(&cell.borders[2]) } else { 0.0 });
            if cell.margins.iter().any(|m| m.is_some()) || top_w > 0.0 || bottom_w > 0.0 {
                let mut m: [f32; 4] = std::array::from_fn(|k| cell.margins[k].unwrap_or(t.margins[k]));
                m[1] += top_w;
                m[3] += bottom_w;
                let _ = p.doc.set_cells_padding(h, id, (r, c, r, (c + cell.span).min(c0) - 1), m);
            }
            // the cell's own borders (over the table's); on an edge it shares with a neighbour,
            // a border either cell draws wins over the other's "none" (Word)
            let c1 = (c + cell.span).min(c0) - 1;
            let neighbour = |rr: Option<usize>, cc: u32, side: usize| -> bool {
                let Some(row) = rr.and_then(|rr| t.rows.get(rr)) else { return false };
                let mut x = 0u32;
                row.2.iter().find(|n| {
                    let hit = x <= cc && cc < x + n.span;
                    x += n.span;
                    hit
                }).is_some_and(|n| matches!(n.borders[side], Some(Some(_))))
            };
            for (i, b) in cell.borders.iter().enumerate() {
                let Some(b) = b else { continue };
                if b.is_none() {
                    let shown = match i {
                        0 => neighbour(ri.checked_sub(1), c, 2),
                        2 => neighbour(Some(ri + 1), c, 0),
                        1 => c > 0 && neighbour(Some(ri), c - 1, 3),
                        _ => neighbour(Some(ri), c1 + 1, 1),
                    };
                    if shown {
                        continue;
                    }
                }
                let edge = [kn_model::tablefmt::BorderEdges::Top, kn_model::tablefmt::BorderEdges::Left, kn_model::tablefmt::BorderEdges::Bottom, kn_model::tablefmt::BorderEdges::Right][i];
                let stroke = b.map(|(col, w)| kn_model::format::StrokeSpec { color: col, width: w, dash: kn_model::format::Dash::Solid });
                let _ = p.doc.set_cells_border(h, id, (r, c, r, c1), edge, stroke.as_ref());
            }
            let runs: Vec<&RunFmt> = cell.paras.iter().flat_map(|p| p.p.runs.iter().filter(|r| !r.0.trim().is_empty()).map(|r| &r.1)).collect();
            if !runs.is_empty() && !rich {
                if runs.iter().all(|f| f.bold) {
                    let _ = p.doc.set_cells_text_prop(h, id, (r, c, r, c), &kn_model::format::TextProp::Bold(true));
                }
                if let Some(s) = runs[0].size {
                    let _ = p.doc.set_cells_text_prop(h, id, (r, c, r, c), &kn_model::format::TextProp::Size(s));
                }
                if let Some(col) = runs[0].color {
                    let _ = p.doc.set_cells_text_prop(h, id, (r, c, r, c), &kn_model::format::TextProp::Color(col));
                }
                if runs.iter().all(|f| f.caps == runs[0].caps) && runs[0].caps != 0 {
                    let caps = if runs[0].caps == 1 { kn_model::Caps::All } else { kn_model::Caps::Small };
                    let _ = p.doc.set_cells_text_prop(h, id, (r, c, r, c), &kn_model::format::TextProp::Caps(caps));
                }
                if let Some(fnt) = &runs[0].font {
                    if runs.iter().all(|f| f.font.as_ref() == Some(fnt)) {
                        let _ = p.doc.set_cells_text_prop(h, id, (r, c, r, c), &kn_model::format::TextProp::Font(crate::import::ps_name(fnt)));
                    }
                }
            }
            if let Some(a) = cell.paras.first().and_then(|p| p.p.align) {
                let ca = match a {
                    Align::Center => kn_model::tablecells::CellAlign::Center,
                    Align::Right => kn_model::tablecells::CellAlign::Right,
                    Align::Justify => kn_model::tablecells::CellAlign::Justify,
                    _ => kn_model::tablecells::CellAlign::Left,
                };
                let _ = p.doc.set_cells_alignment(h, id, (r, c, r, c), ca);
            }
            col += cell.span;
        }
    }
    for m in merges {
        if m.rows > 1 || m.cols > 1 {
            let _ = p.doc.merge_cells(h, id, m);
        }
    }
    let header_rows = t.rows.iter().take_while(|r| r.1).count() as u32;
    if header_rows > 0 {
        let _ = p.doc.set_table_header_count(h, id, kn_model::tablerows::HeaderKind::Rows, header_rows);
    }
}

/// Build the Word document into a new Pages document.
pub fn build(w: &WordDoc) -> Pages {
    let mut p = Pages::new_blank(false);
    // laid out as Word lays it out (anchors are placed from that layout too)
    p.word_layout = true;
    let mut h = History::default();
    p.doc.begin(&mut h, "Import");
    // page setup
    let mut setup = p.setup();
    // (the document's setup is its first section's; later sections keep their own)
    if let Some(s) = w.section_setups.first().or(w.setup.as_ref()) {
        setup.width = s.width;
        setup.height = s.height;
        setup.landscape = s.landscape;
        setup.top = s.top;
        setup.bottom = s.bottom;
        setup.left = s.left;
        setup.right = s.right;
        setup.header_margin = s.header;
        setup.footer_margin = s.footer;
    }
    // headers and footers stay on (Pages' default): Word has no switch, only their text
    setup.headers = true;
    setup.footers = true;
    p.set_setup(&mut h, &setup);
    if let Some(s) = w.setup.as_ref().filter(|s| s.columns > 1) {
        let tw = setup.text_width().max(1.0);
        p.set_columns(&mut h, s.columns, s.column_gap / tw);
    }
    // body text and its formatting
    let Some(body) = p.body_storage() else {
        p.doc.end(&mut h);
        return p;
    };
    word_neutral_styles(&mut p, &mut h);
    // Word's default tab interval on the styles the paragraphs use
    for name in ["Body", "Title", "Subtitle", "Heading", "Heading 2", "Heading 3", "Caption", "Quote"] {
        if let Some(id) = crate::edit::para_style_named(&p.doc, name) {
            p.doc.touch(&mut h, id);
            if let Some(m) = p.doc.file.obj_mut(id) {
                m.msg_mut_or_create("para_properties").set_f32("default_tab_stops", if w.default_tab > 0.0 { w.default_tab } else { 36.0 });
            }
        }
    }
    let (paras, tables) = body_paras(&w.blocks);
    let (text, starts) = storage_text(&paras);
    let old = p.doc.storage_text(body);
    if !old.is_empty() {
        crate::edit::replace(&mut p.doc, &mut h, body, 0, old.len(), "");
    }
    crate::edit::replace(&mut p.doc, &mut h, body, 0, 0, &text);
    format_storage(&mut p, &mut h, body, &paras, &starts, true);
    // comments, footnotes, fields, inline pictures and tables: from the end, so offsets hold
    for (i, para) in paras.iter().enumerate().rev() {
        let at = starts[i];
        for (a, b, t, who) in para.x.comments.iter().rev() {
            if b > a {
                p.add_text_comment(&mut h, body, at + a, at + b, t, who);
            }
        }
    }
    write_fields(&mut p, &mut h, body, &paras, &starts);
    let width = setup.text_width();
    // inserts, latest offset first: (offset, what)
    enum Ins<'a> {
        Image(&'a (Vec<u8>, String), Option<(f32, f32)>),
        Table(&'a Table, usize),
        Note(&'a str),
        Shape(&'a Anchor),
    }
    let mut ins: Vec<(usize, u8, Ins)> = Vec::new();
    // (Word's fonts, to measure table text by, made when a table needs them)
    let mut fonts: Option<kn_render::fonts::Fonts> = None;
    for (i, para) in paras.iter().enumerate() {
        // the fields' stand-ins became one character each: offsets stay
        for (k, img) in para.p.images.iter().enumerate() {
            let at = starts[i] + para.x.image_at.get(k).copied().unwrap_or_else(|| para.p.runs.iter().map(|r| r.0.len()).sum());
            ins.push((at, 1, Ins::Image(img, para.x.image_size.get(k).copied().flatten())));
        }
        for (at, t) in &para.x.footnotes {
            ins.push((starts[i] + at, 0, Ins::Note(t)));
        }
        for (at, a) in &para.x.inline_shapes {
            ins.push((starts[i] + at, 1, Ins::Shape(a)));
        }
    }
    for (pi, t) in &tables {
        ins.push((starts[*pi], 2, Ins::Table(t, *pi)));
    }
    ins.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    for (at, _, what) in ins {
        match what {
            Ins::Image((bytes, name), size) => {
                let id = p.insert_image(&mut h, bytes.clone(), name, crate::objects::Placement::Inline { at }, width);
                // the size Word shows it at (not the picture's own pixel size)
                if let (Ok(id), Some((w, ht))) = (id, size) {
                    let mut g = p.file().obj(id).and_then(|o| o.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
                    let k = (width / w).min(1.0);
                    g.w = w * k;
                    g.h = ht * k;
                    p.doc.set_frame(&mut h, id, g);
                }
            }
            Ins::Table(t, pi) => {
                // an autofit table wider than its room (its section's text width): its columns
                // shrink alike to fit
                let total: f32 = t.col_widths.iter().sum();
                let sec = paras[..pi.min(paras.len())].iter().filter(|q| q.x.section_end).count();
                let room = w.section_setups.iter().chain(w.setup.as_ref()).nth(sec).filter(|_| !w.section_setups.is_empty()).map(|s| s.width - s.left - s.right).unwrap_or(width) + t.overhang;
                let fitted;
                let t = if t.autofit && total > room + 0.01 {
                    let mut f = t.clone();
                    f.col_widths = autofit_widths(t, room, fonts.get_or_insert_with(word_fonts));
                    fitted = f;
                    &fitted
                } else {
                    t
                };
                if let Ok(id) = p.insert_table(&mut h, crate::objects::Placement::Inline { at }) {
                    fill_table(&mut p, &mut h, id, t, fonts.get_or_insert_with(word_fonts));
                    if t.keep_together {
                        p.keep_tables.insert(id);
                    }
                    if t.exact_rows.iter().any(|e| *e) {
                        p.exact_rows.insert(id, t.exact_rows.clone());
                    }
                }
            }
            Ins::Shape(a) => {
                if let Some(id) = insert_anchor(&mut p, &mut h, a, 0, 0.0, 0.0) {
                    p.set_placement(&mut h, id, crate::objects::Placement::Inline { at });
                }
            }
            Ins::Note(t) => {
                if let Ok(note) = p.insert_footnote(&mut h, at) {
                    let st = p.doc.storage_of(note).unwrap_or(note);
                    let old = p.doc.storage_text(st);
                    crate::edit::replace(&mut p.doc, &mut h, st, 0, old.len(), t);
                }
            }
        }
    }
    // headers and footers (a different first page when Word has one)
    let secs = p.sections();
    if let Some(s) = secs.first() {
        let (first, odd, even) = (s.first, s.odd, s.even);
        let sid = s.id;
        for (footer, main, first_hf, even_hf) in [(false, &w.header, &w.first_header, &w.even_header), (true, &w.footer, &w.first_footer, &w.even_footer)] {
            // (even pages: their own when Word has them apart, else the main one)
            if let (true, Some(m), Some(hf)) = (w.even_odd, even, even_hf) {
                write_header_footer(&mut p, &mut h, m, footer, hf);
            }
            if let Some(hf) = main {
                for m in [odd, even.filter(|_| !w.even_odd)].into_iter().flatten() {
                    write_header_footer(&mut p, &mut h, m, footer, hf);
                }
                if !w.title_page {
                    if let Some(m) = first {
                        write_header_footer(&mut p, &mut h, m, footer, hf);
                    }
                }
            }
            if w.title_page {
                if let (Some(m), Some(hf)) = (first, first_hf) {
                    write_header_footer(&mut p, &mut h, m, footer, hf);
                }
            }
        }
        if w.title_page {
            p.set_section(&mut h, sid, |m| m.set_bool("section_template_first_page_different", true));
        }
        if w.even_odd {
            p.set_section(&mut h, sid, |m| m.set_bool("section_template_even_odd_pages_different", true));
        }
        // the page colour
        if let Some(c) = w.background {
            p.set_section_fill(&mut h, sid, &kn_model::format::FillSpec::Color(c));
        }
    }
    word_sections(&mut p, &mut h, w, &paras);
    p.doc.end(&mut h);
    place_anchors(&mut p, w, &paras, &setup);
    p.doc.dirty = true;
    p
}

/// A Word shape as a Pages one: a line, a built-in shape for Word's presets, else its exact
/// outline; fill, border, rotation and flips.
fn insert_shape(p: &mut Pages, h: &mut History, s: &ShapeIn, place: crate::objects::Placement, a: &Anchor) -> Option<kn_format::ObjId> {
    use kn_model::insert::BasicShape;
    let crate::objects::Placement::Floating { x, y, .. } = place else { return None };
    if s.line {
        let id = p.insert(h, crate::objects::NewObject::Line, place).ok()?;
        let (x0, x1) = if s.flip_h { (x + a.w, x) } else { (x, x + a.w) };
        let (y0, y1) = if s.flip_v { (y + a.h, y) } else { (y, y + a.h) };
        p.doc.set_line_points(h, id, kn_model::Point { x: x0, y: y0 }, kn_model::Point { x: x1, y: y1 });
        let (c, wpt) = s.stroke.flatten().unwrap_or((Color::BLACK, 1.0));
        p.doc.set_stroke(h, id, Some(&kn_model::format::StrokeSpec { color: c, width: wpt, dash: kn_model::format::Dash::Solid }));
        return Some(id);
    }
    let basic = match s.preset.as_deref() {
        Some("roundRect") => BasicShape::RoundedRectangle,
        Some("ellipse") => BasicShape::Oval,
        Some("triangle") => BasicShape::Triangle,
        Some("rtTriangle") => BasicShape::RightTriangle,
        Some("diamond") => BasicShape::Diamond,
        Some("rightArrow") => BasicShape::Arrow,
        Some("leftRightArrow") => BasicShape::DoubleArrow,
        Some("star5") => BasicShape::Star,
        Some("pentagon") | Some("homePlate") => BasicShape::Pentagon,
        Some("wedgeRectCallout") | Some("wedgeRoundRectCallout") => BasicShape::Callout,
        Some("wedgeEllipseCallout") | Some("cloudCallout") => BasicShape::QuoteBubble,
        _ => BasicShape::Rectangle,
    };
    let id = p.insert(h, crate::objects::NewObject::Shape(basic), place).ok()?;
    let mut g = p.file().obj(id).and_then(|o| o.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
    g.x = x;
    g.y = y;
    g.w = a.w.max(1.0);
    g.h = a.h.max(1.0);
    g.angle = s.rot;
    g.hflip = s.flip_h;
    g.vflip = s.flip_v;
    p.doc.set_frame(h, id, g);
    if s.preset.is_none() {
        if let Some(path) = &s.path {
            p.doc.make_editable(h, id, path);
        }
    }
    let fill = match s.fill {
        Some(Some(c)) => kn_model::format::FillSpec::Color(c),
        Some(None) => kn_model::format::FillSpec::None,
        None => kn_model::format::FillSpec::None,
    };
    p.doc.set_fill(h, id, &fill);
    let stroke = s.stroke.flatten().map(|(c, wpt)| kn_model::format::StrokeSpec { color: c, width: wpt, dash: kn_model::format::Dash::Solid });
    p.doc.set_stroke(h, id, stroke.as_ref());
    Some(id)
}

/// Where Word puts an anchored object on the page; `py` is its paragraph's top.
fn anchor_xy(a: &Anchor, setup: &crate::PageSetup, py: f32) -> (f32, f32) {
    let tw = setup.width - setup.left - setup.right;
    let th = setup.height - setup.top - setup.bottom;
    let x = match (a.x_from.as_str(), a.x_align.as_deref()) {
        ("page", Some("center")) => (setup.width - a.w) / 2.0,
        ("page", Some("right")) => setup.width - a.w,
        ("page", _) => a.x,
        (_, Some("center")) => setup.left + (tw - a.w) / 2.0,
        (_, Some("right")) => setup.left + tw - a.w,
        _ => setup.left + a.x,
    };
    let y = match (a.y_from.as_str(), a.y_align.as_deref()) {
        ("page", Some("center")) => (setup.height - a.h) / 2.0,
        ("page", Some("bottom")) => setup.height - a.h,
        ("page", _) => a.y,
        ("margin", Some("center")) => setup.top + (th - a.h) / 2.0,
        ("margin", Some("bottom")) => setup.top + th - a.h,
        ("margin", Some("top")) => setup.top,
        ("margin", _) | ("topMargin", _) => setup.top + a.y,
        _ => py + a.y,
    };
    (x.max(-a.w), y.max(-a.h))
}

/// An anchored object made in the document (floating on `page`); its id.
fn insert_anchor(p: &mut Pages, h: &mut History, a: &Anchor, page: usize, x: f32, y: f32) -> Option<kn_format::ObjId> {
    let place = crate::objects::Placement::Floating { page, x, y };
    let id = match &a.kind {
        AnchorKind::Shape(s) => insert_shape(p, h, s, place, a),
        AnchorKind::Picture(bytes, name) => p.insert_image(h, bytes.clone(), name, place, a.w.max(1.0)).ok(),
        AnchorKind::Chart(ci) => p.insert(h, crate::objects::NewObject::Chart, place).ok().inspect(|id| {
            let id = *id;
            let _ = p.doc.set_chart_type(h, id, &ci.ty);
            // a pie plots its series by column: the categories are its rows
            let round = ci.ty.starts_with("pie") || ci.ty.starts_with("donut");
            let grid = if round {
                kn_model::chartdata::ChartGrid { row_names: ci.categories.clone(), col_names: ci.series.clone(), values: (0..ci.categories.len()).map(|r| ci.values.iter().map(|s| s.get(r).copied().unwrap_or(f64::NAN)).collect()).collect() }
            } else {
                kn_model::chartdata::ChartGrid { row_names: ci.series.clone(), col_names: ci.categories.clone(), values: ci.values.clone() }
            };
            let _ = p.doc.set_chart_grid(h, id, &grid);
            let _ = p.doc.set_chart_title(h, id, ci.title.as_deref());
            let _ = p.doc.set_chart_legend(h, id, ci.legend);
            if !round {
                for (i, c) in ci.colors.iter().enumerate() {
                    if let Some(c) = c {
                        let _ = p.doc.set_series_fill(h, id, i, &kn_model::format::FillSpec::Color(*c));
                    }
                }
            }
        }),
        AnchorKind::TextBox(tps) => p.insert(h, crate::objects::NewObject::TextBox, place).ok().inspect(|id| {
            let mut g = p.file().obj(*id).and_then(|o| o.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
            g.w = a.w.max(20.0);
            g.h = a.h.max(14.0);
            p.doc.set_frame(h, *id, g);
            if let Some(st) = p.doc.storage_of(*id) {
                let ps: Vec<Para> = tps.iter().map(|q| Para { p: q.clone(), x: ParaExtra::default() }).collect();
                let (t, starts) = storage_text(&ps);
                let n = p.doc.storage_text(st).len();
                crate::edit::replace(&mut p.doc, h, st, 0, n, &t);
                format_storage(p, h, st, &ps, &starts, false);
            }
        }),
    }?;
    if matches!(a.kind, AnchorKind::Picture(..) | AnchorKind::Chart(..)) {
        let mut g = p.file().obj(id).and_then(|o| o.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap_or_default();
        g.w = a.w;
        g.h = a.h;
        g.x = x;
        g.y = y;
        p.doc.set_frame(h, id, g);
    }
    p.set_wrap(h, id, a.wrap);
    if a.behind {
        p.reorder(h, &[id], kn_model::edit::ZMove::Back);
    }
    Some(id)
}

/// Header / footer objects onto the section template they belong to.
fn place_header_anchors(p: &mut Pages, h: &mut History, master: kn_format::ObjId, hf: &HeaderFooter, setup: &crate::PageSetup, footer: bool) {
    let py = if footer { setup.height - setup.footer_margin } else { setup.header_margin };
    let top = if footer { setup.height - setup.footer_margin - hf.height } else { setup.header_margin };
    let mut ids = Vec::new();
    for a in &hf.anchors {
        // a paragraph rule: across the text, from the block's top
        let mut a = a.clone();
        let rule = a.y_from == "hf-top";
        if rule {
            a.w = setup.width - setup.left - setup.right;
        }
        let a = &a;
        let (x, y) = anchor_xy(a, setup, if rule { top } else { py });
        if let (AnchorKind::Picture(bytes, name), true) = (&a.kind, a.behind && x.abs() < 2.0 && y.abs() < 2.0 && (a.w - setup.width).abs() < 3.0 && (a.h - setup.height).abs() < 3.0) {
            let sec = p.sections().first().map(|s| s.id);
            if let (Some(sec), Ok(data)) = (sec, p.doc.file.add_image_data(bytes.clone(), name)) {
                p.set_section_fill(h, sec, &kn_model::format::FillSpec::Image { data_id: data, technique: kn_model::ImageFillTechnique::Stretch, size: Some((a.w, a.h)), reference: None, tint: None });
                continue;
            }
        }
        if let Some(id) = insert_anchor(p, h, a, 0, x, y) {
            ids.push(id);
        }
    }
    if !ids.is_empty() {
        p.move_to_section_layout(h, &ids, master);
    }
}

/// Floating pictures and text boxes: laid out once to find each anchor paragraph's page, then
/// put where Word positions them.
fn place_anchors(p: &mut Pages, w: &WordDoc, paras: &[Para], setup: &crate::PageSetup) {
    if !paras.iter().any(|q| !q.x.anchors.is_empty()) {
        return;
    }
    let _ = w;
    let mut fonts = kn_render::fonts::Fonts::new();
    let mut cache = crate::flow::Cache::default();
    let l = crate::flow::layout(p, &mut fonts, &mut cache, &Default::default());
    let mut h = History::default();
    p.doc.begin(&mut h, "Import");
    for (i, para) in paras.iter().enumerate() {
        if para.x.anchors.is_empty() {
            continue;
        }
        // the page and line the paragraph starts on
        let (page, py) = l
            .pages
            .iter()
            .find_map(|pg| pg.lines.iter().find(|ln| ln.para >= i).map(|ln| (pg.index, ln.dy)))
            .unwrap_or((l.pages.len().saturating_sub(1), setup.top));
        // the paragraph's table: the inline object just above its text
        let table = l.pages.get(page).and_then(|pg| pg.objects.iter().filter(|o| o.inline && o.y <= py + 0.5).max_by(|a, b| a.y.total_cmp(&b.y))).map(|o| (o.x, o.y));
        for a in &para.x.anchors {
            let (x, y) = match (a.y_from.as_str(), table) {
                ("table", Some((tx, ty))) => (tx + a.x, ty + a.y),
                _ => anchor_xy(a, setup, py),
            };
            insert_anchor(p, &mut h, a, page, x, y);
        }
    }
    p.doc.end(&mut h);
}

