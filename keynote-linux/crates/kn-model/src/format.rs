//! Format edits (Format inspector): fill, border, shadow, reflection, opacity, text attributes,
//! slide background. Like Keynote, an object never changes a shared style: it gets its own style
//! *variation* (parent = the theme style, only the overridden properties) and points at it.

use kn_format::objects::fix_override_count;
use kn_format::{KeyFile, Msg, ObjId};

use crate::edit::History;
use crate::{Align, Color, Document};

#[derive(Clone, Debug, PartialEq)]
pub enum FillSpec {
    None,
    Color(Color),
    /// Two-colour linear gradient; `angle` in degrees (stored in radians, as Keynote does).
    Gradient { from: Color, to: Color, angle: f32 },
    /// Advanced Gradient Fill: stops (fraction, colour, inflection), linear at `angle` degrees or radial.
    Advanced { stops: Vec<(f32, Color, f32)>, radial: bool, angle: f32 },
    /// Image Fill of picture data already in the file; `size` is Keynote's fill size (Scale).
    /// `tint` (Advanced Image Fill): a translucent colour over the picture, its alpha the strength.
    Image { data_id: u64, technique: crate::ImageFillTechnique, size: Option<(f32, f32)>, reference: Option<Color>, tint: Option<Color> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dash {
    Solid,
    Dashed,
    Dotted,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrokeSpec {
    pub color: Color,
    pub width: f32,
    pub dash: Dash,
}

/// Format > Text > Layout options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextLayoutChange {
    /// Text inset on all four sides, points.
    Inset(f32),
    /// Column count and gutter.
    Columns(u32, f32),
    ShrinkToFit(bool),
}

/// One text attribute, applied to a whole text box.
#[derive(Clone, Debug, PartialEq)]
pub enum TextProp {
    Font(String),
    Size(f32),
    Bold(bool),
    Italic(bool),
    Underline(bool),
    Strike(bool),
    Color(Color),
    Align(Align),
    /// Relative line spacing (1.0 = single).
    LineSpacing(f32),
    Bullets(bool),
    VAlign(crate::VAlign),
    /// +1 superscript, -1 subscript, 0 normal baseline.
    Script(i8),
    Caps(crate::Caps),
    /// Character spacing as a fraction of the font size (0.05 = Keynote's "5%").
    Tracking(f32),
    /// Baseline shift in points (Format > Font > Baseline > Raise / Lower).
    BaselineShift(f32),
    /// Ligatures: 0 none (required only), 1 default, 2 all.
    Ligatures(u8),
    /// Space before / after a paragraph, in points.
    SpaceBefore(f32),
    SpaceAfter(f32),
    /// Indents (Layout > Indents), in points: the first line, the left and right edges.
    FirstIndent(f32),
    LeftIndent(f32),
    RightIndent(f32),
}

impl TextProp {
    pub fn is_char(&self) -> bool {
        matches!(
            self,
            TextProp::Font(_)
                | TextProp::Size(_)
                | TextProp::Bold(_)
                | TextProp::Italic(_)
                | TextProp::Underline(_)
                | TextProp::Strike(_)
                | TextProp::Color(_)
                | TextProp::Script(_)
                | TextProp::Caps(_)
                | TextProp::Tracking(_)
                | TextProp::BaselineShift(_)
                | TextProp::Ligatures(_)
        )
    }
}

pub fn color_msg(c: Color) -> Msg {
    let mut m = Msg::new("TSP.Color");
    m.set_enum("model", "rgb");
    m.set_f32("r", c.r);
    m.set_f32("g", c.g);
    m.set_f32("b", c.b);
    m.set_f32("a", c.a);
    m.set_enum("rgbspace", "srgb");
    m.set_f32("headroom", 1.0);
    m
}

pub(crate) fn fill_msg(f: &FillSpec) -> Msg {
    let mut m = Msg::new("TSD.FillArchive");
    match f {
        FillSpec::None => {}
        FillSpec::Color(c) => m.push_msg("color", color_msg(*c)),
        FillSpec::Gradient { from, to, angle } => {
            let g = m.msg_mut_or_create("gradient");
            g.set_enum("type", "Linear");
            for (frac, c) in [(0.0, from), (1.0, to)] {
                let mut s = Msg::new("TSD.GradientArchive.GradientStop");
                s.push_msg("color", color_msg(*c));
                s.set_f32("fraction", frac);
                s.set_f32("inflection", 0.5);
                g.push_msg("stops", s);
            }
            g.set_f32("opacity", 1.0);
            g.set_bool("advancedGradient", false);
            g.msg_mut_or_create("anglegradient").set_f32("gradientangle", angle.to_radians());
        }
        FillSpec::Advanced { stops, radial, angle } => {
            let g = m.msg_mut_or_create("gradient");
            g.set_enum("type", if *radial { "Radial" } else { "Linear" });
            for (frac, c, infl) in stops {
                let mut s = Msg::new("TSD.GradientArchive.GradientStop");
                s.push_msg("color", color_msg(*c));
                s.set_f32("fraction", *frac);
                s.set_f32("inflection", *infl);
                g.push_msg("stops", s);
            }
            g.set_f32("opacity", 1.0);
            g.set_bool("advancedGradient", true);
            g.msg_mut_or_create("anglegradient").set_f32("gradientangle", angle.to_radians());
        }
        FillSpec::Image { data_id, technique, size, reference, tint } => {
            // as Keynote writes it (R37-6)
            let i = m.msg_mut_or_create("image");
            i.set_enum(
                "technique",
                match technique {
                    crate::ImageFillTechnique::Natural => "NaturalSize",
                    crate::ImageFillTechnique::Stretch => "Stretch",
                    crate::ImageFillTechnique::Tile => "Tile",
                    crate::ImageFillTechnique::ScaleToFill => "ScaleToFill",
                    crate::ImageFillTechnique::ScaleToFit => "ScaleToFit",
                },
            );
            if let Some(t) = tint {
                i.push_msg("tint", color_msg(*t));
            }
            if let Some((w, h)) = size {
                let fs = i.msg_mut_or_create("fillsize");
                fs.set_f32("width", *w);
                fs.set_f32("height", *h);
            }
            i.msg_mut_or_create("imagedata").set_u64("identifier", *data_id);
            i.set_bool("interpretsUntaggedImageDataAsGeneric", true);
            if let Some(c) = reference {
                i.push_msg("referencecolor", color_msg(*c));
            }
        }
    }
    m
}

/// The picture data a fill uses, which the object holding it must list in its archive info.
pub(crate) fn fill_data(f: &FillSpec) -> Option<u64> {
    match f {
        FillSpec::Image { data_id, .. } => Some(*data_id),
        _ => None,
    }
}

/// List `data` among the data references of object `id` (an image fill's picture).
pub(crate) fn add_data_ref(k: &mut KeyFile, id: ObjId, data: Option<u64>) {
    let (Some(d), Some(a)) = (data, k.archive_mut(id)) else { return };
    let mut refs = a.data_references(0);
    if !refs.contains(&d) {
        refs.push(d);
        refs.sort_unstable();
        a.set_data_references(0, &refs);
    }
}

pub(crate) fn stroke_msg(s: Option<&StrokeSpec>) -> Msg {
    let mut m = Msg::new("TSD.StrokeArchive");
    let (color, width, dash) = match s {
        Some(s) => (s.color, s.width, s.dash),
        None => (Color::BLACK, 1.0, Dash::Solid),
    };
    m.push_msg("color", color_msg(color));
    m.set_f32("width", width);
    m.set_enum("cap", if dash == Dash::Dotted { "RoundCap" } else { "ButtCap" });
    m.set_enum("join", "MiterJoin");
    m.set_f32("miter_limit", 4.0);
    let p = m.msg_mut_or_create("pattern");
    let (ty, pat): (&str, &[f32]) = match (s, dash) {
        (None, _) => ("TSDEmptyPattern", &[]),
        (_, Dash::Solid) => ("TSDSolidPattern", &[]),
        (_, Dash::Dashed) => ("TSDPattern", &[4.0, 2.0]),
        (_, Dash::Dotted) => ("TSDPattern", &[0.01, 2.0]),
    };
    p.set_enum("type", ty);
    p.set_f32("phase", 0.0);
    p.set_u64("count", pat.len() as u64);
    // Keynote always writes six pattern slots
    let num = p.field_def("pattern").unwrap().num;
    for i in 0..6 {
        let v = pat.get(i).copied().unwrap_or(0.0);
        p.fields.push(kn_format::msg::Field { num, val: kn_format::Val::I32(v.to_bits()) });
    }
    m
}

/// Where a drawable keeps its style reference.
fn style_holder(m: &mut Msg) -> Option<&mut Msg> {
    let tn = m.type_name();
    if matches!(tn, "TSD.ImageArchive" | "TSD.MovieArchive" | "KN.SlideArchive") {
        return Some(m);
    }
    m.find_super_mut("TSD.ShapeArchive")
}

fn style_ref(k: &KeyFile, id: ObjId) -> Option<ObjId> {
    let m = k.obj(id)?;
    let tn = m.type_name();
    if matches!(tn, "TSD.ImageArchive" | "TSD.MovieArchive" | "KN.SlideArchive") {
        return m.ref_id("style");
    }
    m.find_super("TSD.ShapeArchive")?.ref_id("style")
}

/// The graphic property message (TSD shape or media properties) of a style message.
fn graphic_props(style: &mut Msg) -> Option<&mut Msg> {
    if style.find_super("TSD.ShapeStyleArchive").is_some() {
        return Some(style.find_super_mut("TSD.ShapeStyleArchive")?.msg_mut_or_create("shape_properties"));
    }
    if style.find_super("TSD.MediaStyleArchive").is_some() {
        return Some(style.find_super_mut("TSD.MediaStyleArchive")?.msg_mut_or_create("media_properties"));
    }
    None
}

impl Document {
    /// A style variation of `base` that this action may change: `base` itself if the action
    /// created it, else a new variation.
    pub(crate) fn variation_of(&mut self, h: &mut History, base: ObjId) -> Option<ObjId> {
        if h.created_now(base) {
            return Some(base);
        }
        let sheet = self.file.stylesheet_id();
        let touch: Vec<ObjId> = sheet.into_iter().collect();
        self.structural(h, &touch, |k| k.new_style_variation(base)).ok()
    }

    /// Point `holder` (a drawable or slide) at its own variation of its style; returns it.
    fn own_style(&mut self, h: &mut History, holder: ObjId) -> Option<ObjId> {
        let base = style_ref(&self.file, holder)?;
        let v = self.variation_of(h, base)?;
        if v != base {
            self.touch(h, holder);
            let a = self.file.archive_mut(holder)?;
            style_holder(a.obj_mut())?.set_ref("style", v);
            self.file.refresh_refs(holder);
            if let Some(f) = self.file.file_of(holder).map(str::to_string) {
                self.file.ensure_external_refs(&f);
            }
        }
        Some(v)
    }

    fn edit_graphic(&mut self, h: &mut History, id: ObjId, f: impl FnOnce(&mut Msg)) -> bool {
        let Some(st) = self.own_style(h, id) else { return false };
        self.touch(h, st);
        let Some(a) = self.file.archive_mut(st) else { return false };
        let ok = match graphic_props(a.obj_mut()) {
            Some(p) => {
                f(p);
                true
            }
            None => false,
        };
        fix_override_count(a.obj_mut());
        self.file.refresh_refs(st);
        ok
    }

    pub fn set_fill(&mut self, h: &mut History, id: ObjId, fill: &FillSpec) -> bool {
        let m = fill_msg(fill);
        // (pictures and movies have no fill: nothing to do, rather than a crash)
        let mut fits = true;
        let ok = self.edit_graphic(h, id, |p| {
            if !p.has_field("fill") {
                fits = false;
                return;
            }
            p.remove("fill");
            p.push_msg("fill", m);
        }) && fits;
        if let Some(st) = style_ref(&self.file, id) {
            add_data_ref(&mut self.file, st, fill_data(fill));
        }
        ok
    }

    /// Border / line stroke; `None` = no border.
    pub fn set_stroke(&mut self, h: &mut History, id: ObjId, stroke: Option<&StrokeSpec>) -> bool {
        let m = stroke_msg(stroke);
        self.edit_graphic(h, id, |p| {
            p.remove("stroke");
            p.push_msg("stroke", m);
        })
    }

    pub fn set_shadow(&mut self, h: &mut History, id: ObjId, on: bool) -> bool {
        self.edit_graphic(h, id, |p| {
            let mut s = Msg::new("TSD.ShadowArchive");
            s.push_msg("color", color_msg(Color::BLACK));
            s.set_f32("angle", 315.0);
            s.set_f32("offset", 5.0);
            s.set_i64("radius", 10);
            s.set_f32("opacity", 0.75);
            s.set_bool("is_enabled", on);
            s.set_enum("type", "TSDDropShadow");
            p.remove("shadow");
            p.push_msg("shadow", s);
        })
    }

    /// Format > Style > Shadow with its settings (None: No Shadow).
    pub fn set_shadow_spec(&mut self, h: &mut History, id: ObjId, spec: Option<&crate::Shadow>) -> bool {
        self.edit_graphic(h, id, |p| {
            let d = crate::Shadow { color: Color::BLACK, angle: 315.0, offset: 5.0, blur: 10.0, opacity: 0.75, kind: 0, height: 0.2, curve: 0.6 };
            let sp = spec.unwrap_or(&d);
            let mut s = Msg::new("TSD.ShadowArchive");
            s.push_msg("color", color_msg(sp.color));
            s.set_f32("angle", sp.angle);
            s.set_f32("offset", sp.offset);
            s.set_i64("radius", sp.blur.round() as i64);
            s.set_f32("opacity", sp.opacity);
            s.set_bool("is_enabled", spec.is_some());
            s.set_enum("type", ["TSDDropShadow", "TSDContactShadow", "TSDCurvedShadow"][usize::from(sp.kind.min(2))]);
            match sp.kind {
                1 => {
                    let mut c = Msg::new("TSD.ContactShadowArchive");
                    c.set_f32("height", sp.height);
                    s.push_msg("contactShadow", c);
                }
                2 => {
                    let mut c = Msg::new("TSD.CurvedShadowArchive");
                    c.set_f32("curve", sp.curve);
                    s.push_msg("curvedShadow", c);
                }
                _ => s.push_msg("dropShadow", Msg::new("TSD.DropShadowArchive")),
            }
            p.remove("shadow");
            p.push_msg("shadow", s);
        })
    }

    /// Reflection strength (0 = none).
    pub fn set_reflection_amount(&mut self, h: &mut History, id: ObjId, amount: f32) -> bool {
        self.edit_graphic(h, id, |p| {
            let mut r = Msg::new("TSD.ReflectionArchive");
            if amount > 0.0 {
                r.set_f32("opacity", amount.clamp(0.0, 1.0));
            }
            p.remove("reflection");
            p.push_msg("reflection", r);
        })
    }

    pub fn set_reflection(&mut self, h: &mut History, id: ObjId, on: bool) -> bool {
        self.edit_graphic(h, id, |p| {
            let mut r = Msg::new("TSD.ReflectionArchive");
            if on {
                r.set_f32("opacity", 0.5);
            }
            p.remove("reflection");
            p.push_msg("reflection", r);
        })
    }

    pub fn set_opacity(&mut self, h: &mut History, id: ObjId, opacity: f32) -> bool {
        self.edit_graphic(h, id, |p| p.set_f32("opacity", opacity.clamp(0.0, 1.0)))
    }

    /// Arrowheads on a line (head = end point, tail = start point).
    pub fn set_line_ends(&mut self, h: &mut History, id: ObjId, tail: Option<&Msg>, head: Option<&Msg>) -> bool {
        let none = {
            let mut m = Msg::new("TSD.LineEndArchive");
            m.set_str("identifier", "none");
            m
        };
        let (t, hd) = (tail.cloned().unwrap_or_else(|| none.clone()), head.cloned().unwrap_or(none));
        let mut fits = true;
        self.edit_graphic(h, id, |p| {
            if !p.has_field("head_line_end") {
                fits = false;
                return;
            }
            p.remove("head_line_end");
            p.remove("tail_line_end");
            p.push_msg("head_line_end", hd);
            p.push_msg("tail_line_end", t);
        }) && fits
    }

    pub fn set_slide_background(&mut self, h: &mut History, slide: ObjId, fill: &FillSpec) -> bool {
        let ok = self.set_slide_fill_msg(h, slide, fill_msg(fill));
        if let Some(st) = style_ref(&self.file, slide) {
            add_data_ref(&mut self.file, st, fill_data(fill));
        }
        ok
    }

    /// Replace a picture's bytes everywhere it is used (undo restores the whole file).
    pub fn replace_data(&mut self, h: &mut History, data_id: u64, bytes: Vec<u8>, file_name: &str) -> bool {
        self.touch_all(h);
        self.file.replace_data_bytes(data_id, bytes, file_name)
    }

    /// Write `fill` as field `field` of object `id` (None removes it), listing an image fill's
    /// picture among the object's data references. For fills kept outside styles, such as a
    /// Pages section's background_fill. False when the object has no such field.
    pub fn set_fill_field(&mut self, h: &mut History, id: ObjId, field: &str, fill: Option<&FillSpec>) -> bool {
        if !self.file.obj(id).is_some_and(|m| m.has_field(field)) {
            return false;
        }
        self.touch(h, id);
        let Some(m) = self.file.obj_mut(id) else { return false };
        m.remove(field);
        if let Some(f) = fill {
            m.push_msg(field, fill_msg(f));
        }
        add_data_ref(&mut self.file, id, fill.and_then(fill_data));
        self.file.refresh_refs(id);
        true
    }

    /// Set the slide style's fill to a prepared TSD.FillArchive.
    pub fn set_slide_fill_msg(&mut self, h: &mut History, slide: ObjId, fill: Msg) -> bool {
        let Some(st) = self.own_style(h, slide) else { return false };
        self.touch(h, st);
        let Some(a) = self.file.archive_mut(st) else { return false };
        let s = a.obj_mut();
        let p = s.msg_mut_or_create("slide_properties");
        p.remove("fill");
        p.push_msg("fill", fill);
        fix_override_count(s);
        self.file.refresh_refs(st);
        if let Some(node) = self.slides().into_iter().find(|i| i.slide == slide).map(|i| i.node) {
            self.touch(h, node);
            if let Some(n) = self.file.obj_mut(node) {
                n.set_bool("thumbnailsAreDirty", true);
            }
        }
        true
    }

    // ------------------------------------------------------------------------------ text

    /// Format > Text > Layout: text inset, columns and shrink-to-fit (on the shape's own style).
    pub fn set_text_layout(&mut self, h: &mut History, id: ObjId, change: TextLayoutChange) -> bool {
        let Some(st) = self.own_style(h, id) else { return false };
        self.touch(h, st);
        let Some(a) = self.file.archive_mut(st) else { return false };
        if a.obj().type_name() != "TSWP.ShapeStyleArchive" {
            return false;
        }
        let p = a.obj_mut().msg_mut_or_create("shape_properties");
        match change {
            TextLayoutChange::Inset(v) => {
                let pd = p.msg_mut_or_create("padding");
                for k in ["left", "top", "right", "bottom"] {
                    pd.set_f32(k, v.max(0.0));
                }
            }
            TextLayoutChange::Columns(n, gap) => {
                p.remove("columns");
                let c = p.msg_mut_or_create("columns");
                let e = c.msg_mut_or_create("equal_columns");
                e.set_u64("count", u64::from(n.clamp(1, 10)));
                e.set_f32("gap", gap.max(0.0));
            }
            TextLayoutChange::ShrinkToFit(b) => p.set_bool("shrink_to_fit", b),
        }
        fix_override_count(a.obj_mut());
        true
    }

    /// Text storage of a shape / text box.
    pub fn storage_of(&self, id: ObjId) -> Option<ObjId> {
        let info = self.file.obj(id)?.find_super("TSWP.ShapeInfoArchive")?;
        info.ref_id("owned_storage").or(info.ref_id("deprecated_storage"))
    }

    pub(crate) fn style_by_identifier(&self, type_name: &str, ident: &str) -> Option<ObjId> {
        self.file
            .objects_of(type_name)
            .find(|(_, m)| m.find_super("TSS.StyleArchive").and_then(|s| s.str("style_identifier")) == Some(ident))
            .map(|(id, _)| id)
    }

    /// Apply a text attribute to all the text of a shape / text box.
    pub fn set_text_prop(&mut self, h: &mut History, id: ObjId, prop: &TextProp) -> bool {
        if let TextProp::VAlign(v) = prop {
            let v = match v {
                crate::VAlign::Top => "kFrameAlignTop",
                crate::VAlign::Middle => "kFrameAlignMiddle",
                crate::VAlign::Bottom => "kFrameAlignBottom",
            };
            let Some(st) = self.own_style(h, id) else { return false };
            self.touch(h, st);
            let Some(a) = self.file.archive_mut(st) else { return false };
            if a.obj().type_name() != "TSWP.ShapeStyleArchive" {
                return false;
            }
            a.obj_mut().msg_mut_or_create("shape_properties").set_enum("vertical_alignment", v);
            fix_override_count(a.obj_mut());
            return true;
        }
        let Some(storage) = self.storage_of(id) else { return false };
        if let TextProp::Bullets(on) = prop {
            let ident = if *on { "text-1-liststyle-Bullet" } else { "text-0-liststyle-None" };
            let Some(ls) = self.style_by_identifier("TSWP.ListStyleArchive", ident) else { return false };
            self.touch(h, storage);
            let Some(a) = self.file.archive_mut(storage) else { return false };
            let st = a.obj_mut();
            let t = st.msg_mut_or_create("table_list_style");
            if t.count("entries") == 0 {
                let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
                e.set_u64("character_index", 0);
                t.push_msg("entries", e);
            }
            for e in t.msgs_mut("entries") {
                e.set_ref("object", ls);
            }
            self.file.refresh_refs(storage);
            if let Some(f) = self.file.file_of(storage).map(str::to_string) {
                self.file.ensure_external_refs(&f);
            }
            return true;
        }
        // paragraph styles (every paragraph), and for character attributes also the character
        // styles of runs, so the change wins everywhere
        let Some(stm) = self.file.obj(storage) else { return false };
        let mut tables = vec!["table_para_style"];
        if prop.is_char() {
            tables.push("table_char_style");
        }
        let mut bases: Vec<ObjId> = Vec::new();
        for t in &tables {
            for e in stm.msg(t).map(|t| t.msgs("entries").collect::<Vec<_>>()).unwrap_or_default() {
                if let Some(o) = e.ref_id("object") {
                    if !bases.contains(&o) {
                        bases.push(o);
                    }
                }
            }
        }
        let needs_default = stm.msg("table_para_style").map(|t| t.msgs("entries").all(|e| e.ref_id("object").is_none())).unwrap_or(true);
        if needs_default {
            let shape_style = style_ref(&self.file, id);
            if let Some(ps) = crate::style::shape_paragraph_style(&self.file, shape_style) {
                bases.push(ps);
            }
        }
        let mut map = Vec::new();
        for b in bases {
            let Some(v) = self.variation_of(h, b) else { continue };
            self.touch(h, v);
            if let Some(a) = self.file.archive_mut(v) {
                apply_text_prop(a.obj_mut(), prop);
                fix_override_count(a.obj_mut());
            }
            map.push((b, v));
        }
        self.touch(h, storage);
        let Some(a) = self.file.archive_mut(storage) else { return false };
        let st = a.obj_mut();
        for t in tables {
            let Some(tm) = st.msg_mut(t) else { continue };
            for e in tm.msgs_mut("entries") {
                if let Some(o) = e.ref_id("object") {
                    if let Some((_, v)) = map.iter().find(|(b, _)| *b == o) {
                        e.set_ref("object", *v);
                    }
                }
            }
        }
        if needs_default {
            if let Some((_, v)) = map.last() {
                let t = st.msg_mut_or_create("table_para_style");
                t.remove("entries");
                let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
                e.set_u64("character_index", 0);
                e.set_ref("object", *v);
                t.push_msg("entries", e);
            }
        }
        self.file.refresh_refs(storage);
        if let Some(f) = self.file.file_of(storage).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        true
    }
}

impl Document {
    /// Fonts (Keynote PostScript names) used by the document's styles, with how many styles use each.
    pub fn fonts_used(&self) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = Vec::new();
        for (_, a) in self.file.archives() {
            if let Some(f) = a.obj().msg("char_properties").and_then(|cp| cp.str("font_name")) {
                match out.iter_mut().find(|o| o.0 == f) {
                    Some(o) => o.1 += 1,
                    None => out.push((f.to_string(), 1)),
                }
            }
        }
        out.sort();
        out
    }

    /// Format > Font > Replace Fonts…: every style using font `from` uses `to` instead.
    /// Returns how many styles changed.
    pub fn replace_font(&mut self, h: &mut History, from: &str, to: &str) -> usize {
        let ids: Vec<ObjId> = self
            .file
            .archives()
            .filter(|(_, a)| a.obj().msg("char_properties").and_then(|cp| cp.str("font_name")) == Some(from))
            .map(|(_, a)| a.id())
            .collect();
        for &id in &ids {
            self.touch(h, id);
            if let Some(cp) = self.file.obj_mut(id).and_then(|m| m.msg_mut("char_properties")) {
                cp.set_str("font_name", to);
            }
        }
        ids.len()
    }
}

impl Document {
    /// The presentation's named paragraph styles (Text inspector's paragraph style list):
    /// (style, name), the stylesheet's own named styles, sorted by name, no duplicates.
    pub fn paragraph_styles(&self) -> Vec<(ObjId, String)> {
        let mut v: Vec<(ObjId, String)> = self
            .file
            .objects_of("TSWP.ParagraphStyleArchive")
            .filter_map(|(id, m)| {
                let s = m.find_super("TSS.StyleArchive")?;
                if s.bool("is_variation") == Some(true) {
                    return None;
                }
                let n = s.str("name").filter(|n| !n.is_empty())?;
                // the presenter notes / comment styles aren't for slides
                if s.str("style_identifier").is_some_and(|i| i.starts_with("note-") || i.starts_with("stickyComment")) {
                    return None;
                }
                Some((id, n.to_string()))
            })
            .collect();
        v.sort_by(|a, b| a.1.cmp(&b.1));
        v.dedup_by(|a, b| a.1 == b.1);
        v
    }

    /// Named character styles (Text inspector > Character Styles).
    pub fn character_styles(&self) -> Vec<(ObjId, String)> {
        let mut v: Vec<(ObjId, String)> = self
            .file
            .objects_of("TSWP.CharacterStyleArchive")
            .filter_map(|(id, m)| {
                let s = m.find_super("TSS.StyleArchive")?;
                if s.bool("is_variation") == Some(true) {
                    return None;
                }
                Some((id, s.str("name").filter(|n| !n.is_empty())?.to_string()))
            })
            .collect();
        v.sort_by(|a, b| a.1.cmp(&b.1));
        v.dedup_by(|a, b| a.1 == b.1);
        v
    }

    /// Apply character style `style` (None = no character style) to bytes `a..b` of a storage.
    pub fn set_character_style(&mut self, h: &mut History, storage: ObjId, a: usize, b: usize, style: Option<ObjId>) -> bool {
        let raw = self.storage_text(storage);
        let (a, b) = (a.min(raw.len()), b.min(raw.len()));
        if a >= b || !raw.is_char_boundary(a) || !raw.is_char_boundary(b) {
            return false;
        }
        let (ua, ub) = (raw[..a].encode_utf16().count() as u64, raw[..b].encode_utf16().count() as u64);
        self.touch(h, storage);
        let Some(st) = self.file.obj_mut(storage) else { return false };
        let mut entries: Vec<(u64, Option<ObjId>)> = st
            .msg("table_char_style")
            .map(|t| t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0), e.ref_id("object"))).collect())
            .unwrap_or_default();
        entries.sort_by_key(|e| e.0);
        let after = entries.iter().filter(|e| e.0 <= ub).last().and_then(|e| e.1);
        entries.retain(|e| e.0 < ua || e.0 > ub);
        entries.push((ua, style));
        entries.push((ub, after));
        if !entries.iter().any(|e| e.0 == 0) {
            entries.push((0, None));
        }
        entries.sort_by_key(|e| e.0);
        entries.dedup_by(|x, y| x.0 == y.0);
        st.remove("table_char_style");
        let t = st.msg_mut_or_create("table_char_style");
        for (i, o) in entries {
            let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
            e.set_u64("character_index", i);
            if let Some(o) = o {
                e.set_ref("object", o);
            }
            t.push_msg("entries", e);
        }
        self.file.refresh_refs(storage);
        if let Some(f) = self.file.file_of(storage).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        true
    }

    /// Format > Advanced > Create Style: a new named paragraph style based on the one the text
    /// uses ("Body 2"…), applied to the text.
    pub fn create_paragraph_style(&mut self, h: &mut History, storage: ObjId) -> Option<String> {
        let base = self.file.obj(storage)?.msg("table_para_style")?.msgs("entries").next()?.ref_id("object")?;
        let base_name = self.paragraph_style_name(storage).unwrap_or_else(|| "Style".into());
        let existing: Vec<String> = self.paragraph_styles().into_iter().map(|s| s.1).collect();
        let name = (2..).map(|n| format!("{base_name} {n}")).find(|n| !existing.contains(n))?;
        let sheet = self.file.stylesheet_id();
        let touch: Vec<ObjId> = sheet.into_iter().collect();
        let v = self.structural(h, &touch, |k| k.new_style_variation(base)).ok()?;
        self.touch(h, v);
        if let Some(t) = self.file.obj_mut(v).and_then(|m| m.find_super_mut("TSS.StyleArchive")) {
            t.set_str("name", &name);
            t.set_bool("is_variation", false);
        }
        self.set_paragraph_style(h, storage, v);
        Some(name)
    }

    /// Give every paragraph of a text storage paragraph style `style` (their own overrides go).
    pub fn set_paragraph_style(&mut self, h: &mut History, storage: ObjId, style: ObjId) -> bool {
        self.touch(h, storage);
        let Some(st) = self.file.obj_mut(storage) else { return false };
        let t = st.msg_mut_or_create("table_para_style");
        if t.count("entries") == 0 {
            let mut e = Msg::new("TSWP.ObjectAttributeTable.ObjectAttribute");
            e.set_u64("character_index", 0);
            t.push_msg("entries", e);
        }
        for e in t.msgs_mut("entries") {
            e.set_ref("object", style);
        }
        self.file.refresh_refs(storage);
        if let Some(f) = self.file.file_of(storage).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        true
    }

    /// Name of the paragraph style at the start of a text storage ("Body", "Title"…), as the
    /// Text inspector's paragraph style button shows it (nearest named style in the chain).
    pub fn paragraph_style_name(&self, storage: ObjId) -> Option<String> {
        let st = self.file.obj(storage)?;
        let first = st.msg("table_para_style")?.msgs("entries").next()?.ref_id("object")?;
        crate::style::chain(&self.file, Some(first))
            .into_iter()
            .find_map(|m| m.find_super("TSS.StyleArchive").and_then(|s| s.str("name")).filter(|n| !n.is_empty()).map(str::to_string))
    }

    /// The theme's shape style presets (Format > Style "Shape Styles" grid), or its text box
    /// presets. They sit in the theme's TSD.ThemePresetsArchive extension.
    pub fn shape_style_presets(&self, textbox: bool) -> Vec<ObjId> {
        let Some((_, theme)) = self.file.first_of("KN.ThemeArchive") else { return Vec::new() };
        let Some(t) = theme.find_super("TSS.ThemeArchive") else { return Vec::new() };
        let ty = kn_format::schema::schema().type_idx("TSD.ThemePresetsArchive");
        let field = if textbox { "textbox_style_presets" } else { "shape_style_presets" };
        for f in &t.fields {
            let parsed = match &f.val {
                kn_format::Val::Msg(m) if m.type_name() == "TSD.ThemePresetsArchive" => Some(m.clone()),
                kn_format::Val::Bytes(b) => Msg::parse(b, ty).ok(),
                _ => None,
            };
            if let Some(m) = parsed {
                let mut ids = m.ref_ids(field);
                // themes define one text box style; the shape styles follow it in the grid
                if textbox {
                    ids.extend(m.ref_ids("shape_style_presets"));
                }
                if !ids.is_empty() && ids.iter().all(|i| self.file.obj(*i).is_some()) {
                    return ids;
                }
            }
        }
        Vec::new()
    }

    /// The theme's image style presets (Format > Style "Image Styles" grid).
    pub fn image_style_presets(&self) -> Vec<ObjId> {
        let Some((_, theme)) = self.file.first_of("KN.ThemeArchive") else { return Vec::new() };
        let Some(t) = theme.find_super("TSS.ThemeArchive") else { return Vec::new() };
        let ty = kn_format::schema::schema().type_idx("TSD.ThemePresetsArchive");
        for f in &t.fields {
            let parsed = match &f.val {
                kn_format::Val::Msg(m) if m.type_name() == "TSD.ThemePresetsArchive" => Some(m.clone()),
                kn_format::Val::Bytes(b) => Msg::parse(b, ty).ok(),
                _ => None,
            };
            if let Some(m) = parsed {
                let ids = m.ref_ids("image_style_presets");
                if !ids.is_empty() && ids.iter().all(|i| self.file.obj(*i).is_some()) {
                    return ids;
                }
            }
        }
        Vec::new()
    }

    /// Apply a theme shape style preset: the shape now uses that style (its own overrides go).
    pub fn apply_shape_style(&mut self, h: &mut History, drawable: ObjId, preset: ObjId) -> bool {
        self.touch(h, drawable);
        let Some(a) = self.file.archive_mut(drawable) else { return false };
        // pictures keep their style on the ImageArchive itself (R37: only ImageArchive.style changes)
        let o = a.obj_mut();
        let Some(sh) = (if o.find_super("TSD.ShapeArchive").is_some() { o.find_super_mut("TSD.ShapeArchive") } else { o.find_super_mut("TSD.ImageArchive") }) else { return false };
        sh.set_ref("style", preset);
        a.refresh_object_references();
        if let Some(f) = self.file.file_of(drawable).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        true
    }
}

pub(crate) fn apply_text_prop(style: &mut Msg, prop: &TextProp) {
    if prop.is_char() {
        let cp = style.msg_mut_or_create("char_properties");
        match prop {
            TextProp::Font(f) => cp.set_str("font_name", f),
            TextProp::Size(s) => cp.set_f32("font_size", s.clamp(1.0, 2000.0)),
            TextProp::Tracking(t) => cp.set_f32("tracking", t.clamp(-0.5, 2.0)),
            TextProp::Bold(b) => cp.set_bool("bold", *b),
            TextProp::Italic(b) => cp.set_bool("italic", *b),
            TextProp::Underline(b) => cp.set_enum("underline", if *b { "kSingleUnderline" } else { "kNoUnderline" }),
            TextProp::Strike(b) => cp.set_enum("strikethru", if *b { "kSingleStrikethru" } else { "kNoStrikethru" }),
            TextProp::Script(v) => cp.set_enum(
                "superscript",
                match v {
                    1 => "kSuperscript",
                    -1 => "kSubscript",
                    _ => "kNoScript",
                },
            ),
            TextProp::Caps(c) => {
                cp.set_bool("capitalization_uses_linguistics", *c != crate::Caps::Start);
                cp.set_enum(
                "capitalization",
                match c {
                    crate::Caps::All => "kAllCaps",
                    crate::Caps::Small => "kSmallCaps",
                    crate::Caps::Title | crate::Caps::Start => "kTitled",
                    crate::Caps::None => "kNoCaps",
                },
            )
            }
            TextProp::BaselineShift(v) => cp.set_f32("baseline_shift", v.clamp(-1000.0, 1000.0)),
            TextProp::Ligatures(v) => cp.set_enum(
                "ligatures",
                match v {
                    0 => "kRequiredLigatures",
                    2 => "kAllLigatures",
                    _ => "kStandardLigatures",
                },
            ),
            TextProp::Color(c) => {
                cp.remove("font_color");
                cp.push_msg("font_color", color_msg(*c));
                // Keynote also stores the text colour as a TSD fill (which wins when reading)
                let mut f = Msg::new("TSD.FillArchive");
                f.push_msg("color", color_msg(*c));
                cp.remove("tsd_fill");
                cp.push_msg("tsd_fill", f);
            }
            _ => {}
        }
        return;
    }
    if !style.has_field("para_properties") {
        return; // character styles carry no paragraph attributes
    }
    let pp = style.msg_mut_or_create("para_properties");
    match prop {
        TextProp::Align(a) => pp.set_enum(
            "alignment",
            match a {
                Align::Left => "TATvalue0",
                Align::Right => "TATvalue1",
                Align::Center => "TATvalue2",
                Align::Justify => "TATvalue3",
                Align::Natural => "TATvalue4",
            },
        ),
        TextProp::LineSpacing(v) => {
            let ls = pp.msg_mut_or_create("line_spacing");
            ls.set_enum("mode", "kRelativeLineSpacing");
            ls.set_f32("amount", *v);
        }
        TextProp::SpaceBefore(v) => pp.set_f32("space_before", v.clamp(0.0, 1000.0)),
        TextProp::SpaceAfter(v) => pp.set_f32("space_after", v.clamp(0.0, 1000.0)),
        TextProp::FirstIndent(v) => pp.set_f32("first_line_indent", v.clamp(-1000.0, 1000.0)),
        TextProp::LeftIndent(v) => pp.set_f32("left_indent", v.clamp(0.0, 1000.0)),
        TextProp::RightIndent(v) => pp.set_f32("right_indent", v.clamp(0.0, 1000.0)),
        _ => {}
    }
}
