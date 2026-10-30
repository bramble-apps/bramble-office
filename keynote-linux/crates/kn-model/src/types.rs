//! Render-ready, fully resolved slide content (styles already looked up through their chains).

use kn_format::ObjId;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }

    pub fn with_alpha(self, a: f32) -> Color {
        Color { a: self.a * a, ..self }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

/// Position + size of the unrotated frame (points, slide coordinates, y down);
/// rotation is about the frame centre, `angle` in degrees counter-clockwise (Keynote's convention).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Geometry {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub angle: f32,
    pub hflip: bool,
    pub vflip: bool,
    /// Width follows the text (geometry flag bit 1 clear); `x` is then an anchor set by alignment.
    pub auto_width: bool,
    /// Height follows the text (flag bit 2 clear); `y` is an anchor set by vertical alignment.
    pub auto_height: bool,
}

impl Geometry {
    pub fn center(&self) -> Point {
        Point::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Map a point in the frame's local space (0..w, 0..h) to slide space.
    pub fn to_slide(&self, p: Point) -> Point {
        let (cx, cy) = (self.w / 2.0, self.h / 2.0);
        let mut lx = p.x - cx;
        let mut ly = p.y - cy;
        if self.hflip {
            lx = -lx;
        }
        if self.vflip {
            ly = -ly;
        }
        let a = -self.angle.to_radians();
        let (s, c) = a.sin_cos();
        let c0 = self.center();
        Point::new(c0.x + lx * c - ly * s, c0.y + lx * s + ly * c)
    }

    /// Inverse of [`to_slide`].
    pub fn to_local(&self, p: Point) -> Point {
        let c0 = self.center();
        let (dx, dy) = (p.x - c0.x, p.y - c0.y);
        let a = self.angle.to_radians();
        let (s, c) = a.sin_cos();
        let mut lx = dx * c - dy * s;
        let mut ly = dx * s + dy * c;
        if self.hflip {
            lx = -lx;
        }
        if self.vflip {
            ly = -ly;
        }
        Point::new(lx + self.w / 2.0, ly + self.h / 2.0)
    }

    /// Axis-aligned bounds of the rotated frame in slide space.
    pub fn bounds(&self) -> (Point, Point) {
        let pts = [
            self.to_slide(Point::new(0.0, 0.0)),
            self.to_slide(Point::new(self.w, 0.0)),
            self.to_slide(Point::new(0.0, self.h)),
            self.to_slide(Point::new(self.w, self.h)),
        ];
        let minx = pts.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let miny = pts.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let maxx = pts.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
        let maxy = pts.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
        (Point::new(minx, miny), Point::new(maxx, maxy))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathEl {
    Move(Point),
    Line(Point),
    Quad(Point, Point),
    Cubic(Point, Point, Point),
    Close,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Path {
    pub els: Vec<PathEl>,
}

impl Path {
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.els.push(PathEl::Move(Point::new(x, y)));
    }
    pub fn line_to(&mut self, x: f32, y: f32) {
        self.els.push(PathEl::Line(Point::new(x, y)));
    }
    pub fn cubic_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.els.push(PathEl::Cubic(Point::new(x1, y1), Point::new(x2, y2), Point::new(x, y)));
    }
    pub fn close(&mut self) {
        self.els.push(PathEl::Close);
    }

    pub fn map(&self, f: impl Fn(Point) -> Point) -> Path {
        Path {
            els: self
                .els
                .iter()
                .map(|e| match *e {
                    PathEl::Move(p) => PathEl::Move(f(p)),
                    PathEl::Line(p) => PathEl::Line(f(p)),
                    PathEl::Quad(a, p) => PathEl::Quad(f(a), f(p)),
                    PathEl::Cubic(a, b, p) => PathEl::Cubic(f(a), f(b), f(p)),
                    PathEl::Close => PathEl::Close,
                })
                .collect(),
        }
    }

    pub fn scale(&self, sx: f32, sy: f32) -> Path {
        self.map(|p| Point::new(p.x * sx, p.y * sy))
    }

    /// Bounding box of all points (control points included).
    pub fn bounds(&self) -> Option<(Point, Point)> {
        let mut it = self.els.iter().flat_map(|e| match *e {
            PathEl::Move(p) | PathEl::Line(p) => vec![p],
            PathEl::Quad(a, p) => vec![a, p],
            PathEl::Cubic(a, b, p) => vec![a, b, p],
            PathEl::Close => vec![],
        });
        let first = it.next()?;
        let (mut lo, mut hi) = (first, first);
        for p in it {
            lo.x = lo.x.min(p.x);
            lo.y = lo.y.min(p.y);
            hi.x = hi.x.max(p.x);
            hi.y = hi.y.max(p.y);
        }
        Some((lo, hi))
    }

    /// Bounding box of the curve itself (curves sampled; control points excluded).
    pub fn curve_bounds(&self) -> Option<(Point, Point)> {
        let mut pts = Vec::new();
        let mut cur = Point::default();
        for e in &self.els {
            match *e {
                PathEl::Move(p) | PathEl::Line(p) => {
                    pts.push(p);
                    cur = p;
                }
                PathEl::Quad(a, p) => {
                    for i in 1..=16 {
                        let t = i as f32 / 16.0;
                        let u = 1.0 - t;
                        pts.push(Point::new(
                            u * u * cur.x + 2.0 * u * t * a.x + t * t * p.x,
                            u * u * cur.y + 2.0 * u * t * a.y + t * t * p.y,
                        ));
                    }
                    cur = p;
                }
                PathEl::Cubic(a, b, p) => {
                    for i in 1..=24 {
                        let t = i as f32 / 24.0;
                        let u = 1.0 - t;
                        let (c0, c1, c2, c3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                        pts.push(Point::new(
                            c0 * cur.x + c1 * a.x + c2 * b.x + c3 * p.x,
                            c0 * cur.y + c1 * a.y + c2 * b.y + c3 * p.y,
                        ));
                    }
                    cur = p;
                }
                PathEl::Close => {}
            }
        }
        let first = *pts.first()?;
        let (mut lo, mut hi) = (first, first);
        for p in pts {
            lo.x = lo.x.min(p.x);
            lo.y = lo.y.min(p.y);
            hi.x = hi.x.max(p.x);
            hi.y = hi.y.max(p.y);
        }
        Some((lo, hi))
    }

    pub fn is_closed(&self) -> bool {
        self.els.iter().any(|e| matches!(e, PathEl::Close))
    }

    pub fn rect(w: f32, h: f32) -> Path {
        let mut p = Path::default();
        p.move_to(0.0, 0.0);
        p.line_to(w, 0.0);
        p.line_to(w, h);
        p.line_to(0.0, h);
        p.close();
        p
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GradientKind {
    /// Angle in degrees (0 = left to right, counter-clockwise).
    Linear { angle: f32 },
    Radial,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Fill {
    None,
    Color(Color),
    /// `inflections`: each stop's midpoint to the next (0.5 = halfway); `advanced`: Keynote's
    /// Advanced Gradient Fill (any number of stops, linear or radial).
    Gradient { kind: GradientKind, stops: Vec<(f32, Color)>, opacity: f32, inflections: Vec<f32>, advanced: bool },
    /// `reference`: the colour Keynote stores with an image fill, drawn when the image can't be
    /// found (theme artwork that lives in Keynote's app bundle, not in the file).
    /// `size`: the fill size Keynote stores (its Scale, relative to the picture's own size).
    Image { data_id: u64, technique: ImageFillTechnique, tint: Option<Color>, reference: Option<Color>, size: Option<(f32, f32)> },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageFillTechnique {
    Natural,
    Stretch,
    Tile,
    ScaleToFill,
    ScaleToFit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f32,
    /// Dash lengths in multiples of the stroke width (empty = solid).
    pub dash: Vec<f32>,
    pub phase: f32,
    /// Keynote's artistic strokes (smart_stroke.stroke_name, e.g. "Dry Brush"): painted with a
    /// brush texture, so the mark is thinner and rougher than `width`.
    pub brush: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shadow {
    pub color: Color,
    /// Degrees; light source angle as in Keynote (315 = down-right shadow).
    pub angle: f32,
    pub offset: f32,
    pub blur: f32,
    pub opacity: f32,
    /// 0 drop, 1 contact, 2 curved (TSD.ShadowArchive.ShadowType).
    pub kind: u8,
    /// Contact shadow: how tall the flattened shape is (fraction of the object's height).
    pub height: f32,
    /// Curved shadow: how much it bends (0..1).
    pub curve: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineEnd {
    /// Arrowhead path in its own coordinates; the tip is `end_point`.
    pub path: Path,
    pub end_point: Point,
    pub filled: bool,
    pub id: String,
}

// ---------------------------------------------------------------------------------- text

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Right,
    Center,
    Justify,
    Natural,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineSpacing {
    /// Multiple of the natural line height.
    Relative(f32),
    AtLeast(f32),
    Exactly(f32),
    AtMost(f32),
    /// Extra points between lines.
    Between(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Caps {
    None,
    All,
    Small,
    /// Title Case: words capitalised except short ones (a, of, the…).
    Title,
    /// Start Case: every word capitalised (stored as kTitled without linguistics).
    Start,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CharStyle {
    /// PostScript font name as stored by Keynote, e.g. "HelveticaNeue-Medium".
    pub font_name: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: Color,
    pub underline: bool,
    pub strike: bool,
    /// +1 superscript, -1 subscript.
    pub script: i8,
    pub baseline_shift: f32,
    pub caps: Caps,
    /// Tracking in 1/1000 em? stored as fraction of the font size by Keynote.
    pub tracking: f32,
    pub background: Option<Color>,
    pub outline: Option<(Color, f32)>,
    pub shadow: Option<Shadow>,
}

impl Default for CharStyle {
    fn default() -> Self {
        CharStyle {
            font_name: "HelveticaNeue".into(),
            size: 24.0,
            bold: false,
            italic: false,
            color: Color::BLACK,
            underline: false,
            strike: false,
            script: 0,
            baseline_shift: 0.0,
            caps: Caps::None,
            tracking: 0.0,
            background: None,
            outline: None,
            shadow: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// Byte range in [`TextBox::text`].
    pub start: usize,
    pub end: usize,
    pub style: CharStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ListLabel {
    Bullet(String),
    Number(String),
    Image(u64),
}

/// A tab stop: where the text after a tab lines up.
#[derive(Debug, Clone, PartialEq)]
pub struct TabStop {
    pub pos: f32,
    pub align: TabAlign,
    /// The character filling the gap ("." dot leader), if any.
    pub leader: Option<char>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TabAlign {
    Left,
    Center,
    Right,
    Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    /// Byte range in [`TextBox::text`] (without the trailing paragraph separator).
    pub start: usize,
    pub end: usize,
    pub runs: Vec<Run>,
    pub align: Align,
    pub line_spacing: LineSpacing,
    pub space_before: f32,
    pub space_after: f32,
    pub left_indent: f32,
    pub right_indent: f32,
    pub first_line_indent: f32,
    pub level: u32,
    pub label: Option<ListLabel>,
    /// Position of the list label for this level (points, from the left indent).
    pub list_indent: f32,
    /// Gap from the label position to the text, in ems of the paragraph's font size.
    pub label_indent: f32,
    pub label_color: Option<Color>,
    pub label_scale: f32,
    /// A labelled paragraph whose first line's text starts elsewhere than its other lines'
    /// (a Word list whose tab after the label goes to a tab stop, not the hanging indent):
    /// points from the label's position. None: where the other lines start.
    pub first_text_indent: Option<f32>,
    /// Tab stops (points from the text's left edge, in order) and the default stop interval.
    /// A default interval of 0 keeps the shaper's own tab width (Keynote's text so far).
    pub tabs: Vec<TabStop>,
    pub default_tab: f32,
    pub rtl: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextBox {
    pub storage: ObjId,
    pub text: String,
    pub paragraphs: Vec<Paragraph>,
    pub valign: VAlign,
    /// left, top, right, bottom
    pub padding: [f32; 4],
    pub shrink_to_fit: bool,
    pub columns: u32,
    pub column_gap: f32,
    /// Text box grows with its content (free text boxes).
    pub autogrow: bool,
    /// Review highlights (Insert > Highlight): byte range and the author's colour.
    pub highlights: Vec<(usize, usize, Color)>,
    /// Fields (slide number, date…): stored as one U+FFFC, shown as their value. Each is
    /// (shown byte offset, shown length, stored byte offset).
    pub fields: Vec<(usize, usize, usize)>,
}

impl TextBox {
    /// A byte offset in the shown text -> the stored text (inside a field: before it, or after
    /// it at its end).
    pub fn stored_offset(&self, shown: usize) -> usize {
        let mut delta: i64 = 0;
        for &(at, len, raw) in &self.fields {
            if shown < at {
                break;
            }
            if shown < at + len || (len == 0 && shown == at) {
                return raw;
            }
            if shown == at + len {
                return raw + 3;
            }
            delta = (raw + 3) as i64 - (at + len) as i64;
        }
        (shown as i64 + delta).max(0) as usize
    }
}

// ---------------------------------------------------------------------------------- drawables

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderKind {
    Title,
    Body,
    SlideNumber,
    Object,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Drawable {
    pub id: ObjId,
    pub geom: Geometry,
    pub opacity: f32,
    pub shadow: Option<Shadow>,
    pub reflection: Option<f32>,
    pub locked: bool,
    pub hyperlink: Option<String>,
    pub alt_text: Option<String>,
    pub placeholder: Option<PlaceholderKind>,
    /// Comes from the slide layout (master), not editable on the slide.
    pub from_template: bool,
    pub kind: DrawableKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DrawableKind {
    Shape(ShapeData),
    Image(ImageData),
    Movie(MovieData),
    Group(Vec<Drawable>),
    Table(TableData),
    Chart(ChartData),
    /// Anything we don't render yet (type name for diagnostics).
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeData {
    /// Outline in frame-local coordinates (0..w, 0..h).
    pub path: Path,
    pub fill: Fill,
    pub stroke: Option<Stroke>,
    pub head: Option<LineEnd>,
    pub tail: Option<LineEnd>,
    pub text: Option<TextBox>,
    pub is_line: bool,
    pub is_text_box: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageData {
    pub data_id: Option<u64>,
    /// Thumbnail data (used when the full image isn't in the package, e.g. theme photos).
    pub thumb_id: Option<u64>,
    /// Mask frame in the image frame's local coordinates, and its outline.
    pub mask: Option<(Geometry, Path)>,
    pub stroke: Option<Stroke>,
    pub natural: Size,
    /// Equation images keep their LaTeX/MathML source.
    pub equation: Option<String>,
    /// Insert > Image Gallery: the gallery's images (the image itself shows the first one).
    pub gallery: Vec<GalleryItem>,
    /// The gallery image shown now (its build steps and the editor's arrows change it).
    pub gallery_shown: usize,
    /// Captions: 0 none, 1 one per image, 2 the same for all (`gallery_caption`).
    pub gallery_caption_mode: u8,
    pub gallery_caption: Option<TextBox>,
    /// The caption text box drawn under the gallery (a drawable of the scene), if shown.
    pub gallery_caption_id: Option<ObjId>,
    /// Insert > Web Video: the video's page; the image is its poster.
    pub web_video: Option<WebVideo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GalleryItem {
    pub id: u64,
    pub data_id: Option<u64>,
    pub caption: String,
    /// The image's own caption text (styled), for "one caption per image".
    pub caption_box: Option<TextBox>,
    /// Pan and zoom inside the frame: the picture fills the frame, is scaled by `scale` and its
    /// centre moved by `offset` (slide points) from the frame's centre.
    pub offset: (f32, f32),
    pub scale: f32,
    /// Image Description (accessibility).
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WebVideo {
    pub url: String,
    pub title: String,
    pub author: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MovieData {
    pub movie_data: Option<u64>,
    pub poster_data: Option<u64>,
    pub audio_only: bool,
    pub volume: f32,
    pub loop_mode: u32,
    pub start: f32,
    pub end: f32,
    /// Plays when the slide appears (deprecated flag; modern decks start movies without a
    /// "movie-start" build automatically anyway).
    pub autoplay: bool,
    /// Keeps playing into the next slide when that slide shows the same movie.
    pub plays_across_slides: bool,
    pub poster_time: f32,
    /// Insert > Live Video: shows a camera (the poster is its stand-in picture).
    pub live: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableData {
    pub rows: u32,
    pub cols: u32,
    pub header_rows: u32,
    pub header_cols: u32,
    pub footer_rows: u32,
    pub row_heights: Vec<f32>,
    pub col_widths: Vec<f32>,
    pub name: String,
    /// Row-major cell text, already formatted with the cell's number/date/duration format.
    pub cells: Vec<String>,
    /// Resolved look of each cell (row-major, same indexing as `cells`; may be empty).
    pub cell_styles: Vec<TableCellStyle>,
    /// Rich text cells (row-major like `cells`; empty when the table has none): the cell's
    /// text with its own fonts, sizes and styles, laid out instead of `cells`' plain text.
    pub rich: Vec<Option<TextBox>>,
    /// Merged ranges; the top-left cell holds the content and style of the whole range.
    pub merges: Vec<CellRange>,
    /// Horizontal grid line segments: `(rows + 1) * cols`, index `boundary * cols + col`
    /// (boundary 0 is the top edge of row 0).
    pub h_strokes: Vec<Option<Stroke>>,
    /// Vertical grid line segments: `rows * (cols + 1)`, index `row * (cols + 1) + boundary`.
    pub v_strokes: Vec<Option<Stroke>>,
    /// Rows that grow to fit their text (at the table's default height; a row resized by hand
    /// keeps its size). Empty: none known.
    pub auto_rows: Vec<bool>,
    /// Part of a table continued over pages (Letters): the model row each shown row is
    /// (headers repeated, then this page's rows). Empty: the whole table, rows as they are.
    pub row_map: Vec<u32>,
}

impl TableData {
    /// The merged range covering (row, col), if any.
    pub fn merge_at(&self, row: u32, col: u32) -> Option<&CellRange> {
        self.merges.iter().find(|m| m.contains(row, col))
    }

    /// Total width of the columns (0 when the widths are unknown). Keynote lays a table out at
    /// its row/column sizes; the drawable's stored frame size can be stale.
    pub fn width(&self) -> f32 {
        (0..self.cols as usize).map(|c| self.col_widths.get(c).copied().unwrap_or(0.0)).sum()
    }

    /// Total height of the rows (0 when the heights are unknown).
    pub fn height(&self) -> f32 {
        (0..self.rows as usize).map(|r| self.row_heights.get(r).copied().unwrap_or(0.0)).sum()
    }

    /// The frame the table actually occupies: `g` at the table's row/column size (each axis
    /// keeps `g`'s size when the sizes are unknown).
    pub fn frame(&self, g: &Geometry) -> Geometry {
        let (w, h) = (self.width(), self.height());
        Geometry { w: if w > 0.0 { w } else { g.w }, h: if h > 0.0 { h } else { g.h }, ..*g }
    }
}

/// A rectangular block of cells.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CellRange {
    pub row: u32,
    pub col: u32,
    pub rows: u32,
    pub cols: u32,
}

impl CellRange {
    pub fn contains(&self, row: u32, col: u32) -> bool {
        row >= self.row && row < self.row + self.rows && col >= self.col && col < self.col + self.cols
    }
}

/// Fill, text style and layout of one table cell (table style + region style + cell overrides).
#[derive(Debug, Clone, PartialEq)]
pub struct TableCellStyle {
    pub fill: Fill,
    pub text: CharStyle,
    pub align: Align,
    pub valign: VAlign,
    /// Left, top, right, bottom.
    pub padding: [f32; 4],
    pub wrap: bool,
    /// Paragraph spacing and indents of the cell's text.
    pub para: CellPara,
}

/// A cell's paragraph formatting (Text inspector > Spacing, Layout > Indents).
#[derive(Debug, Clone, PartialEq)]
pub struct CellPara {
    pub line_spacing: LineSpacing,
    pub space_before: f32,
    pub space_after: f32,
    pub left_indent: f32,
    pub right_indent: f32,
    pub first_line_indent: f32,
}

impl Default for CellPara {
    fn default() -> Self {
        CellPara { line_spacing: LineSpacing::Relative(1.0), space_before: 0.0, space_after: 0.0, left_indent: 0.0, right_indent: 0.0, first_line_indent: 0.0 }
    }
}

impl Default for TableCellStyle {
    fn default() -> Self {
        TableCellStyle {
            fill: Fill::None,
            text: CharStyle::default(),
            align: Align::Left,
            valign: VAlign::Middle,
            padding: [4.0; 4],
            wrap: true,
            para: CellPara::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartData {
    pub chart_type: String,
    pub row_names: Vec<String>,
    pub col_names: Vec<String>,
    /// rows x cols values.
    pub values: Vec<Vec<f64>>,
    /// Series run along the grid's rows (Keynote's default) rather than its columns.
    pub series_by_row: bool,
    /// Category names (the grid's column names when series are rows).
    pub categories: Vec<String>,
    /// Plotted series in drawing order, with their resolved look.
    pub series: Vec<ChartSeries>,
    /// Y axis, then the second Y axis of 2-axis charts (also the X axis of scatter/bubble
    /// charts is `category_axis`).
    pub value_axes: Vec<ChartAxis>,
    pub category_axis: ChartAxis,
    pub show_legend: bool,
    /// Format > Chart > Show Borders (a frame around the chart).
    pub show_border: bool,
    /// Chart title (Format > Chart > Title), shown above the chart.
    pub title: Option<String>,
    pub title_style: CharStyle,
    /// Legend frame relative to the chart frame's origin (x, y, w, h).
    pub legend_frame: Option<[f32; 4]>,
    pub legend_style: CharStyle,
    /// Text style of value labels drawn inside marks (pie wedges) and outside them.
    pub value_label_style: CharStyle,
    pub outside_label_style: CharStyle,
    /// Donut hole radius as a fraction of the outer radius.
    pub inner_radius: f32,
    /// Interactive (multi-data) charts: the data set currently shown.
    pub data_set: Option<u32>,
    /// Gaps between bars of one set and between sets, as fractions of a bar width.
    pub bar_gap: f32,
    pub set_gap: f32,
    /// Format > Chart: background fill behind the plot (None: clear).
    pub background: Option<Fill>,
    /// The border's line when Show Border is on (None: Keynote's thin grey).
    pub border: Option<Stroke>,
    /// Rounded corners of bars and columns, a fraction of half their thickness; only the outer
    /// end when `corner_outer_only`.
    pub corner_radius: f32,
    pub corner_outer_only: bool,
}

impl ChartData {
    pub fn is_3d(&self) -> bool {
        self.chart_type.ends_with("3D")
    }
}

/// How one chart series is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SeriesKind {
    #[default]
    Column,
    Bar,
    Line,
    Area,
    Pie,
    Scatter,
    Bubble,
    Radar,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChartSeries {
    pub name: String,
    /// One value per category (Y values for scatter/bubble).
    pub values: Vec<f64>,
    /// X values (scatter/bubble only).
    pub xs: Vec<f64>,
    /// Bubble sizes (bubble only).
    pub sizes: Vec<f64>,
    pub kind: SeriesKind,
    pub fill: Fill,
    /// Representative colour of `fill` (or of the line stroke).
    pub color: Color,
    /// Line / outline stroke.
    pub stroke: Option<Stroke>,
    /// Plotted against the second value axis.
    pub y2: bool,
    pub show_value_labels: bool,
    /// Pie wedges and bubbles labelled with the series name (Format > Chart > Show Series Names).
    pub show_series_names: bool,
    pub show_symbols: bool,
    /// Value labels' number format (TSK.FormatStructArchive; None: automatic).
    pub value_format: Option<kn_format::Msg>,
    /// Pie wedge pulled out from the centre, as a fraction of the radius.
    pub explosion: f32,
    /// Pie labels' distance from the centre, percent of the radius (None: Keynote's default).
    pub label_distance: Option<f32>,
    /// Value labels' place (the chart type's valuelabelposition): 4 inside the bar's end
    /// (Numbers draws them there, in the inside label style; N29), others above / beside it.
    pub label_position: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartAxis {
    /// User-set range (None = automatic).
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Number of major intervals (None = automatic).
    pub intervals: Option<u32>,
    pub show_labels: bool,
    pub show_line: bool,
    pub show_gridlines: bool,
    pub gridline: Option<Stroke>,
    pub label_style: CharStyle,
    pub title: Option<String>,
    /// Label angle, degrees (counter-clockwise; 0 = level).
    pub label_angle: f32,
    /// Labels' number format (TSK.FormatStructArchive; None: automatic).
    pub number_format: Option<kn_format::Msg>,
    pub show_minor_gridlines: bool,
    pub minor_gridline: Option<Stroke>,
    /// Minor intervals between two major gridlines (None: automatic).
    pub minor_intervals: Option<u32>,
    pub show_major_ticks: bool,
    pub show_minor_ticks: bool,
    /// Logarithmic value scale.
    pub log: bool,
}

// ---------------------------------------------------------------------------------- slides

#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    pub effect: String,
    pub duration: f64,
    pub delay: f64,
    pub automatic: bool,
    pub direction: u32,
    pub color: Option<Color>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Build {
    pub id: ObjId,
    pub drawable: ObjId,
    /// "In", "Out" or "Action".
    pub kind: String,
    pub effect: String,
    pub delivery: String,
    pub duration: f64,
    pub direction: u32,
    pub params: BuildParams,
}

/// Effect options of a build (KN.BuildAttributesArchive), all optional.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BuildParams {
    /// 0 none, 1 ease in, 2 ease out, 3 ease in and out, 4 custom.
    pub acceleration: u32,
    /// Rotate action: degrees, and direction (true = clockwise).
    pub rotation_angle: Option<f32>,
    pub rotation_clockwise: bool,
    /// Scale action: target size as a fraction of the current size.
    pub scale: Option<f32>,
    /// Opacity action: target opacity 0..1.
    pub opacity: Option<f32>,
    /// Move action: motion path in slide points (raw path coordinates of the path source).
    pub motion_path: Option<Path>,
    pub bounce: bool,
    /// 1 by object, 2 by word, 3 by character, 4 by line.
    pub text_delivery: u32,
    /// 1 forward, 2 backward, 3 from centre, 4 from edges, 5 random.
    pub delivery_option: u32,
    pub repeat_count: u32,
    pub scale_amount: Option<f32>,
    pub travel_distance: Option<f32>,
    pub angle: Option<f32>,
    pub include_endpoints: bool,
    /// Keyboard build: show the typing cursor (custom_cursor).
    pub cursor: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuildStep {
    pub build: ObjId,
    pub delay: f64,
    pub duration: f64,
    /// Starts automatically after/with the previous one instead of on click.
    pub automatic: bool,
    /// "with previous" (true) vs "after previous" (false) for automatic steps.
    pub with_previous: bool,
    /// Chunk number within its build (1-based; builds delivered by paragraph have several).
    pub chunk: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SlideScene {
    pub slide: ObjId,
    pub number: usize,
    pub size: Size,
    pub background: Fill,
    /// Back to front; layout (master) objects first.
    pub drawables: Vec<Drawable>,
    pub transition: Option<Transition>,
    pub builds: Vec<Build>,
    pub build_order: Vec<BuildStep>,
    /// Dynamic background (drawn animated instead of `background`, which holds its still).
    pub motion: Option<crate::motionbg::MotionBg>,
}
