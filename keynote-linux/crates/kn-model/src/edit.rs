//! Editing operations on a [`Document`]. Each operation snapshots the archives it touches so it
//! can be undone; everything is written straight into the KeyFile (lossless save).

use std::time::{Duration, Instant};

use kn_format::{Archive, Entry, KeyFile, ObjId};

use crate::{Document, Geometry};

/// Archives as they were before one user action.
#[derive(Default)]
pub struct Snapshot {
    archives: Vec<(ObjId, Archive)>,
    /// The file of each touched archive (undo puts one back that the action removed).
    files: std::collections::HashMap<ObjId, (String, usize)>,
    /// Archives the action created (undo removes them).
    added: Vec<ObjId>,
    /// Archives the action removed, with their file (undo puts them back).
    readd: Vec<(String, Archive)>,
    /// Package entries (data files, .iwa files) the action created / removed.
    added_entries: Vec<String>,
    readd_entries: Vec<Entry>,
    /// Whole-file snapshot for structural edits (slides added/removed).
    whole: Option<Box<KeyFile>>,
    pub label: String,
    ended: Option<Instant>,
}

impl Snapshot {
    fn new(label: &str) -> Snapshot {
        Snapshot { label: label.to_string(), ..Default::default() }
    }
    fn is_empty(&self) -> bool {
        self.archives.is_empty()
            && self.added.is_empty()
            && self.readd.is_empty()
            && self.added_entries.is_empty()
            && self.readd_entries.is_empty()
            && self.whole.is_none()
    }
}

#[derive(Default)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Open snapshot being filled by the current action.
    current: Option<Snapshot>,
}

impl History {
    /// Steps that can be undone (automated checks).
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }
    /// True if the open action created this object (so it can be changed in place).
    pub fn created_now(&self, id: ObjId) -> bool {
        self.current.as_ref().is_some_and(|c| c.added.contains(&id))
    }
}

fn restore(doc: &mut Document, snap: Snapshot) -> Snapshot {
    if let Some(k) = snap.whole {
        let old = std::mem::replace(&mut doc.file, *k);
        return Snapshot { whole: Some(Box::new(old)), ..Snapshot::new(&snap.label) };
    }
    let mut inverse = Snapshot::new(&snap.label);
    for e in snap.readd_entries {
        inverse.added_entries.push(e.name.clone());
        doc.file.add_entry(e);
    }
    for (f, a) in snap.readd {
        inverse.added.push(a.id());
        doc.file.insert_archive(&f, a);
    }
    for (id, a) in snap.archives {
        if let Some(cur) = doc.file.archive_mut(id) {
            inverse.archives.push((id, std::mem::replace(cur, a)));
        } else if let Some((f, pos)) = snap.files.get(&id) {
            // touched, then removed by the action: back it comes, where it was
            doc.file.insert_archive_at(f, *pos, a);
            inverse.added.push(id);
        }
    }
    for id in snap.added {
        if let Some(fa) = doc.file.remove_archive(id) {
            inverse.readd.push(fa);
        }
    }
    for n in snap.added_entries {
        if let Some(e) = doc.file.remove_entry(&n) {
            inverse.readd_entries.push(e);
        }
    }
    inverse
}

impl Document {
    /// Start an undoable action; touched objects are recorded via [`Document::touch`].
    pub fn begin(&mut self, history: &mut History, label: &str) {
        self.end(history);
        history.current = Some(Snapshot::new(label));
    }

    /// Like [`Document::begin`], but continues the previous action if it had the same label and
    /// ended moments ago (slider drags, typing in a field: one undo step, not one per frame).
    pub fn begin_merge(&mut self, history: &mut History, label: &str) {
        self.end(history);
        let recent = history.undo.last().is_some_and(|s| {
            s.label == label && s.whole.is_none() && s.ended.is_some_and(|t| t.elapsed() < Duration::from_millis(1500))
        });
        if recent && history.redo.is_empty() {
            history.current = history.undo.pop();
        } else {
            history.current = Some(Snapshot::new(label));
        }
    }

    /// Close the current action (no-op if nothing was touched).
    pub fn end(&mut self, history: &mut History) {
        if let Some(mut s) = history.current.take() {
            if !s.is_empty() {
                s.ended = Some(Instant::now());
                history.undo.push(s);
                history.redo.clear();
                if history.undo.len() > 500 {
                    history.undo.remove(0);
                }
                // whole-file snapshots are big: keep only the most recent 20
                let mut wholes = 0;
                for snap in history.undo.iter_mut().rev() {
                    if snap.whole.is_some() {
                        wholes += 1;
                        if wholes > 20 {
                            snap.whole = None;
                        }
                    }
                }
                history.undo.retain(|s| !s.is_empty());
            }
        }
    }

    /// Snapshot the whole file before a structural edit (call right after `begin`).
    pub fn touch_all(&mut self, history: &mut History) {
        if let Some(cur) = history.current.as_mut() {
            if cur.whole.is_none() {
                // undo the parts recorded so far first, so the whole snapshot is the state before
                let partial = std::mem::replace(cur, Snapshot::new(&cur.label.clone()));
                let label = partial.label.clone();
                let redo = restore(self, partial);
                let before = self.file.clone();
                restore(self, redo);
                *history.current.as_mut().unwrap() = Snapshot { whole: Some(Box::new(before)), ..Snapshot::new(&label) };
            }
        }
        self.dirty = true;
    }

    /// Record an object's state before modifying it (once per action).
    pub fn touch(&mut self, history: &mut History, id: ObjId) {
        let Some(cur) = history.current.as_mut() else { return };
        if cur.whole.is_some() || cur.added.contains(&id) || cur.archives.iter().any(|(i, _)| *i == id) {
            return;
        }
        if let Some(a) = self.file.archive(id) {
            cur.archives.push((id, a.clone()));
            if let (Some(f), Some(pos)) = (self.file.file_of(id), self.file.archive_position(id)) {
                cur.files.insert(id, (f.to_string(), pos));
            }
        }
        self.dirty = true;
    }

    /// Run an edit that may create archives or package entries: `touch` lists existing objects
    /// it changes (the package metadata is always recorded). Created objects are removed again on
    /// undo.
    pub fn structural<R>(&mut self, history: &mut History, touch: &[ObjId], f: impl FnOnce(&mut KeyFile) -> R) -> R {
        if let Some((pm, _)) = self.file.first_of("TSP.PackageMetadata") {
            self.touch(history, pm);
        }
        for id in touch {
            self.touch(history, *id);
        }
        let last = self.file.last_object_identifier();
        let names: std::collections::HashSet<String> = self.file.entries.iter().map(|e| e.name.clone()).collect();
        let r = f(&mut self.file);
        let new_last = self.file.last_object_identifier();
        if let Some(cur) = history.current.as_mut() {
            if cur.whole.is_none() {
                for id in last + 1..=new_last {
                    if self.file.archive(id).is_some() {
                        cur.added.push(id);
                    }
                }
                for e in &self.file.entries {
                    if !names.contains(&e.name) {
                        cur.added_entries.push(e.name.clone());
                    }
                }
            }
        }
        self.dirty = true;
        r
    }

    pub fn undo(&mut self, history: &mut History) -> bool {
        self.end(history);
        let Some(s) = history.undo.pop() else { return false };
        let inv = restore(self, s);
        history.redo.push(inv);
        self.dirty = true;
        true
    }

    pub fn redo(&mut self, history: &mut History) -> bool {
        self.end(history);
        let Some(s) = history.redo.pop() else { return false };
        let inv = restore(self, s);
        history.undo.push(inv);
        self.dirty = true;
        true
    }

    // ------------------------------------------------------------------------------ geometry

    /// Move a drawable by (dx, dy) points.
    pub fn move_by(&mut self, h: &mut History, id: ObjId, dx: f32, dy: f32) {
        self.touch(h, id);
        let Some(d) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) else { return };
        let pos = d.msg_mut_or_create("geometry").msg_mut_or_create("position");
        let x = pos.f32("x").unwrap_or(0.0);
        let y = pos.f32("y").unwrap_or(0.0);
        pos.set_f32("x", x + dx);
        pos.set_f32("y", y + dy);
    }

    /// Set a drawable's frame. Fixes the size (clears auto-size) when the size changes, and keeps
    /// parametric path sources in step so arrows/stars/rounded corners don't stretch.
    pub fn set_frame(&mut self, h: &mut History, id: ObjId, g: Geometry) {
        self.touch(h, id);
        let Some(m) = self.file.obj_mut(id) else { return };
        // Keynote draws shapes from their path source, so the path itself must be resized
        // (geometry size alone is ignored) and flips live on the path source.
        let mut is_shape = false;
        if let Some(shape) = m.find_super_mut("TSD.ShapeArchive") {
            if let Some(ps) = shape.msg_mut("pathsource") {
                is_shape = true;
                resize_path_source(ps, g.w, g.h);
                ps.set_bool("horizontalFlip", g.hflip);
                ps.set_bool("verticalFlip", g.vflip);
            }
        }
        let Some(d) = m.find_super_mut("TSD.DrawableArchive") else { return };
        let geo = d.msg_mut_or_create("geometry");
        {
            let pos = geo.msg_mut_or_create("position");
            pos.set_f32("x", g.x);
            pos.set_f32("y", g.y);
        }
        {
            let size = geo.msg_mut_or_create("size");
            size.set_f32("width", g.w);
            size.set_f32("height", g.h);
        }
        geo.set_f32("angle", g.angle.rem_euclid(360.0));
        // bits 1/2 = width/height fixed; auto-sizing text boxes keep growing with their text
        let mut flags = geo.u32("flags").unwrap_or(3) & !3;
        if !g.auto_width {
            flags |= 1;
        }
        if !g.auto_height {
            flags |= 2;
        }
        flags &= !12;
        if !is_shape {
            flags |= if g.hflip { 4 } else { 0 } | if g.vflip { 8 } else { 0 };
        }
        geo.set_u64("flags", u64::from(flags));
    }

    /// Object hyperlink (Format > Add Link); None removes it.
    pub fn set_hyperlink(&mut self, h: &mut History, id: ObjId, url: Option<&str>) {
        self.touch(h, id);
        if let Some(d) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
            match url {
                Some(u) => d.set_str("hyperlink_url", u),
                None => d.remove("hyperlink_url"),
            }
        }
    }

    pub fn set_locked(&mut self, h: &mut History, id: ObjId, locked: bool) {
        self.touch(h, id);
        if let Some(d) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
            d.set_bool("locked", locked);
        }
    }

    // ------------------------------------------------------------------------------ text

    /// Replace the whole text of a storage. Attribute tables keep their entries; indices past
    /// the new end are clamped (the approach proven by the research writer).
    pub fn set_storage_text(&mut self, h: &mut History, storage: ObjId, text: &str) {
        self.touch(h, storage);
        let Some(st) = self.file.obj_mut(storage) else { return };
        st.set_strs("text", &[text]);
        let n = text.encode_utf16().count() as u64;
        for tbl in [
            "table_para_style",
            "table_char_style",
            "table_list_style",
            "table_para_data",
            "table_para_starts",
            "table_para_bidi",
            "table_drop_cap_style",
            "table_language",
            "table_smartfield",
            "table_attachment",
        ] {
            let Some(t) = st.msg_mut(tbl) else { continue };
            for e in t.msgs_mut("entries") {
                if e.u64("character_index").unwrap_or(0) > n {
                    e.set_u64("character_index", n);
                }
            }
            // clamping can create several entries at one index: keep the last of each
            let idx: Vec<u64> = t.msgs("entries").map(|e| e.u64("character_index").unwrap_or(0)).collect();
            let keep: Vec<bool> = (0..idx.len()).map(|i| !idx[i + 1..].contains(&idx[i])).collect();
            let mut i = 0;
            t.retain_msgs("entries", |_| {
                let k = keep[i];
                i += 1;
                k
            });
        }
    }

    /// Set presenter notes of a slide (slides always carry a note storage).
    pub fn set_notes(&mut self, h: &mut History, slide: ObjId, text: &str) -> bool {
        let Some(st) = self.notes_storage(slide) else { return false };
        self.set_text_by_diff(h, st, text);
        if let Some(node) = self.slides().into_iter().find(|s| s.slide == slide).map(|s| s.node) {
            self.touch(h, node);
            if let Some(n) = self.file.obj_mut(node) {
                n.set_bool("hasNote", !text.is_empty());
            }
        }
        true
    }

    // ------------------------------------------------------------------------------ slides

    pub fn set_skipped(&mut self, h: &mut History, node: ObjId, skipped: bool) {
        self.touch(h, node);
        if let Some(n) = self.file.obj_mut(node) {
            n.set_bool("isSkipped", skipped);
        }
    }

    /// Set (or clear with effect "none") a slide's transition.
    pub fn set_transition(&mut self, h: &mut History, slide: ObjId, effect: &str, duration: f64, delay: f64, automatic: bool) {
        self.touch(h, slide);
        let Some(s) = self.file.obj_mut(slide) else { return };
        let aa = s
            .msg_mut_or_create("transition")
            .msg_mut_or_create("attributes")
            .msg_mut_or_create("animationAttributes");
        aa.set_str("animation_type", "Transition");
        aa.set_str("effect", effect);
        aa.set_f64("duration", duration);
        aa.set_f64("delay", delay);
        aa.set_bool("is_automatic", automatic);
        let has = effect != "none";
        if let Some(node) = self.slides().into_iter().find(|i| i.slide == slide).map(|i| i.node) {
            self.touch(h, node);
            if let Some(n) = self.file.obj_mut(node) {
                n.set_bool("hasTransition", has);
            }
        }
    }
}

/// TSP.Path of a w x h rectangle, written the way Keynote writes it.
fn rect_path(w: f32, h: f32) -> kn_format::Msg {
    let mut path = kn_format::Msg::new("TSP.Path");
    for (t, pt) in [("moveTo", Some((0.0, 0.0))), ("lineTo", Some((w, 0.0))), ("lineTo", Some((w, h))), ("lineTo", Some((0.0, h))), ("closeSubpath", None), ("moveTo", Some((0.0, 0.0)))] {
        let mut el = kn_format::Msg::new("TSP.Path.Element");
        el.set_enum("type", t);
        if let Some((x, y)) = pt {
            let mut p = kn_format::Msg::new("TSP.Point");
            p.set_f32("x", x);
            p.set_f32("y", y);
            el.push_msg("points", p);
        }
        path.push_msg("elements", el);
    }
    path
}

/// Scale a TSP.Path's points.
fn scale_tsp_path(path: &mut kn_format::Msg, sx: f32, sy: f32) {
    for el in path.msgs_mut("elements") {
        for p in el.msgs_mut("points") {
            let (x, y) = (p.f32("x").unwrap_or(0.0), p.f32("y").unwrap_or(0.0));
            p.set_f32("x", x * sx);
            p.set_f32("y", y * sy);
        }
    }
}

fn set_size(m: &mut kn_format::Msg, field: &str, w: f32, h: f32) -> (f32, f32) {
    let ns = m.msg_mut_or_create(field);
    let old = (ns.f32("width").unwrap_or(0.0), ns.f32("height").unwrap_or(0.0));
    ns.set_f32("width", w);
    ns.set_f32("height", h);
    old
}

fn ratio(new: f32, old: f32) -> f32 {
    if old.abs() > 1e-3 {
        new / old
    } else {
        1.0
    }
}

/// Resize every kind of path source to `w` x `h` (natural size and the path points).
/// Format > Image > Mask with Shape: give an image's mask the outline of a basic shape (sized to
/// the mask's current frame). Rectangle restores a plain mask.
impl Document {
    pub fn mask_with_shape(&mut self, h: &mut History, image: ObjId, shape: crate::insert::BasicShape) -> bool {
        let mask = match self.file.obj(image).and_then(|m| m.ref_id("mask")) {
            Some(m) => m,
            None => match self.add_image_mask(h, image) {
                Some(m) => m,
                None => return false,
            },
        };
        let Some((w, hh)) = self.file.obj(mask).and_then(|m| m.find_super("TSD.DrawableArchive")).map(|d| {
            let g = crate::scene::geometry(d);
            (g.w, g.h)
        }) else {
            return false;
        };
        let mut ps = shape.path_source();
        resize_path_source(&mut ps, w, hh);
        self.touch(h, mask);
        let Some(m) = self.file.obj_mut(mask) else { return false };
        m.remove("pathsource");
        m.push_msg("pathsource", ps);
        true
    }
}

impl Document {
    /// A rectangular mask showing only (x, y, w, h) of the picture, in the picture's own frame
    /// (Format > Image > Auto Crop).
    pub fn set_mask_rect(&mut self, h: &mut History, image: ObjId, x: f32, y: f32, w: f32, hh: f32) -> bool {
        let mask = match self.file.obj(image).and_then(|m| m.ref_id("mask")) {
            Some(m) => m,
            None => match self.add_image_mask(h, image) {
                Some(m) => m,
                None => return false,
            },
        };
        self.touch(h, mask);
        let Some(m) = self.file.obj_mut(mask) else { return false };
        if let Some(g) = m.find_super_mut("TSD.DrawableArchive").and_then(|d| d.msg_mut("geometry")) {
            if let Some(p) = g.msg_mut("position") {
                p.set_f32("x", x);
                p.set_f32("y", y);
            }
            if let Some(sz) = g.msg_mut("size") {
                sz.set_f32("width", w.max(1.0));
                sz.set_f32("height", hh.max(1.0));
            }
        }
        let mut ps = crate::insert::BasicShape::Rectangle.path_source();
        resize_path_source(&mut ps, w.max(1.0), hh.max(1.0));
        m.remove("pathsource");
        m.push_msg("pathsource", ps);
        true
    }

    /// A rectangular mask covering the whole image (as on Keynote's layout photos: a
    /// TSD.MaskArchive in the image's file, geometry in the image's own frame, parent = image).
    fn add_image_mask(&mut self, h: &mut History, image: ObjId) -> Option<ObjId> {
        let file = self.file.file_of(image)?.to_string();
        let g = crate::scene::geometry(self.file.obj(image)?.find_super("TSD.DrawableArchive")?);
        self.touch_all(h);
        let id = self.file.alloc_ids(1)[0];
        let mut geo = kn_format::Msg::new("TSD.GeometryArchive");
        let mut pos = kn_format::Msg::new("TSP.Point");
        pos.set_f32("x", 0.0);
        pos.set_f32("y", 0.0);
        geo.push_msg("position", pos);
        let mut size = kn_format::Msg::new("TSP.Size");
        size.set_f32("width", g.w);
        size.set_f32("height", g.h);
        geo.push_msg("size", size);
        geo.set_u64("flags", 3);
        geo.set_f32("angle", 0.0);
        let mut d = kn_format::Msg::new("TSD.DrawableArchive");
        d.push_msg("geometry", geo);
        d.set_ref("parent", image);
        // the defaults Keynote fills in when it saves a mask (Mac round R38)
        let mut wrap = kn_format::Msg::new("TSD.ExteriorTextWrapArchive");
        wrap.set_u64("type", 4);
        wrap.set_u64("direction", 2);
        wrap.set_u64("fit_type", 1);
        wrap.set_f32("margin", 0.0);
        wrap.set_f32("alpha_threshold", 0.5);
        wrap.set_bool("is_html_wrap", false);
        d.push_msg("exterior_text_wrap", wrap);
        for f in ["aspect_ratio_locked", "caption_hidden", "title_hidden", "locked"] {
            d.set_bool(f, false);
        }
        let mut m = kn_format::Msg::new("TSD.MaskArchive");
        m.push_msg("super", d);
        let mut ps = crate::insert::BasicShape::Rectangle.path_source();
        resize_path_source(&mut ps, g.w, g.h);
        m.push_msg("pathsource", ps);
        let mut a = kn_format::Archive::new_object(id, m);
        a.refresh_object_references();
        self.file.insert_archive(&file, a);
        let ia = self.file.archive_mut(image)?;
        ia.obj_mut().set_ref("mask", id);
        ia.refresh_object_references();
        Some(id)
    }
}

/// Keynote's green shape handle: the one adjustable value of an arrow (head size), star (inner
/// radius) or rounded rectangle (corner radius), dragged on the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeControl {
    /// Handle position in the shape's frame (0..w, 0..h).
    pub pos: crate::Point,
}

fn natural(ps: &kn_format::Msg, src: &str, w: f32, h: f32) -> (f32, f32) {
    let ns = ps.msg(src).and_then(|s| s.msg("naturalSize"));
    (ns.and_then(|n| n.f32("width")).unwrap_or(w).max(1e-3), ns.and_then(|n| n.f32("height")).unwrap_or(h).max(1e-3))
}

/// Where the control handle of a path source sits, in a `w` × `h` frame.
fn control_pos(ps: &kn_format::Msg, w: f32, h: f32) -> Option<crate::Point> {
    if let Some(p) = ps.msg("point_path_source") {
        let (nw, nh) = natural(ps, "point_path_source", w, h);
        let pt = p.msg("point")?;
        let (px, py) = (pt.f32("x").unwrap_or(0.0), pt.f32("y").unwrap_or(0.0));
        let (sx, sy) = (w / nw, h / nh);
        return match p.enum_name("type")? {
            "kTSDRightSingleArrow" => Some(crate::Point::new((nw - px.clamp(0.0, nw)) * sx, py.clamp(0.0, 0.5) * nh * sy)),
            "kTSDLeftSingleArrow" | "kTSDDoubleArrow" => Some(crate::Point::new(px.clamp(0.0, nw) * sx, py.clamp(0.0, 0.5) * nh * sy)),
            "kTSDStar" => {
                // the first inner corner of the star as drawn
                let path = crate::path::from_path_source(ps, w, h);
                match path.els.get(1)? {
                    crate::PathEl::Line(q) => Some(*q),
                    _ => None,
                }
            }
            _ => None,
        };
    }
    if let Some(s) = ps.msg("scalar_path_source") {
        if s.enum_name("type") == Some("kTSDRoundedRectangle") || s.enum_name("type").is_none() {
            let r = s.f32("scalar").unwrap_or(0.0).clamp(0.0, w.min(h) / 2.0);
            return Some(crate::Point::new(r, 0.0));
        }
    }
    None
}

/// Set a path source's adjustable value from a handle dragged to `q` (frame coordinates).
fn control_update(ps: &mut kn_format::Msg, w: f32, h: f32, q: crate::Point) -> bool {
    if ps.msg("point_path_source").is_some() {
        let (nw, nh) = natural(ps, "point_path_source", w, h);
        let p = ps.msg_mut("point_path_source").unwrap();
        let (qx, qy) = (q.x * nw / w.max(1e-3), q.y * nh / h.max(1e-3));
        let ty = p.enum_name("type").unwrap_or("").to_string();
        let pt = p.msg_mut_or_create("point");
        match ty.as_str() {
            "kTSDRightSingleArrow" => {
                pt.set_f32("x", (nw - qx).clamp(0.0, nw));
                pt.set_f32("y", (qy / nh).clamp(0.0, 0.5));
            }
            "kTSDLeftSingleArrow" => {
                pt.set_f32("x", qx.clamp(0.0, nw));
                pt.set_f32("y", (qy / nh).clamp(0.0, 0.5));
            }
            "kTSDDoubleArrow" => {
                pt.set_f32("x", qx.clamp(0.0, nw / 2.0));
                pt.set_f32("y", (qy / nh).clamp(0.0, 0.5));
            }
            "kTSDStar" => {
                let (dx, dy) = ((q.x - w / 2.0) / (w / 2.0).max(1e-3), (q.y - h / 2.0) / (h / 2.0).max(1e-3));
                pt.set_f32("y", (dx * dx + dy * dy).sqrt().clamp(0.05, 1.0));
            }
            _ => return false,
        }
        return true;
    }
    if let Some(s) = ps.msg_mut("scalar_path_source") {
        if s.enum_name("type") == Some("kTSDRoundedRectangle") || s.enum_name("type").is_none() {
            s.set_f32("scalar", q.x.clamp(0.0, w.min(h) / 2.0));
            return true;
        }
    }
    false
}

impl Document {
    fn shape_path_source(&self, id: ObjId) -> Option<&kn_format::Msg> {
        self.file.obj(id)?.find_super("TSD.ShapeArchive")?.msg("pathsource")
    }

    /// The shape's green control handle, if it has one (frame `w` × `h`).
    pub fn shape_control(&self, id: ObjId, w: f32, h: f32) -> Option<ShapeControl> {
        control_pos(self.shape_path_source(id)?, w, h).map(|pos| ShapeControl { pos })
    }

    /// The outline with the control handle dragged to `q` (live preview).
    pub fn shape_path_with_control(&self, id: ObjId, w: f32, h: f32, q: crate::Point) -> Option<crate::Path> {
        let mut ps = self.shape_path_source(id)?.clone();
        control_update(&mut ps, w, h, q).then(|| crate::path::from_path_source(&ps, w, h))
    }

    /// Drop the control handle at `q`: the arrow head / star radius / corner radius changes.
    pub fn set_shape_control(&mut self, h: &mut History, id: ObjId, w: f32, hh: f32, q: crate::Point) -> bool {
        self.touch(h, id);
        let Some(ps) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.ShapeArchive")).and_then(|s| s.msg_mut("pathsource")) else { return false };
        control_update(ps, w, hh, q)
    }

    /// Move a straight line's end points (slide coordinates), as Keynote's line handles do: the
    /// frame becomes the box around the two points (unrotated, unflipped) and the path runs
    /// from one to the other inside it. Only for plain two-point lines.
    pub fn set_line_points(&mut self, h: &mut History, id: ObjId, a: crate::Point, b: crate::Point) -> bool {
        let is_two_point = self
            .file
            .obj(id)
            .and_then(|m| m.find_super("TSD.ShapeArchive"))
            .and_then(|s| s.msg("pathsource"))
            .and_then(|p| p.msg("bezier_path_source"))
            .and_then(|b| b.msg("path"))
            .is_some_and(|p| p.count("elements") == 2);
        if !is_two_point {
            return false;
        }
        let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
        let (w, hh) = ((a.x - b.x).abs(), (a.y - b.y).abs());
        self.touch(h, id);
        let Some(m) = self.file.obj_mut(id) else { return false };
        if let Some(d) = m.find_super_mut("TSD.DrawableArchive") {
            let g = d.msg_mut_or_create("geometry");
            let pos = g.msg_mut_or_create("position");
            pos.set_f32("x", x0);
            pos.set_f32("y", y0);
            let size = g.msg_mut_or_create("size");
            size.set_f32("width", w);
            size.set_f32("height", hh);
            g.set_f32("angle", 0.0);
        }
        let Some(ps) = m.find_super_mut("TSD.ShapeArchive").and_then(|s| s.msg_mut("pathsource")) else { return false };
        ps.set_bool("horizontalFlip", false);
        ps.set_bool("verticalFlip", false);
        let Some(bp) = ps.msg_mut("bezier_path_source") else { return false };
        let ns = bp.msg_mut_or_create("naturalSize");
        ns.set_f32("width", w);
        ns.set_f32("height", hh);
        let mut path = kn_format::Msg::new("TSP.Path");
        for (t, (x, y)) in [("moveTo", (a.x - x0, a.y - y0)), ("lineTo", (b.x - x0, b.y - y0))] {
            let mut el = kn_format::Msg::new("TSP.Path.Element");
            el.set_enum("type", t);
            let mut p = kn_format::Msg::new("TSP.Point");
            p.set_f32("x", x);
            p.set_f32("y", y);
            el.push_msg("points", p);
            path.push_msg("elements", el);
        }
        bp.remove("path");
        bp.push_msg("path", path);
        true
    }

    /// A shape's outline as it will be at size `w` × `h` (live resize preview: the same path
    /// source scaling a finished resize stores).
    pub fn shape_path_at(&self, id: ObjId, w: f32, h: f32) -> Option<crate::Path> {
        let m = self.file.obj(id)?;
        let ps = m.find_super("TSD.ShapeArchive")?.msg("pathsource")?;
        let mut ps = ps.clone();
        resize_path_source(&mut ps, w, h);
        Some(crate::path::from_path_source(&ps, w, h))
    }
}

pub(crate) fn resize_path_source(ps: &mut kn_format::Msg, w: f32, h: f32) {
    if let Some(b) = ps.msg_mut("bezier_path_source") {
        let (ow, oh) = set_size(b, "naturalSize", w, h);
        let closed = b.msg("path").is_some_and(|p| p.msgs("elements").any(|e| e.enum_name("type") == Some("closeSubpath")));
        if closed && (ow <= 1e-3 || oh <= 1e-3) {
            // a closed path with no usable size (e.g. AppleScript-made text boxes): rebuild it as
            // the rectangle Keynote uses for text boxes
            b.remove("path");
            b.push_msg("path", rect_path(w, h));
        } else if let Some(p) = b.msg_mut("path") {
            scale_tsp_path(p, ratio(w, ow), ratio(h, oh));
        }
    }
    if let Some(c) = ps.msg_mut("connection_line_path_source").and_then(|c| c.msg_mut("super")) {
        let (ow, oh) = set_size(c, "naturalSize", w, h);
        if let Some(p) = c.msg_mut("path") {
            scale_tsp_path(p, ratio(w, ow), ratio(h, oh));
        }
    }
    for src in ["point_path_source", "scalar_path_source"] {
        if let Some(s) = ps.msg_mut(src) {
            set_size(s, "naturalSize", w, h);
        }
    }
    if let Some(c) = ps.msg_mut("callout_path_source") {
        let (ow, oh) = set_size(c, "natural_size", w, h);
        if let Some(t) = c.msg_mut("tail_position") {
            let (x, y) = (t.f32("x").unwrap_or(0.0), t.f32("y").unwrap_or(0.0));
            t.set_f32("x", x * ratio(w, ow));
            t.set_f32("y", y * ratio(h, oh));
        }
    }
    if let Some(e) = ps.msg_mut("editable_bezier_path_source") {
        let (ow, oh) = set_size(e, "naturalSize", w, h);
        let (sx, sy) = (ratio(w, ow), ratio(h, oh));
        for sp in e.msgs_mut("subpaths") {
            for n in sp.msgs_mut("nodes") {
                for f in ["inControlPoint", "nodePoint", "outControlPoint"] {
                    if let Some(p) = n.msg_mut(f) {
                        let (x, y) = (p.f32("x").unwrap_or(0.0), p.f32("y").unwrap_or(0.0));
                        p.set_f32("x", x * sx);
                        p.set_f32("y", y * sy);
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZMove {
    Front,
    Back,
    Forward,
    Backward,
}

impl Document {
    /// Change the stacking order of drawables on a slide.
    pub fn reorder(&mut self, h: &mut History, slide: ObjId, ids: &[ObjId], how: ZMove) {
        self.touch(h, slide);
        let Some(s) = self.file.obj_mut(slide) else { return };
        let mut z = s.ref_ids("drawables_z_order");
        if z.is_empty() {
            return;
        }
        let moving: Vec<ObjId> = z.iter().copied().filter(|i| ids.contains(i)).collect();
        match how {
            ZMove::Front => {
                z.retain(|i| !ids.contains(i));
                z.extend(&moving);
            }
            ZMove::Back => {
                z.retain(|i| !ids.contains(i));
                z.splice(0..0, moving.iter().copied());
            }
            ZMove::Forward => {
                for i in (0..z.len().saturating_sub(1)).rev() {
                    if ids.contains(&z[i]) && !ids.contains(&z[i + 1]) {
                        z.swap(i, i + 1);
                    }
                }
            }
            ZMove::Backward => {
                for i in 1..z.len() {
                    if ids.contains(&z[i]) && !ids.contains(&z[i - 1]) {
                        z.swap(i, i - 1);
                    }
                }
            }
        }
        s.remove("drawables_z_order");
        for id in z {
            s.push_ref("drawables_z_order", id);
        }
    }

    /// Remove drawables from a slide. Placeholders are hidden (as Keynote does); other objects
    /// are detached from the slide (their archives stay until Keynote's next save cleans up).
    /// The saved canvas selection (view state) forgets objects being deleted: a selection
    /// that lists any of them is emptied, as Keynote saves when nothing is selected (else the
    /// file would point at objects that are gone).
    fn forget_selection_of(&mut self, h: &mut History, ids: &[ObjId]) {
        const FIELDS: [&str; 4] = ["infos", "non_interactive_infos", "infos_for_action_ghost_indexes", "build_chunks"];
        let sels: Vec<ObjId> = self
            .file
            .archives()
            .map(|(_, a)| a.id())
            .filter(|i| matches!(self.file.type_name(*i), "KN.CanvasSelectionArchive" | "TSD.CanvasSelectionArchive"))
            .collect();
        for sel in sels {
            let hit = self.file.obj(sel).is_some_and(|m| FIELDS.iter().any(|f| m.has(f) && m.ref_ids(f).iter().any(|r| ids.contains(r))));
            if !hit {
                continue;
            }
            self.touch(h, sel);
            if let Some(a) = self.file.archive_mut(sel) {
                let m = a.obj_mut();
                for f in FIELDS.iter().chain(["action_ghost_indexes"].iter()) {
                    if m.has(f) {
                        m.remove(f);
                    }
                }
                a.refresh_object_references();
            }
        }
    }

    pub fn delete_drawables(&mut self, h: &mut History, slide: ObjId, ids: &[ObjId]) {
        // a deleted table leaves the calculation engine too (its records are archives that
        // go: the whole document is kept for undo)
        let tables: Vec<ObjId> = ids.iter().copied().filter(|i| self.file.type_name(*i) == "TST.TableInfoArchive").collect();
        if !tables.is_empty() {
            self.touch_all(h);
            for t in &tables {
                self.file.remove_table_engine(*t);
            }
        }
        self.touch(h, slide);
        self.forget_selection_of(h, ids);
        // builds (and their build-order chunks) that animate the removed objects go too
        let Some(sm) = self.file.obj(slide) else { return };
        let dead_builds: Vec<ObjId> = sm
            .ref_ids("builds")
            .into_iter()
            .filter(|b| self.file.obj(*b).and_then(|m| m.ref_id("drawable")).is_some_and(|d| ids.contains(&d)))
            .collect();
        let dead_chunks: Vec<ObjId> = sm
            .ref_ids("buildChunks")
            .into_iter()
            .filter(|c| self.file.obj(*c).and_then(|m| m.ref_id("build")).is_some_and(|b| dead_builds.contains(&b)))
            .collect();
        let Some(a) = self.file.archive_mut(slide) else { return };
        let s = &mut a.payloads[0];
        let gone = |r: &kn_format::Msg, list: &[ObjId]| list.contains(&r.u64("identifier").unwrap_or(0));
        s.retain_msgs("drawables_z_order", |r| !gone(r, ids));
        s.retain_msgs("owned_drawables", |r| !gone(r, ids));
        s.retain_msgs("builds", |r| !gone(r, &dead_builds));
        s.retain_msgs("buildChunks", |r| !gone(r, &dead_chunks));
        a.refresh_object_references();
    }

    /// Slide > Collapse / Expand: Keynote stores collapsed slide groups as view state, the
    /// parent nodes listed in KN.UIStateArchive.collapsed_slide_nodes (Mac, round 9).
    pub fn set_collapsed(&mut self, h: &mut History, node: ObjId, collapsed: bool) {
        let Some((ui, _)) = self.file.first_of("KN.UIStateArchive") else { return };
        self.touch(h, ui);
        if let Some(a) = self.file.archive_mut(ui) {
            let m = a.obj_mut();
            m.retain_msgs("collapsed_slide_nodes", |r| r.u64("identifier") != Some(node));
            if collapsed {
                m.push_ref("collapsed_slide_nodes", node);
            }
            a.refresh_object_references();
        }
        if let Some(f) = self.file.file_of(ui).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
    }

    /// Indent a slide (Tab in the navigator) or outdent it: its node's `depth` (1 = top level).
    /// A slide can be at most one level deeper than the slide above it (Keynote).
    pub fn set_slide_depth(&mut self, h: &mut History, index: usize, depth: u32) -> bool {
        let slides = self.slides();
        let Some(info) = slides.get(index) else { return false };
        let max = if index == 0 { 0 } else { slides[index - 1].depth + 1 };
        let d = depth.min(max);
        if d == info.depth {
            return false;
        }
        self.touch(h, info.node);
        if let Some(n) = self.file.obj_mut(info.node) {
            n.set_u64("depth", u64::from(d) + 1);
        }
        true
    }

    /// Edit > Paste of copied slides: copies of `slides` (slide, node) from `src` — a snapshot of
    /// this same document — inserted from position `at`. Returns the first new index.
    pub fn paste_slides(&mut self, h: &mut History, src: &kn_format::KeyFile, slides: &[(ObjId, ObjId)], at: usize) -> Result<usize, kn_format::Error> {
        self.touch_all(h);
        for (k, (slide, node)) in slides.iter().enumerate() {
            self.file.clone_slide_between(Some(src), *slide, *node, at + k)?;
        }
        Ok(at)
    }

    /// Duplicate slide `index` (top level) right after it. Returns the new slide's index.
    pub fn duplicate_slide(&mut self, h: &mut History, index: usize) -> Result<usize, kn_format::Error> {
        self.touch_all(h);
        self.file.clone_slide(index, index + 1)?;
        // a slide's tables live outside its own file (the calculation engine's): the copy
        // pointed at the same tables, so editing one slide's table changed both. Each gets its
        // own copy, in the same place in the stacking order (found by the deck stress test)
        let slides = self.slides();
        let Some(new_slide) = slides.get(index + 1).map(|i| i.slide) else { return Ok(index + 1) };
        let slide_file = self.file.file_of(new_slide).map(str::to_string);
        let tables: Vec<ObjId> = self
            .file
            .obj(new_slide)
            .map(|m| m.ref_ids("owned_drawables"))
            .unwrap_or_default()
            .into_iter()
            .filter(|d| self.file.type_name(*d) == "TST.TableInfoArchive" && self.file.file_of(*d).map(str::to_string) != slide_file)
            .collect();
        if !tables.is_empty() {
            let donor = self.file.clone();
            for t in tables {
                let nt = self.file.paste_table(&donor, t, new_slide)?;
                // the copy was added at the end: put it where the original was
                if let Some(a) = self.file.archive_mut(new_slide) {
                    let m = a.obj_mut();
                    for field in ["drawables_z_order", "owned_drawables"] {
                        m.retain_msgs(field, |r| r.u64("identifier") != Some(nt));
                    }
                }
                if let Some(f) = slide_file.clone() {
                    let ids: Vec<ObjId> = self.file.archives().filter(|(n, _)| *n == f).map(|(_, a)| a.id()).collect();
                    for id in ids {
                        if let Some(a) = self.file.archive_mut(id) {
                            for p in &mut a.payloads {
                                p.for_each_ref_mut(&mut |r| {
                                    if *r == t {
                                        *r = nt;
                                    }
                                });
                            }
                            a.refresh_object_references();
                        }
                    }
                }
            }
            self.file.reindex();
        }
        Ok(index + 1)
    }

    pub fn delete_slides(&mut self, h: &mut History, indices: &[usize]) -> Result<(), kn_format::Error> {
        self.touch_all(h);
        self.file.delete_slides(indices)
    }

    pub fn move_slide(&mut self, h: &mut History, from: usize, to: usize) -> Result<(), kn_format::Error> {
        self.touch_all(h);
        self.file.move_slide(from, to)
    }
}

#[cfg(test)]
mod mask_tests {
    use super::*;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn mask_image_with_oval() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/04-image.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let img = d.scene(&s, 0).unwrap().drawables.into_iter().find(|x| matches!(x.kind, crate::DrawableKind::Image(_)) && !x.from_template).unwrap().id;
        let mut h = History::default();
        d.begin(&mut h, "Mask");
        assert!(d.mask_with_shape(&mut h, img, crate::insert::BasicShape::Oval));
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let sc = d2.scene(&d2.slides(), 0).unwrap();
        let crate::DrawableKind::Image(im) = &sc.drawables.iter().find(|x| x.id == img).unwrap().kind else { panic!() };
        let (_, path) = im.mask.as_ref().unwrap();
        assert!(path.els.iter().any(|e| matches!(e, crate::PathEl::Cubic(..))), "oval mask is curved");
    }
}

#[cfg(test)]
mod line_tests {
    use super::*;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn green_handles_change_arrow_head_and_corner_radius() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let sc = d.scene(&s, 120).unwrap();
        let mut h = History::default();
        let mut found = 0;
        for x in sc.drawables.iter().filter(|x| !x.from_template) {
            let Some(c) = d.shape_control(x.id, x.geom.w, x.geom.h) else { continue };
            found += 1;
            // drag the handle a little and check the shape follows (and saves)
            let q = crate::Point::new(c.pos.x * 0.8 + 2.0, c.pos.y * 0.8 + 2.0);
            d.begin(&mut h, "Control");
            assert!(d.set_shape_control(&mut h, x.id, x.geom.w, x.geom.h, q));
            d.end(&mut h);
            let c2 = d.shape_control(x.id, x.geom.w, x.geom.h).unwrap();
            assert!((c2.pos.x - c.pos.x).abs() + (c2.pos.y - c.pos.y).abs() > 0.5, "handle moved");
        }
        assert!(found >= 4, "rounded rect, arrows, star have handles: {found}");
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn move_line_end_points() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/03-line.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let line = d.scene(&s, 0).unwrap().drawables.into_iter().find(|x| matches!(&x.kind, crate::DrawableKind::Shape(sh) if sh.is_line)).unwrap().id;
        let mut h = History::default();
        d.begin(&mut h, "Line");
        // drawn right to left and upwards
        assert!(d.set_line_points(&mut h, line, crate::Point::new(500.0, 300.0), crate::Point::new(100.0, 100.0)));
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let sc = d2.scene(&d2.slides(), 0).unwrap();
        let x = sc.drawables.iter().find(|x| x.id == line).unwrap();
        assert_eq!((x.geom.x, x.geom.y, x.geom.w, x.geom.h), (100.0, 100.0, 400.0, 200.0));
        let crate::DrawableKind::Shape(sh) = &x.kind else { panic!() };
        let pts: Vec<_> = sh.path.els.iter().filter_map(|e| match e { crate::PathEl::Move(p) | crate::PathEl::Line(p) => Some((p.x, p.y)), _ => None }).collect();
        assert_eq!(pts, vec![(400.0, 200.0), (0.0, 0.0)]);
    }
}

#[cfg(test)]
mod indent_tests {
    use super::*;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn reads_and_writes_keynote_slide_groups() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R9-indented-collapsed.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        assert_eq!(s.iter().map(|x| x.depth).collect::<Vec<_>>(), vec![0, 1, 1, 2, 0, 1]);
        assert_eq!(s.iter().map(|x| x.collapsed).collect::<Vec<_>>(), vec![false, false, false, false, true, false]);
        let mut h = History::default();
        d.begin(&mut h, "Indent");
        // the slide above is at depth 2: asking for 5 gives 3 (one deeper, no more)
        assert!(d.set_slide_depth(&mut h, 4, 5));
        assert_eq!(d.slides()[4].depth, 3);
        d.set_collapsed(&mut h, s[4].node, false);
        d.set_collapsed(&mut h, s[0].node, true);
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let s2 = d2.slides();
        assert!(s2[0].collapsed && !s2[4].collapsed);
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    /// Deleting an object that the saved canvas selection lists empties that selection (no
    /// reference left to a deleted object); undo brings it back.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn delete_forgets_saved_selection() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/r43/R43-effects.key");
        let Ok(mut d) = Document::open(&p) else { return };
        let slides = d.slides();
        let slide = slides[0].slide;
        let text_box = 33250;
        let sel = d.file.archives().map(|(_, a)| a.id()).find(|i| d.file.type_name(*i) == "KN.CanvasSelectionArchive").unwrap();
        // as Keynote saves it with the text box selected
        if let Some(a) = d.file.archive_mut(sel) {
            a.obj_mut().push_ref("infos", text_box);
            a.refresh_object_references();
        }
        let mut h = History::default();
        d.begin(&mut h, "Delete");
        d.delete_drawables(&mut h, slide, &[text_box]);
        d.end(&mut h);
        assert!(d.file.obj(sel).unwrap().ref_ids("infos").is_empty());
        assert!(d.undo(&mut h));
        assert_eq!(d.file.obj(sel).unwrap().ref_ids("infos"), vec![text_box]);
    }
}
