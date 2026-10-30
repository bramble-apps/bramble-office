//! Inserting objects (text box, shapes, lines, pictures) and copy/paste of objects.
//!
//! New objects are "donor copies": the objects Keynote itself made in the minimal sample files
//! (embedded below) are copied into the document with fresh ids, and their style references are
//! re-pointed at this document's equivalent theme styles.

use std::sync::OnceLock;

use kn_format::{Extract, KeyFile, Msg, ObjId};

use crate::edit::History;
use crate::Document;

// (donors made in Keynote, with every image and preview replaced by original ones:
// kn-render's clean_donor example)
static TEXT_BOX: &[u8] = include_bytes!("../data/donors/01-text-box.key");
static SHAPE: &[u8] = include_bytes!("../data/donors/02-shape.key");
static LINE: &[u8] = include_bytes!("../data/donors/03-line.key");
static IMAGE: &[u8] = include_bytes!("../data/donors/04-image.key");
static CHART_COLUMN: &[u8] = include_bytes!("../data/donors/06-chart-column.key");
static CHART_PIE: &[u8] = include_bytes!("../data/donors/07-chart-pie.key");
/// Keynote's own Insert > Live Video result (Mac round 22): the live video object is copied from it.
static LIVE_VIDEO: &[u8] = include_bytes!("../data/donors/K-livevideo.key");
/// Made by hand in Keynote 15.4; slide 2 holds a fresh, untouched Insert > Table > Basic table.
static TABLE: &[u8] = include_bytes!("../data/donors/R2-table-values.key");
static SHAPES: &[u8] = include_bytes!("../data/basic-shapes.bin");

/// Keynote's 12 basic shapes, in the order of its Shape popover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasicShape {
    Rectangle,
    RoundedRectangle,
    Oval,
    Triangle,
    RightTriangle,
    Arrow,
    DoubleArrow,
    Diamond,
    QuoteBubble,
    Callout,
    Pentagon,
    Star,
}

impl BasicShape {
    pub const ALL: [BasicShape; 12] = [
        BasicShape::Rectangle,
        BasicShape::RoundedRectangle,
        BasicShape::Oval,
        BasicShape::Triangle,
        BasicShape::RightTriangle,
        BasicShape::Arrow,
        BasicShape::DoubleArrow,
        BasicShape::Diamond,
        BasicShape::QuoteBubble,
        BasicShape::Callout,
        BasicShape::Pentagon,
        BasicShape::Star,
    ];
    pub fn name(self) -> &'static str {
        match self {
            BasicShape::Rectangle => "Rectangle",
            BasicShape::RoundedRectangle => "Rounded Rectangle",
            BasicShape::Oval => "Oval",
            BasicShape::Triangle => "Triangle",
            BasicShape::RightTriangle => "Right Triangle",
            BasicShape::Arrow => "Arrow",
            BasicShape::DoubleArrow => "Double Arrow",
            BasicShape::Diamond => "Diamond",
            BasicShape::QuoteBubble => "Quote Bubble",
            BasicShape::Callout => "Callout",
            BasicShape::Pentagon => "Polygon",
            BasicShape::Star => "Star",
        }
    }
    /// The shape's TSD.PathSourceArchive (natural size 320 × 220).
    pub fn path_source(self) -> Msg {
        let i = BasicShape::ALL.iter().position(|s| *s == self).unwrap();
        Msg::parse(&blob(i), kn_format::schema::schema().type_idx("TSD.PathSourceArchive")).unwrap()
    }
}

fn blob(index: usize) -> Vec<u8> {
    let mut pos = 0;
    let mut i = 0;
    while pos + 4 <= SHAPES.len() {
        let n = u32::from_le_bytes(SHAPES[pos..pos + 4].try_into().unwrap()) as usize;
        if i == index {
            return SHAPES[pos + 4..pos + 4 + n].to_vec();
        }
        pos += 4 + n;
        i += 1;
    }
    Vec::new()
}

/// Keynote's "simple arrow" line end.
pub fn arrow_line_end() -> Msg {
    Msg::parse(&blob(12), kn_format::schema::schema().type_idx("TSD.LineEndArchive")).unwrap()
}

#[derive(Clone, Debug)]
pub enum NewObject {
    TextBox,
    Shape(BasicShape),
    Line { arrow: bool },
    /// PNG or JPEG bytes + file name.
    Image(Vec<u8>, String),
    /// 2D column chart (`pie` = false) or pie chart with Keynote's sample data.
    Chart { pie: bool },
    /// Empty table in the theme's Basic style (header row; Keynote's default is 5 × 4).
    Table { rows: u32, cols: u32 },
}

impl NewObject {
    /// Keynote's Insert > Table > Basic.
    pub const BASIC_TABLE: NewObject = NewObject::Table { rows: 5, cols: 4 };
}

fn donor(which: usize) -> &'static KeyFile {
    static D: [OnceLock<KeyFile>; 7] = [const { OnceLock::new() }; 7];
    D[which].get_or_init(|| KeyFile::from_zip([TEXT_BOX, SHAPE, LINE, IMAGE, CHART_COLUMN, CHART_PIE, TABLE][which]).expect("embedded donor"))
}

/// The table on the table donor's second slide.
fn donor_table(k: &KeyFile) -> Option<ObjId> {
    let (_, slide) = *k.slide_nodes().get(1)?;
    k.obj(slide)?.ref_ids("drawables_z_order").into_iter().find(|d| k.type_name(*d) == "TST.TableInfoArchive")
}

/// The one object the donor file's first slide adds.
fn donor_object(k: &KeyFile) -> Option<ObjId> {
    let (_, slide) = *k.slide_nodes().first()?;
    k.obj(slide)?.ref_ids("drawables_z_order").into_iter().next()
}

/// Objects copied from a document (an in-app clipboard).
#[derive(Clone)]
pub struct Clip {
    pub objects: Vec<Extract>,
    /// The document they came from (to re-point styles when pasting into another one).
    pub source: std::sync::Arc<KeyFile>,
    pub source_id: String,
}

/// Identifies an open document (its revision id + where it was opened from).
pub fn doc_identity(doc: &Document) -> String {
    let rev = doc.file.package_metadata().and_then(|p| p.msg("revision")).and_then(|r| r.str("identifier")).unwrap_or_default();
    format!("{rev}|{:?}", doc.path)
}

impl Document {
    /// Insert a new object centred on `slide`. Returns its id.
    pub fn insert(&mut self, h: &mut History, slide: ObjId, obj: &NewObject) -> Result<ObjId, kn_format::Error> {
        let which = match obj {
            NewObject::TextBox => 0,
            NewObject::Shape(_) => 1,
            NewObject::Line { .. } => 2,
            NewObject::Image(..) => 3,
            NewObject::Chart { pie } => 4 + usize::from(*pie),
            NewObject::Table { .. } => 6,
        };
        let d = donor(which);
        if let NewObject::Table { rows, cols } = obj {
            if (*rows, *cols) != (5, 4) {
                return Err(kn_format::Error::Invalid("only 5 × 4 tables can be inserted for now".into()));
            }
            let info = donor_table(d).ok_or(kn_format::Error::Invalid("donor has no table".into()))?;
            let mut touch = self.file.table_insert_touches();
            touch.push(slide);
            let id = self.structural(h, &touch, |k| k.paste_table(d, info, slide))?;
            let size = self.slide_size();
            let mut g = crate::scene::geometry(self.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).unwrap());
            g.x = ((size.w - g.w) / 2.0).round();
            g.y = ((size.h - g.h) / 2.0).round();
            self.set_frame(h, id, g);
            self.mark_thumbnail_dirty(h, slide);
            return Ok(id);
        }
        let root = donor_object(d).ok_or(kn_format::Error::Invalid("donor has no object".into()))?;
        let ex = d.extract(root)?;
        let id = if which >= 4 {
            self.paste_chart(h, d, &ex, slide)?
        } else {
            let map = self.file.map_external(d, &ex.external)?;
            self.structural(h, &[slide], |k| k.paste_extract(&ex, slide, Some(&map)))?
        };
        let size = self.slide_size();
        let mut g = crate::scene::geometry(self.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).unwrap());
        match obj {
            NewObject::TextBox => {
                if let Some(st) = self.storage_of(id) {
                    self.set_storage_text(h, st, "Text");
                }
                // fixed width, height grows with the text (Keynote's free text box)
                g.w = 300.0;
                g.h = 60.0;
                g.auto_width = false;
                g.auto_height = true;
            }
            NewObject::Shape(s) => {
                let ps = s.path_source();
                if let Some(sh) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) {
                    sh.remove("pathsource");
                    sh.push_msg("pathsource", ps);
                }
                (g.w, g.h) = (320.0, 220.0);
            }
            NewObject::Line { arrow } => {
                if let Some(sh) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) {
                    if let Some(bp) = sh.msg_mut("pathsource").and_then(|p| p.msg_mut("bezier_path_source")) {
                        let ns = bp.msg_mut_or_create("naturalSize");
                        ns.set_f32("width", 400.0);
                        ns.set_f32("height", 0.0);
                        if let Some(path) = bp.msg_mut("path") {
                            let pts: Vec<&mut Msg> = path.msgs_mut("elements").flat_map(|e| e.msgs_mut("points")).collect();
                            for (i, p) in pts.into_iter().enumerate() {
                                p.set_f32("x", if i == 0 { 0.0 } else { 400.0 });
                                p.set_f32("y", 0.0);
                            }
                        }
                    }
                }
                (g.w, g.h, g.angle) = (400.0, 0.0, 0.0);
                if *arrow {
                    let end = arrow_line_end();
                    self.set_line_ends(h, id, None, Some(&end));
                }
            }
            NewObject::Chart { pie } => {
                // Keynote's default data for a new chart (verified on the Mac, round 15)
                let grid = if *pie {
                    crate::chartdata::ChartGrid {
                        row_names: vec!["Region 1".into()],
                        col_names: ["April", "May", "June", "July", "August", "September"].map(String::from).to_vec(),
                        values: vec![vec![91.0, 76.0, 28.0, 26.0, 21.0, 18.0]],
                    }
                } else {
                    crate::chartdata::ChartGrid {
                        row_names: vec!["Region 1".into(), "Region 2".into()],
                        col_names: ["April", "May", "June", "July"].map(String::from).to_vec(),
                        values: vec![vec![17.0, 26.0, 53.0, 96.0], vec![55.0, 43.0, 70.0, 58.0]],
                    }
                };
                self.set_chart_grid(h, id, &grid);
                if let Some(u) = self.file.obj_mut(id).and_then(|m| m.msg_mut("unity")) {
                    u.set_enum("series_direction", if *pie { "series_direction_by_column" } else { "series_direction_by_row" });
                }
            }
            NewObject::Table { .. } => {}
            NewObject::Image(bytes, name) => {
                let (pw, ph) = kn_format::objects::image_size(bytes).ok_or(kn_format::Error::Invalid("only PNG and JPEG pictures are supported".into()))?;
                let bytes = bytes.clone();
                let name = name.clone();
                let data = self.structural(h, &[], |k| k.add_image_data(bytes, &name))?;
                self.file.set_image_data(id, data, pw as f32, ph as f32);
                let s = (size.w * 0.6 / pw as f32).min(size.h * 0.6 / ph as f32).min(1.0);
                (g.w, g.h) = (pw as f32 * s, ph as f32 * s);
            }
        }
        g.x = ((size.w - g.w) / 2.0).round();
        g.y = ((size.h - g.h) / 2.0).round();
        self.set_frame(h, id, g);
        self.file.refresh_refs(id);
        self.mark_thumbnail_dirty(h, slide);
        Ok(id)
    }

    /// Insert > Live Video: Keynote's live video object (the default camera, no placeholder),
    /// centred on `slide`. Returns its id.
    pub fn insert_live_video(&mut self, h: &mut History, slide: ObjId) -> Result<ObjId, kn_format::Error> {
        static D: OnceLock<KeyFile> = OnceLock::new();
        let d = D.get_or_init(|| KeyFile::from_zip(LIVE_VIDEO).expect("embedded live video"));
        let root = d
            .objects_of("TSD.MovieArchive")
            .find(|(id, m)| m.bool("is_live_video") == Some(true) && d.file_of(*id).is_some_and(|f| f.starts_with("Index/Slide-")))
            .map(|(id, _)| id)
            .ok_or(kn_format::Error::Invalid("no live video in the donor".into()))?;
        if self.file.first_of("KN.LiveVideoSourceCollection").is_none() {
            return Err(kn_format::Error::Invalid("this presentation has no live video sources".into()));
        }
        let ex = d.extract(root)?;
        let map = self.file.map_external(d, &ex.external)?;
        let id = self.structural(h, &[slide], |k| k.paste_extract(&ex, slide, Some(&map)))?;
        let size = self.slide_size();
        let mut g = crate::scene::geometry(self.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).unwrap());
        g.x = ((size.w - g.w) / 2.0).round();
        g.y = ((size.h - g.h) / 2.0).round();
        self.set_frame(h, id, g);
        self.file.refresh_refs(id);
        self.mark_thumbnail_dirty(h, slide);
        Ok(id)
    }

    /// The shared ObjectContainer component and its file, created (empty, from the chart
    /// donor's) when the document has none.
    pub(crate) fn ensure_object_container(&mut self, h: &mut History) -> Result<(ObjId, String), kn_format::Error> {
        const OC_FILE: &str = "Index/ObjectContainer.iwa";
        if let Some((oc, _)) = self.file.first_of("TSP.ObjectContainer") {
            return Ok((oc, self.file.file_of(oc).unwrap_or(OC_FILE).to_string()));
        }
        let d = donor(4);
        let (oc_root, _) = d.first_of("TSP.ObjectContainer").ok_or(kn_format::Error::Invalid("donor has no object container".into()))?;
        if self.file.archive(oc_root).is_some() || self.file.iwa(OC_FILE).is_some() {
            return Err(kn_format::Error::Invalid("unexpected object container layout".into()));
        }
        self.structural(h, &[], |k| {
            let mut a = d.archive(oc_root).unwrap().clone();
            a.obj_mut().remove("objects");
            a.set_object_references(0, &[]);
            k.add_entry(kn_format::Entry { name: OC_FILE.into(), data: kn_format::EntryData::Iwa(kn_format::IwaFile { archives: vec![a] }) });
            let mut comp = d.component(oc_root).unwrap().clone();
            comp.remove("object_uuid_map_entries");
            comp.remove("external_references");
            comp.remove("data_references");
            k.package_metadata_mut().unwrap().push_msg("components", comp);
        });
        Ok((oc_root, OC_FILE.to_string()))
    }

    /// A chart's non-style objects live in the shared ObjectContainer component (created when
    /// the document has none); its styles map onto the theme's chart styles.
    fn paste_chart(&mut self, h: &mut History, d: &KeyFile, ex: &Extract, slide: ObjId) -> Result<ObjId, kn_format::Error> {
        const OC_FILE: &str = "Index/ObjectContainer.iwa";
        let (oc_ids, style_ids): (Vec<ObjId>, Vec<ObjId>) = ex.external.iter().partition(|i| d.file_of(**i) == Some(OC_FILE));
        let mut map = self.file.map_external(d, &style_ids)?;
        let (oc_root, _) = d.first_of("TSP.ObjectContainer").ok_or(kn_format::Error::Invalid("donor has no object container".into()))?;
        let have_oc = self.file.first_of("TSP.ObjectContainer").map(|(i, _)| i);
        if have_oc.is_none() && (self.file.archive(oc_root).is_some() || self.file.iwa(OC_FILE).is_some()) {
            return Err(kn_format::Error::Invalid("unexpected object container layout".into()));
        }
        let touch: Vec<ObjId> = have_oc.into_iter().chain([slide]).collect();
        self.structural(h, &touch, |k| {
            let oc = match have_oc {
                Some(oc) => oc,
                None => {
                    let mut a = d.archive(oc_root).unwrap().clone();
                    a.obj_mut().remove("objects");
                    a.set_object_references(0, &[]);
                    k.add_entry(kn_format::Entry { name: OC_FILE.into(), data: kn_format::EntryData::Iwa(kn_format::IwaFile { archives: vec![a] }) });
                    let mut comp = d.component(oc_root).unwrap().clone();
                    comp.remove("object_uuid_map_entries");
                    comp.remove("external_references");
                    comp.remove("data_references");
                    k.package_metadata_mut().unwrap().push_msg("components", comp);
                    oc_root
                }
            };
            let file = k.file_of(oc).unwrap_or(OC_FILE).to_string();
            let new_ids = k.alloc_ids(oc_ids.len() as u64);
            let oc_map: std::collections::HashMap<ObjId, ObjId> = oc_ids.iter().copied().zip(new_ids.iter().copied()).collect();
            for (old, new) in &oc_map {
                let mut a = d.archive(*old).unwrap().clone();
                a.info.set_u64("identifier", *new);
                for p in &mut a.payloads {
                    p.for_each_ref_mut(&mut |r| {
                        if let Some(n) = oc_map.get(r) {
                            *r = *n;
                        }
                    });
                }
                k.insert_archive(&file, a);
                if let Some(o) = k.obj_mut(oc) {
                    o.push_ref("objects", *new);
                }
                k.add_uuid_entry(&file, *new);
            }
            map.extend(oc_map);
            k.paste_extract(ex, slide, Some(&map))
        })
    }

    fn mark_thumbnail_dirty(&mut self, h: &mut History, slide: ObjId) {
        if let Some(node) = self.slides().into_iter().find(|i| i.slide == slide).map(|i| i.node) {
            self.touch(h, node);
            if let Some(n) = self.file.obj_mut(node) {
                n.set_bool("thumbnailsAreDirty", true);
            }
        }
    }

    /// Copy objects (placeholders can't be copied and are skipped; so are tables, whose data
    /// lives in separate files and the calculation engine that a plain copy would not carry).
    pub fn copy_objects(&self, ids: &[ObjId]) -> Option<Clip> {
        let objects: Vec<Extract> = ids
            .iter()
            .filter(|id| !matches!(self.file.type_name(**id), "KN.PlaceholderArchive" | "TST.TableInfoArchive"))
            .filter_map(|id| self.file.extract(*id).ok())
            .collect();
        (!objects.is_empty()).then(|| Clip { objects, source: std::sync::Arc::new(self.file.clone()), source_id: doc_identity(self) })
    }

    /// Paste copied objects onto `slide`, moved by `offset` points. Returns the new ids.
    pub fn paste_objects(&mut self, h: &mut History, slide: ObjId, clip: &Clip, offset: f32) -> Result<Vec<ObjId>, kn_format::Error> {
        let same = clip.source_id == doc_identity(self);
        let mut out = Vec::new();
        for ex in &clip.objects {
            // charts from another presentation bring their object-container pieces along
            if !same && clip.source.type_name(ex.root) == "TSCH.ChartDrawableArchive" {
                let id = self.paste_chart(h, &clip.source, ex, slide)?;
                if offset != 0.0 {
                    self.move_by(h, id, offset, offset);
                }
                out.push(id);
                continue;
            }
            let map = if same { None } else { Some(self.file.map_external(&clip.source, &ex.external)?) };
            let id = self.structural(h, &[slide], |k| {
                if same {
                    return k.paste_extract(ex, slide, None);
                }
                // pictures/movies come along with the objects
                let data: std::collections::HashMap<u64, u64> = KeyFile::extract_data_ids(ex)
                    .into_iter()
                    .filter_map(|d| Some((d, k.copy_data_from(&clip.source, d)?)))
                    .collect();
                k.paste_extract_with_data(ex, slide, map.as_ref(), Some(&data))
            })?;
            if offset != 0.0 {
                self.move_by(h, id, offset, offset);
            }
            out.push(id);
        }
        self.mark_thumbnail_dirty(h, slide);
        Ok(out)
    }
}

/// Insert > Line > Straight / Curved / Angled Connection Line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Connection {
    Straight,
    Curved,
    Angled,
}

impl Document {
    /// Insert > Line > Line with Two Arrowheads.
    pub fn insert_double_arrow_line(&mut self, h: &mut History, slide: ObjId) -> Result<ObjId, kn_format::Error> {
        let id = self.insert(h, slide, &NewObject::Line { arrow: true })?;
        let end = arrow_line_end();
        self.set_line_ends(h, id, Some(&end), Some(&end));
        Ok(id)
    }

    /// Insert a connection line (TSD.ConnectionLineArchive), joining `ends` (two objects on the
    /// slide) when given, else a free one in the middle of the slide.
    pub fn insert_connection_line(&mut self, h: &mut History, slide: ObjId, kind: Connection, ends: Option<(ObjId, ObjId)>) -> Result<ObjId, kn_format::Error> {
        let id = self.insert(h, slide, &NewObject::Line { arrow: false })?;
        self.make_connection_line(h, id, kind, ends)?;
        Ok(id)
    }

    /// Turn line `id` into a connection line joining `ends` (or a free one). For documents that
    /// insert lines from their own donor (Letters).
    pub fn make_connection_line(&mut self, h: &mut History, id: ObjId, kind: Connection, ends: Option<(ObjId, ObjId)>) -> Result<(), kn_format::Error> {
        self.touch(h, id);
        let tid = kn_format::schema::schema().type_id_of("TSD.ConnectionLineArchive").ok_or(kn_format::Error::Invalid("no connection line type".into()))?;
        {
            let a = self.file.archive_mut(id).ok_or(kn_format::Error::Invalid("no line".into()))?;
            // the super is a TSD.ShapeArchive (a line from a ShapeInfo donor gives its shape part)
            let shape = a.payloads[0].find_super("TSD.ShapeArchive").cloned().unwrap_or_else(|| a.payloads[0].clone());
            let mut cl = Msg::new("TSD.ConnectionLineArchive");
            cl.push_msg("super", shape);
            a.payloads[0] = cl;
            if let Some(mi) = a.info.msgs_mut("message_infos").next() {
                mi.set_u64("type", u64::from(tid));
            }
        }
        if let Some((from, to)) = ends {
            if let Some(m) = self.file.obj_mut(id) {
                m.set_ref("connected_from", from);
                m.set_ref("connected_to", to);
            }
        }
        self.layout_connection(h, id, kind);
        self.file.refresh_refs(id);
        Ok(())
    }

    /// Recompute a connection line's frame and path from the objects it joins (or keep its own
    /// ends when free). `kind` None keeps the stored curve type.
    fn layout_connection(&mut self, h: &mut History, id: ObjId, kind: Connection) {
        let Some(m) = self.file.obj(id) else { return };
        let frame_of = |o: ObjId| self.file.obj(o).and_then(|x| x.find_super("TSD.DrawableArchive")).map(crate::scene::geometry);
        let ends = match (m.ref_id("connected_from").and_then(frame_of), m.ref_id("connected_to").and_then(frame_of)) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        };
        let (a, b) = match ends {
            Some((fa, fb)) => {
                let (ca, cb) = (fa.center(), fb.center());
                // from the edge of each object's box along the line between their centres
                let clip = |g: &crate::Geometry, from: crate::Point, to: crate::Point| {
                    let (dx, dy) = (to.x - from.x, to.y - from.y);
                    let tx = if dx.abs() > 1e-3 { (g.w / 2.0) / dx.abs() } else { f32::INFINITY };
                    let ty = if dy.abs() > 1e-3 { (g.h / 2.0) / dy.abs() } else { f32::INFINITY };
                    let t = tx.min(ty).min(1.0);
                    crate::Point::new(from.x + dx * t, from.y + dy * t)
                };
                (clip(&fa, ca, cb), clip(&fb, cb, ca))
            }
            None => {
                let s = self.slide_size();
                (crate::Point::new(s.w / 2.0 - 150.0, s.h / 2.0 + 100.0), crate::Point::new(s.w / 2.0 + 150.0, s.h / 2.0 - 100.0))
            }
        };
        let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
        let (w, hgt) = ((a.x - b.x).abs().max(1.0), (a.y - b.y).abs().max(1.0));
        let (la, lb) = ((a.x - x0, a.y - y0), (b.x - x0, b.y - y0));
        let mid = match kind {
            // the bend: curved lines bow out a quarter of their length
            Connection::Curved => {
                let (mx, my) = ((la.0 + lb.0) / 2.0, (la.1 + lb.1) / 2.0);
                let (dx, dy) = (lb.0 - la.0, lb.1 - la.1);
                (mx - dy * 0.25, my + dx * 0.25)
            }
            _ => ((la.0 + lb.0) / 2.0, (la.1 + lb.1) / 2.0),
        };
        self.touch(h, id);
        let Some(m) = self.file.obj_mut(id) else { return };
        let Some(sh) = m.find_super_mut("TSD.ShapeArchive") else { return };
        sh.remove("pathsource");
        let mut ps = Msg::new("TSD.PathSourceArchive");
        ps.set_bool("horizontalFlip", false);
        ps.set_bool("verticalFlip", false);
        {
            let c = ps.msg_mut_or_create("connection_line_path_source");
            let b = c.msg_mut_or_create("super");
            let ns = b.msg_mut_or_create("naturalSize");
            ns.set_f32("width", w);
            ns.set_f32("height", hgt);
            let path = b.msg_mut_or_create("path");
            for (i, (x, y)) in [la, mid, lb].into_iter().enumerate() {
                let mut e = Msg::new("TSP.Path.Element");
                e.set_enum("type", if i == 0 { "moveTo" } else { "lineTo" });
                let mut p = Msg::new("TSP.Point");
                p.set_f32("x", x);
                p.set_f32("y", y);
                e.push_msg("points", p);
                path.push_msg("elements", e);
            }
            c.set_enum("type", if kind == Connection::Angled { "kTSDConnectionLineTypeOrthogonal" } else { "kTSDConnectionLineTypeQuadratic" });
            c.set_f32("outset_from", 0.0);
            c.set_f32("outset_to", 0.0);
        }
        sh.push_msg("pathsource", ps);
        if let Some(d) = sh.find_super_mut("TSD.DrawableArchive") {
            let g = d.msg_mut_or_create("geometry");
            let p = g.msg_mut_or_create("position");
            p.set_f32("x", x0);
            p.set_f32("y", y0);
            let s = g.msg_mut_or_create("size");
            s.set_f32("width", w);
            s.set_f32("height", hgt);
            g.set_f32("angle", 0.0);
        }
    }

    /// After objects moved or resized: re-route the connection lines attached to them.
    pub fn update_connections(&mut self, h: &mut History, slide: ObjId, moved: &[ObjId]) {
        let Some(s) = self.file.obj(slide) else { return };
        let lines = s.ref_ids("drawables_z_order");
        self.update_connections_in(h, &lines, moved);
    }

    /// Re-route those of `lines` (any drawables; only connection lines count) attached to the
    /// `moved` objects. For documents whose drawables aren't on a slide (Letters passes every
    /// connection line in the file).
    pub fn update_connections_in(&mut self, h: &mut History, lines: &[ObjId], moved: &[ObjId]) {
        let lines: Vec<(ObjId, Connection)> = lines
            .iter()
            .copied()
            .filter(|l| self.file.type_name(*l) == "TSD.ConnectionLineArchive")
            .filter_map(|l| {
                let m = self.file.obj(l)?;
                let attached = [m.ref_id("connected_from"), m.ref_id("connected_to")].iter().flatten().any(|o| moved.contains(o));
                if !attached {
                    return None;
                }
                let c = m.find_super("TSD.ShapeArchive")?.msg("pathsource")?.msg("connection_line_path_source")?;
                let kind = if c.enum_name("type") == Some("kTSDConnectionLineTypeOrthogonal") {
                    Connection::Angled
                } else {
                    // a bend off the straight line means curved
                    let pts: Vec<(f32, f32)> = c.msg("super").and_then(|b| b.msg("path")).map(|p| p.msgs("elements").filter_map(|e| e.msg("points")).map(|q| (q.f32("x").unwrap_or(0.0), q.f32("y").unwrap_or(0.0))).collect()).unwrap_or_default();
                    match pts.as_slice() {
                        [a, m, b] if ((m.0 - (a.0 + b.0) / 2.0).abs() + (m.1 - (a.1 + b.1) / 2.0).abs()) > 1.0 => Connection::Curved,
                        _ => Connection::Straight,
                    }
                };
                Some((l, kind))
            })
            .collect();
        for (l, kind) in lines {
            self.layout_connection(h, l, kind);
        }
    }
}

/// A movie or sound to insert (Insert > Choose…, Record Audio, the media browsers).
pub struct NewMovie {
    pub bytes: Vec<u8>,
    pub name: String,
    /// Still shown before it plays (JPEG), for movies.
    pub poster: Option<Vec<u8>>,
    /// Pixel size (movies) — sounds get Keynote's speaker-sized frame.
    pub natural: Option<(f32, f32)>,
    pub duration: f32,
    pub audio_only: bool,
}

impl Document {
    /// Insert a movie / sound (TSD.MovieArchive) in the middle of the slide.
    pub fn insert_movie(&mut self, h: &mut History, slide: ObjId, m: NewMovie) -> Result<ObjId, kn_format::Error> {
        let size = self.slide_size();
        let style = self
            .file
            .objects_of("TSD.MediaStyleArchive")
            .find(|(_, s)| s.find_super("TSS.StyleArchive").and_then(|t| t.str("style_identifier")).is_some_and(|i| i.starts_with(if m.audio_only { "audio" } else { "movie" })))
            .or_else(|| self.file.objects_of("TSD.MediaStyleArchive").next())
            .map(|(i, _)| i);
        let (nw, nh) = m.natural.unwrap_or((128.0, 128.0));
        let (w, hgt) = if m.audio_only {
            (128.0, 128.0)
        } else {
            let k = (size.w * 0.6 / nw).min(size.h * 0.6 / nh).min(1.0);
            (nw * k, nh * k)
        };
        let file = self.file.file_of(slide).ok_or(kn_format::Error::Invalid("no slide".into()))?.to_string();
        let NewMovie { bytes, name, poster, duration, audio_only, .. } = m;
        self.structural(h, &[slide], |k| {
            let movie_data = k.add_file_data(bytes, &name)?;
            let poster_data = match poster {
                Some(p) => Some(k.add_image_data(p, "posterImage.jpeg")?),
                None => None,
            };
            let id = k.alloc_ids(1)[0];
            let mut d = Msg::new("TSD.DrawableArchive");
            {
                let g = d.msg_mut_or_create("geometry");
                let p = g.msg_mut_or_create("position");
                p.set_f32("x", ((size.w - w) / 2.0).round());
                p.set_f32("y", ((size.h - hgt) / 2.0).round());
                let s = g.msg_mut_or_create("size");
                s.set_f32("width", w);
                s.set_f32("height", hgt);
                g.set_u64("flags", 3);
                g.set_f32("angle", 0.0);
            }
            d.set_ref("parent", slide);
            d.set_bool("locked", false);
            d.set_bool("aspect_ratio_locked", true);
            let mut mv = Msg::new("TSD.MovieArchive");
            mv.push_msg("super", d);
            mv.set_f32("startTime", 0.0);
            mv.set_f32("endTime", duration.max(0.0));
            mv.set_f32("posterTime", 0.0);
            mv.set_f32("volume", 1.0);
            mv.set_bool("audioOnly", audio_only);
            mv.set_u64("flags", 0);
            let dr = |i: u64| {
                let mut r = Msg::new("TSP.DataReference");
                r.set_u64("identifier", i);
                r
            };
            mv.push_msg("movieData", dr(movie_data));
            if let Some(p) = poster_data {
                mv.push_msg("posterImageData", dr(p));
            }
            mv.set_bool("streaming", false);
            if let Some(st) = style {
                mv.set_ref("style", st);
            }
            for f in ["originalSize", "naturalSize"] {
                let s = mv.msg_mut_or_create(f);
                s.set_f32("width", nw);
                s.set_f32("height", nh);
            }
            mv.set_enum("loop_option", "None");
            let mut a = kn_format::Archive::new_object(id, mv);
            let datas: Vec<u64> = std::iter::once(movie_data).chain(poster_data).collect();
            a.set_data_references(0, &datas);
            k.insert_archive(&file, a);
            k.add_uuid_entry(&file, id);
            for dd in datas {
                k.add_data_reference(&file, dd, id);
            }
            k.attach_to_slide(slide, id);
            k.ensure_external_refs(&file);
            Ok(id)
        })
    }
}

impl Document {
    /// Insert > Line > Draw With Pen: a freehand stroke (slide points) as a smooth open path.
    pub fn insert_freehand(&mut self, h: &mut History, slide: ObjId, pts: &[crate::Point], color: crate::Color, width: f32) -> Result<ObjId, kn_format::Error> {
        if pts.len() < 2 {
            return Err(kn_format::Error::Invalid("too short".into()));
        }
        let id = self.insert(h, slide, &NewObject::Line { arrow: false })?;
        let (x0, y0) = pts.iter().fold((f32::INFINITY, f32::INFINITY), |a, p| (a.0.min(p.x), a.1.min(p.y)));
        let (x1, y1) = pts.iter().fold((f32::NEG_INFINITY, f32::NEG_INFINITY), |a, p| (a.0.max(p.x), a.1.max(p.y)));
        let (w, hh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
        let local: Vec<(f32, f32)> = pts.iter().map(|p| (p.x - x0, p.y - y0)).collect();
        let el = |kind: &str, ps: &[(f32, f32)]| {
            let mut e = Msg::new("TSP.Path.Element");
            e.set_enum("type", kind);
            for (x, y) in ps {
                let mut p = Msg::new("TSP.Point");
                p.set_f32("x", *x);
                p.set_f32("y", *y);
                e.push_msg("points", p);
            }
            e
        };
        let mut path = Msg::new("TSP.Path");
        path.push_msg("elements", el("moveTo", &local[..1]));
        // Catmull-Rom through the samples, as cubic curves
        for i in 0..local.len() - 1 {
            let p0 = local[i.saturating_sub(1)];
            let (p1, p2) = (local[i], local[i + 1]);
            let p3 = local[(i + 2).min(local.len() - 1)];
            let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
            let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
            path.push_msg("elements", el("curveTo", &[c1, c2, p2]));
        }
        self.touch(h, id);
        if let Some(sh) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")) {
            sh.remove("pathsource");
            let mut ps = Msg::new("TSD.PathSourceArchive");
            ps.set_bool("horizontalFlip", false);
            ps.set_bool("verticalFlip", false);
            {
                let b = ps.msg_mut_or_create("bezier_path_source");
                let ns = b.msg_mut_or_create("naturalSize");
                ns.set_f32("width", w);
                ns.set_f32("height", hh);
                b.push_msg("path", path);
            }
            sh.push_msg("pathsource", ps);
            if let Some(d) = sh.find_super_mut("TSD.DrawableArchive") {
                let g = d.msg_mut_or_create("geometry");
                let p = g.msg_mut_or_create("position");
                p.set_f32("x", x0);
                p.set_f32("y", y0);
                let s = g.msg_mut_or_create("size");
                s.set_f32("width", w);
                s.set_f32("height", hh);
                g.set_f32("angle", 0.0);
            }
        }
        self.set_stroke(h, id, Some(&crate::format::StrokeSpec { color, width, dash: crate::format::Dash::Solid }));
        Ok(id)
    }
}
