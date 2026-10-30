//! Format > Image: adjustments (TSD.ImageAdjustmentsArchive + a pre-rendered adjustedImageData,
//! as Keynote stores them), Replace, Reset Mask, Description.

use kn_format::{Msg, ObjId};

use crate::edit::History;
use crate::Document;

/// Keynote's image adjustments. Sliders run -1..1 (0 = unchanged); levels 0..1; gamma 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adjustments {
    pub exposure: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub sharpness: f32,
    pub denoise: f32,
    pub temperature: f32,
    pub tint: f32,
    pub bottom_level: f32,
    pub top_level: f32,
    pub gamma: f32,
    pub enhance: bool,
}

impl Default for Adjustments {
    fn default() -> Self {
        Adjustments { exposure: 0.0, saturation: 0.0, contrast: 0.0, highlights: 0.0, shadows: 0.0, sharpness: 0.0, denoise: 0.0, temperature: 0.0, tint: 0.0, bottom_level: 0.0, top_level: 1.0, gamma: 1.0, enhance: false }
    }
}

impl Adjustments {
    pub fn is_default(&self) -> bool {
        *self == Adjustments::default()
    }
}

fn data_ref(id: u64) -> Msg {
    let mut m = Msg::new("TSP.DataReference");
    m.set_u64("identifier", id);
    m
}

impl Document {
    pub fn image_adjustments(&self, image: ObjId) -> Adjustments {
        let mut a = Adjustments::default();
        let Some(m) = self.file.obj(image).and_then(|m| m.msg("imageAdjustments")) else { return a };
        let f = |n: &str, d: f32| m.f32(n).unwrap_or(d);
        a.exposure = f("exposure", 0.0);
        a.saturation = f("saturation", 0.0);
        a.contrast = f("contrast", 0.0);
        a.highlights = f("highlights", 0.0);
        a.shadows = f("shadows", 0.0);
        a.sharpness = f("sharpness", 0.0);
        a.denoise = f("denoise", 0.0);
        a.temperature = f("temperature", 0.0);
        a.tint = f("tint", 0.0);
        a.bottom_level = f("bottom_level", 0.0);
        a.top_level = f("top_level", 1.0);
        a.gamma = f("gamma", 1.0);
        a.enhance = m.bool("enhance").unwrap_or(false);
        a
    }

    /// The image's own picture (before adjustments): data id.
    pub fn image_original_data(&self, image: ObjId) -> Option<u64> {
        self.file.obj(image)?.msg("data")?.u64("identifier")
    }

    /// Store adjustments and the picture rendered with them (`adjusted`: bytes + file name).
    pub fn set_image_adjustments(&mut self, h: &mut History, image: ObjId, adj: &Adjustments, adjusted: Option<(Vec<u8>, String)>) -> bool {
        let new_data = match adjusted {
            Some((bytes, name)) if !adj.is_default() => self.structural(h, &[image], |k| k.add_image_data(bytes, &name)).ok(),
            _ => None,
        };
        self.touch(h, image);
        let Some(file) = self.file.file_of(image).map(str::to_string) else { return false };
        {
            let Some(m) = self.file.obj_mut(image) else { return false };
            m.remove("imageAdjustments");
            m.remove("adjustedImageData");
            m.remove("thumbnailAdjustedImageData");
            if !adj.is_default() {
                let mut a = Msg::new("TSD.ImageAdjustmentsArchive");
                for (n, v) in [
                    ("exposure", adj.exposure),
                    ("saturation", adj.saturation),
                    ("contrast", adj.contrast),
                    ("highlights", adj.highlights),
                    ("shadows", adj.shadows),
                    ("sharpness", adj.sharpness),
                    ("denoise", adj.denoise),
                    ("temperature", adj.temperature),
                    ("tint", adj.tint),
                    ("bottom_level", adj.bottom_level),
                    ("top_level", adj.top_level),
                    ("gamma", adj.gamma),
                ] {
                    a.set_f32(n, v);
                }
                a.set_bool("enhance", adj.enhance);
                m.push_msg("imageAdjustments", a);
                if let Some(d) = new_data {
                    m.push_msg("adjustedImageData", data_ref(d));
                    m.push_msg("thumbnailAdjustedImageData", data_ref(d));
                }
            }
        }
        // the archive lists every picture it uses
        if let Some(a) = self.file.archive_mut(image) {
            let mut ids = Vec::new();
            for f in ["data", "thumbnailData", "originalData", "adjustedImageData", "thumbnailAdjustedImageData", "enhancedImageData"] {
                if let Some(i) = a.obj().msg(f).and_then(|r| r.u64("identifier")) {
                    if !ids.contains(&i) {
                        ids.push(i);
                    }
                }
            }
            a.set_data_references(0, &ids);
        }
        if let Some(d) = new_data {
            self.file.add_data_reference(&file, d, image);
        }
        true
    }

    /// Format > Image > Replace…: new picture, same width, height following its proportions.
    pub fn replace_image(&mut self, h: &mut History, image: ObjId, bytes: Vec<u8>, name: &str) -> Result<(), kn_format::Error> {
        let (pw, ph) = kn_format::objects::image_size(&bytes).ok_or(kn_format::Error::Invalid("only PNG and JPEG pictures are supported".into()))?;
        let data = self.structural(h, &[image], |k| k.add_image_data(bytes, name))?;
        self.touch(h, image);
        self.file.set_image_data(image, data, pw as f32, ph as f32);
        if let Some(m) = self.file.obj_mut(image) {
            m.remove("imageAdjustments");
        }
        let g = crate::scene::geometry(self.file.obj(image).and_then(|m| m.find_super("TSD.DrawableArchive")).ok_or(kn_format::Error::Invalid("no image".into()))?);
        let g2 = crate::Geometry { h: g.w * ph as f32 / pw.max(1) as f32, ..g };
        self.set_frame(h, image, g2);
        self.reset_mask(h, image);
        Ok(())
    }

    /// Format > Image > Reset Mask: the mask covers the whole picture again (rectangle).
    pub fn reset_mask(&mut self, h: &mut History, image: ObjId) -> bool {
        let Some(mask) = self.file.obj(image).and_then(|m| m.ref_id("mask")) else { return false };
        let Some(g) = self.file.obj(image).and_then(|m| m.find_super("TSD.DrawableArchive")).map(crate::scene::geometry) else { return false };
        self.touch(h, mask);
        if let Some(d) = self.file.obj_mut(mask).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
            let geo = d.msg_mut_or_create("geometry");
            let p = geo.msg_mut_or_create("position");
            p.set_f32("x", 0.0);
            p.set_f32("y", 0.0);
            let s = geo.msg_mut_or_create("size");
            s.set_f32("width", g.w);
            s.set_f32("height", g.h);
            geo.set_f32("angle", 0.0);
        }
        self.mask_with_shape(h, image, crate::insert::BasicShape::Rectangle)
    }

    /// Format > Image > Remove Background (`Some(png)`: the picture with its background made
    /// transparent) / Reset Background (`None`). The untouched picture is kept as originalData.
    pub fn set_background_removed(&mut self, h: &mut History, image: ObjId, png: Option<Vec<u8>>) -> Result<(), kn_format::Error> {
        let m = self.file.obj(image).ok_or(kn_format::Error::Invalid("no image".into()))?;
        let cur = m.msg("data").and_then(|r| r.u64("identifier"));
        let original = m.msg("originalData").and_then(|r| r.u64("identifier"));
        match png {
            Some(bytes) => {
                let id = self.structural(h, &[image], |k| k.add_image_data(bytes, "background-removed.png"))?;
                self.touch(h, image);
                let file = self.file.file_of(image).map(str::to_string);
                if let Some(m) = self.file.obj_mut(image) {
                    if original.is_none() {
                        if let Some(c) = cur {
                            m.push_msg("originalData", data_ref(c));
                        }
                    }
                    m.remove("data");
                    m.push_msg("data", data_ref(id));
                    m.remove("adjustedImageData");
                    m.remove("thumbnailAdjustedImageData");
                    // (Keynote's own Remove Background keeps the picture and adds a traced mask;
                    // this stores the cut-out picture, which every reader shows as is)
                }
                if let Some(f) = file {
                    self.file.add_data_reference(&f, id, image);
                }
            }
            None => {
                let Some(o) = original else { return Ok(()) };
                self.touch(h, image);
                if let Some(m) = self.file.obj_mut(image) {
                    m.remove("data");
                    m.push_msg("data", data_ref(o));
                    m.remove("originalData");
                    m.remove("background_removed");
                }
            }
        }
        if let Some(a) = self.file.archive_mut(image) {
            let mut ids = Vec::new();
            for f in ["data", "thumbnailData", "originalData", "adjustedImageData", "thumbnailAdjustedImageData"] {
                if let Some(i) = a.obj().msg(f).and_then(|r| r.u64("identifier")) {
                    if !ids.contains(&i) {
                        ids.push(i);
                    }
                }
            }
            a.set_data_references(0, &ids);
        }
        Ok(())
    }

    /// Format > Image > Edit Mask: move the picture by (dx, dy) while its mask stays put.
    pub fn move_image_in_mask(&mut self, h: &mut History, image: ObjId, dx: f32, dy: f32) -> bool {
        let Some(mask) = self.file.obj(image).and_then(|m| m.ref_id("mask")) else { return false };
        self.move_by(h, image, dx, dy);
        self.move_by(h, mask, -dx, -dy);
        true
    }

    /// The image description (accessibility) shown in Format > Image > Description.
    pub fn set_description(&mut self, h: &mut History, id: ObjId, text: &str) {
        self.touch(h, id);
        if let Some(d) = self.file.obj_mut(id).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
            if text.is_empty() {
                d.remove("accessibility_description");
            } else {
                d.set_str("accessibility_description", text);
            }
        }
    }
}

/// One change to a movie (Format > Movie).
#[derive(Clone, Debug, PartialEq)]
pub enum MovieProp {
    Volume(f32),
    /// 0 none, 1 loop, 2 back and forth.
    Loop(u32),
    Trim(f32, f32),
    AutoPlay(bool),
    AcrossSlides(bool),
}

impl Document {
    pub fn set_movie_prop(&mut self, h: &mut History, movie: ObjId, prop: &MovieProp) -> bool {
        self.touch(h, movie);
        let Some(m) = self.file.obj_mut(movie) else { return false };
        match prop {
            MovieProp::Volume(v) => m.set_f32("volume", v.clamp(0.0, 1.0)),
            MovieProp::Loop(l) => {
                m.set_enum("loop_option", ["None", "Repeat", "BackAndForth"][(*l as usize).min(2)]);
                m.set_u64("loopOptionAsInteger", u64::from((*l).min(2)));
            }
            MovieProp::Trim(a, b) => {
                m.set_f32("startTime", a.max(0.0));
                m.set_f32("endTime", b.max(*a));
            }
            MovieProp::AutoPlay(b) => m.set_bool("autoPlay", *b),
            MovieProp::AcrossSlides(b) => m.set_bool("playsAcrossSlides", *b),
        }
        true
    }

    /// Format > Movie > Poster Frame: the still shown before the movie plays.
    pub fn set_movie_poster(&mut self, h: &mut History, movie: ObjId, t: f32, jpeg: Vec<u8>) -> Result<(), kn_format::Error> {
        let id = self.structural(h, &[movie], |k| k.add_image_data(jpeg, "posterImage.jpeg"))?;
        self.touch(h, movie);
        let file = self.file.file_of(movie).map(str::to_string);
        if let Some(m) = self.file.obj_mut(movie) {
            m.set_f32("posterTime", t.max(0.0));
            m.remove("posterImageData");
            m.push_msg("posterImageData", data_ref(id));
        }
        if let Some(a) = self.file.archive_mut(movie) {
            let mut ids = a.data_references(0);
            if !ids.contains(&id) {
                ids.push(id);
            }
            a.set_data_references(0, &ids);
        }
        if let Some(f) = file {
            self.file.add_data_reference(&f, id, movie);
        }
        Ok(())
    }
}

impl Document {
    /// Insert > Equation: the LaTeX source kept with the picture (Keynote's equation fields).
    pub fn set_equation_source(&mut self, h: &mut History, image: ObjId, latex: &str) {
        self.touch(h, image);
        if let Some(m) = self.file.obj_mut(image) {
            m.set_str("equation_source_text", latex);
        }
    }
}

impl Document {
    /// Format > Style > Title / Caption on an object (image, shape, movie…): shown or hidden.
    /// The first time, Keynote's stand-in becomes a TSA.CaptionInfoArchive text box ("Title" /
    /// "Caption", placed above / below the object; layout verified on the Mac).
    pub fn set_caption(&mut self, h: &mut History, slide: ObjId, drawable: ObjId, title: bool, on: bool) -> bool {
        let (field, hidden) = if title { ("title", "title_hidden") } else { ("caption", "caption_hidden") };
        let Some(cur) = self.file.obj(drawable).and_then(|m| m.find_super("TSD.DrawableArchive")).map(|d| d.ref_id(field)) else { return false };
        let real = cur.filter(|c| self.file.type_name(*c) == "TSA.CaptionInfoArchive");
        if real.is_some() || !on {
            self.touch(h, drawable);
            if let Some(d) = self.file.obj_mut(drawable).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
                d.set_bool(hidden, !on);
            }
            return true;
        }
        // a new caption text box, taken off the slide and wrapped as the object's caption
        let Ok(id) = self.insert(h, slide, &crate::insert::NewObject::TextBox) else { return false };
        if let Some(st) = self.storage_of(id) {
            self.set_storage_text(h, st, if title { "Title" } else { "Caption" });
            let ident = if title { "captions-0-paragraphstyle-Caption Title" } else { "captions-0-paragraphstyle-Caption Text" };
            if let Some(ps) = self.style_by_identifier("TSWP.ParagraphStyleArchive", ident) {
                self.set_paragraph_style(h, st, ps);
            }
        }
        let Some(g) = self.file.obj(drawable).and_then(|m| m.find_super("TSD.DrawableArchive")).map(crate::scene::geometry) else { return false };
        let Some(file) = self.file.file_of(id).map(str::to_string) else { return false };
        let ok = self.structural(h, &[slide, drawable, id], |k| {
            if let Some(s) = k.obj_mut(slide) {
                s.retain_msgs("drawables_z_order", |r| r.u64("identifier") != Some(id));
                s.retain_msgs("owned_drawables", |r| r.u64("identifier") != Some(id));
            }
            k.refresh_refs(slide);
            let pid = k.alloc_ids(1)[0];
            let mut pl = Msg::new("TSA.CaptionPlacementArchive");
            pl.set_i64("caption_anchor_location", if title { 7 } else { 1 });
            pl.set_i64("drawable_anchor_location", if title { 1 } else { 7 });
            k.insert_archive(&file, kn_format::Archive::new_object(pid, pl));
            let Some(tid) = kn_format::schema::schema().type_id_of("TSA.CaptionInfoArchive") else { return false };
            let Some(a) = k.archive_mut(id) else { return false };
            let mut inner = a.payloads[0].clone();
            if let Some(d) = inner.find_super_mut("TSD.DrawableArchive") {
                d.set_ref("parent", drawable);
                let geo = d.msg_mut_or_create("geometry");
                let p = geo.msg_mut_or_create("position");
                p.set_f32("x", 0.0);
                p.set_f32("y", 0.0);
                let sz = geo.msg_mut_or_create("size");
                sz.set_f32("width", g.w);
                sz.set_f32("height", 0.0);
                geo.set_u64("flags", 1);
            }
            let mut cap = Msg::new("TSA.CaptionInfoArchive");
            cap.push_msg("super", inner);
            cap.set_ref("placement", pid);
            cap.set_enum("childInfoKind", if title { "Title" } else { "Caption" });
            a.payloads[0] = cap;
            if let Some(mi) = a.info.msgs_mut("message_infos").next() {
                mi.set_u64("type", u64::from(tid));
            }
            a.refresh_object_references();
            if let Some(d) = k.obj_mut(drawable).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
                d.set_ref(field, id);
                d.set_bool(hidden, false);
            }
            k.refresh_refs(drawable);
            true
        });
        if let Some(f) = self.file.file_of(drawable).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        ok
    }
}

/// The gallery extension field, declared like Keynote does (its items and caption storage).
fn gallery_field_info(k: &mut kn_format::KeyFile, image: ObjId) {
    let Some(g) = k.obj(image).and_then(|m| m.msg("gallery_info")) else { return };
    let mut refs: Vec<u64> = g.ref_ids("items");
    refs.extend(g.ref_id("caption_storage"));
    if let Some(a) = k.archive_mut(image) {
        a.set_field_info(0, &[200], &refs, &[]);
    }
}

/// An empty gallery caption storage as Keynote writes it: message type 2001, and only the
/// paragraph and list styles listed as references (not the stylesheet; Mac round 20).
fn caption_archive(id: ObjId, m: Msg, para: Option<ObjId>, list: Option<ObjId>) -> kn_format::Archive {
    let mut a = kn_format::Archive::new_object(id, m);
    if let Some(mi) = a.info.msgs_mut("message_infos").next() {
        mi.set_u64("type", 2001);
    }
    let refs: Vec<ObjId> = para.into_iter().chain(list).collect();
    a.set_object_references(0, &refs);
    a
}

impl Document {
    /// Insert > Image Gallery: make an inserted picture a gallery of itself and `more` pictures
    /// (Keynote: ImageArchive extension 200, one TSA.GalleryItem object per picture; Mac
    /// round 12). The picture itself keeps showing the first one.
    pub fn make_gallery(&mut self, h: &mut History, image: ObjId, more: Vec<(Vec<u8>, String)>) -> Result<(), kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let file = self.file.file_of(image).ok_or(err("no image"))?.to_string();
        // an existing gallery gets the pictures added after its own
        let existing = self.file.obj(image).and_then(|m| m.msg("gallery_info")).is_some();
        let first = self.file.obj(image).and_then(|m| m.msg("data")).and_then(|d| d.u64("identifier")).ok_or(err("the picture has no data"))?;
        // Keynote always gives the gallery and every item a caption storage (empty ones are
        // fine; without them it crashes while loading, Mac round 20)
        let sheet = self.file.stylesheet_id().ok_or(err("no stylesheet"))?;
        let para = self.style_by_identifier("TSWP.ParagraphStyleArchive", "captions-0-paragraphstyle-Caption Text");
        let list = self.style_by_identifier("TSWP.ListStyleArchive", "text-0-liststyle-None");
        let storage = || {
            let mut s = Msg::new("TSWP.StorageArchive");
            s.set_ref("style_sheet", sheet);
            let e = s.msg_mut_or_create("table_para_style").push_new("entries");
            e.set_u64("character_index", 0);
            if let Some(p) = para {
                e.set_ref("object", p);
            }
            for t in ["table_para_data", "table_para_starts"] {
                let e = s.msg_mut_or_create(t).push_new("entries");
                e.set_u64("character_index", 0);
                e.set_u64("first", 0);
                e.set_u64("second", 0);
            }
            let e = s.msg_mut_or_create("table_list_style").push_new("entries");
            e.set_u64("character_index", 0);
            if let Some(l) = list {
                e.set_ref("object", l);
            }
            s.set_bool("in_document", true);
            let e = s.msg_mut_or_create("table_para_bidi").push_new("entries");
            e.set_u64("character_index", 0);
            e.set_u64("first", 65535);
            e.set_u64("second", 65535);
            s.msg_mut_or_create("table_drop_cap_style").push_new("entries").set_u64("character_index", 0);
            s
        };
        self.structural(h, &[image], |k| {
            let mut datas = if existing { Vec::new() } else { vec![first] };
            for (bytes, name) in more {
                datas.push(k.add_image_data(bytes, &name)?);
            }
            let ids = k.alloc_ids(datas.len() as u64 * 2 + 1);
            let (items, caps) = ids.split_at(datas.len());
            for ((id, cap), data) in items.iter().zip(caps).zip(&datas) {
                k.insert_archive(&file, caption_archive(*cap, storage(), para, list));
                let mut it = Msg::new("TSA.GalleryItem");
                it.set_ref("image_data", *data);
                it.set_ref("thumbnail_image_data", *data);
                it.set_ref("caption_storage", *cap);
                let mut a = kn_format::Archive::new_object(*id, it);
                a.refresh_object_references();
                k.insert_archive(&file, a);
                k.add_data_reference(&file, *data, *id);
            }
            let Some(m) = k.obj_mut(image) else { return Err(err("no image")) };
            let gallery_cap = if existing {
                m.msg("gallery_info").and_then(|g| g.ref_id("caption_storage")).unwrap_or(*caps.last().unwrap())
            } else {
                *caps.last().unwrap()
            };
            let g = m.msg_mut_or_create("gallery_info");
            for id in items {
                g.push_ref("items", *id);
            }
            if !existing {
                g.set_enum("caption_mode", "GalleryCaptionModePerImage");
                g.set_ref("caption_storage", gallery_cap);
                k.insert_archive(&file, caption_archive(gallery_cap, storage(), para, list));
            }
            k.refresh_refs(image);
            gallery_field_info(k, image);
            Ok(())
        })
    }

    /// Format > Gallery > Images, "−": remove image `index` (a gallery keeps at least one).
    pub fn remove_gallery_image(&mut self, h: &mut History, image: ObjId, index: usize) -> bool {
        let items = self.file.obj(image).and_then(|m| m.msg("gallery_info")).map(|g| g.ref_ids("items")).unwrap_or_default();
        if items.len() < 2 || index >= items.len() {
            return false;
        }
        let gone = items[index];
        let keep: Vec<u64> = items.iter().copied().filter(|i| *i != gone).collect();
        let first_data = self.file.obj(keep[0]).and_then(|m| m.msg("image_data")).and_then(|d| d.u64("identifier"));
        self.structural(h, &[image], |k| {
            let cap = k.obj(gone).and_then(|m| m.ref_id("caption_storage"));
            if let Some(g) = k.obj_mut(image).and_then(|m| m.msg_mut("gallery_info")) {
                g.remove("items");
                for i in &keep {
                    g.push_ref("items", *i);
                }
            }
            // the picture itself shows the first image
            if let (Some(d), Some(m)) = (first_data, k.obj_mut(image)) {
                if let Some(r) = m.msg_mut("data") {
                    r.set_u64("identifier", d);
                }
            }
            k.remove_archive(gone);
            if let Some(c) = cap {
                k.remove_archive(c);
            }
            k.refresh_refs(image);
            gallery_field_info(k, image);
        });
        true
    }

    /// Format > Gallery > Images: drag image `from` to place `to`.
    pub fn move_gallery_image(&mut self, h: &mut History, image: ObjId, from: usize, to: usize) -> bool {
        let mut items = self.file.obj(image).and_then(|m| m.msg("gallery_info")).map(|g| g.ref_ids("items")).unwrap_or_default();
        if from >= items.len() || to >= items.len() || from == to {
            return false;
        }
        let it = items.remove(from);
        items.insert(to, it);
        let first_data = self.file.obj(items[0]).and_then(|m| m.msg("image_data")).and_then(|d| d.u64("identifier"));
        self.touch(h, image);
        if let Some(m) = self.file.obj_mut(image) {
            if let Some(g) = m.msg_mut("gallery_info") {
                g.remove("items");
                for i in &items {
                    g.push_ref("items", *i);
                }
            }
            if let (Some(d), Some(r)) = (first_data, m.msg_mut("data")) {
                r.set_u64("identifier", d);
            }
        }
        true
    }

    /// Format > Gallery > Caption: shown or not, one per image (1) or the same for all (2).
    pub fn set_gallery_caption(&mut self, h: &mut History, image: ObjId, on: bool, mode: u8) {
        self.touch(h, image);
        if let Some(m) = self.file.obj_mut(image) {
            if let Some(d) = m.find_super_mut("TSD.DrawableArchive") {
                d.set_bool("caption_hidden", !on);
            }
            if let Some(g) = m.msg_mut("gallery_info") {
                g.set_enum("caption_mode", if mode == 2 { "GalleryCaptionModeAllImages" } else { "GalleryCaptionModePerImage" });
            }
        }
    }

    /// Format > Gallery > Image Description of one gallery image.
    pub fn set_gallery_description(&mut self, h: &mut History, item: ObjId, text: &str) {
        self.touch(h, item);
        if let Some(m) = self.file.obj_mut(item) {
            if text.is_empty() {
                m.remove("accessibility_description");
            } else {
                m.set_str("accessibility_description", text);
            }
        }
    }

    /// Insert > Web Video: make an inserted poster picture a web video of `url` (Keynote:
    /// ImageArchive extension 300, the poster shared with the picture; Mac round 12).
    pub fn make_web_video(&mut self, h: &mut History, image: ObjId, url: &str, title: &str, author: &str) -> bool {
        let Some(data) = self.file.obj(image).and_then(|m| m.msg("data")).and_then(|d| d.u64("identifier")) else { return false };
        self.touch(h, image);
        let Some(m) = self.file.obj_mut(image) else { return false };
        let w = m.msg_mut_or_create("web_video_info");
        w.set_str("url", url);
        w.set_ref("poster_image_data", data);
        let a = w.msg_mut_or_create("attribution");
        if !title.is_empty() {
            a.set_str("title", title);
        }
        a.set_str("external_url", url);
        if !author.is_empty() {
            a.set_str("author_name", author);
        }
        if let Some(a) = self.file.archive_mut(image) {
            a.set_field_info(0, &[300], &[], &[data]);
        }
        true
    }
}

#[cfg(test)]
mod caption_tests {
    use crate::edit::History;
    use crate::{Document, DrawableKind};

    fn caption_texts(d: &Document) -> Vec<String> {
        let s = d.slides();
        let sc = d.scene(&s, 0).unwrap();
        sc.drawables
            .iter()
            .filter(|x| d.file.type_name(x.id) == "TSA.CaptionInfoArchive")
            .filter_map(|x| match &x.kind {
                DrawableKind::Shape(s) => s.text.as_ref().map(|t| t.text.clone()),
                _ => None,
            })
            .collect()
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn keynote_captions_show() {
        let d = Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R13-captions.key")).unwrap();
        let t = caption_texts(&d);
        assert!(t.iter().any(|x| x.contains("title")) || t.iter().any(|x| x.contains("Title")), "{t:?}");
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn add_a_caption() {
        let mut d = Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key")).unwrap();
        let s = d.slides();
        let img = 33438;
        let mut h = History::default();
        d.begin(&mut h, "Caption");
        assert!(d.set_caption(&mut h, s[2].slide, img, false, true));
        d.end(&mut h);
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let d2 = Document::from_file(k);
        let s2 = d2.slides();
        let sc = d2.scene(&s2, 2).unwrap();
        let g = sc.drawables.iter().find(|x| x.id == img).unwrap().geom;
        let cap = sc.drawables.iter().find(|x| d2.file.type_name(x.id) == "TSA.CaptionInfoArchive").unwrap();
        assert!((cap.geom.y - (g.y + g.h)).abs() < 0.5);
    }
}
