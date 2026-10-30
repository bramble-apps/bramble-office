//! Keynote's Dynamic Backgrounds (Format > Slide > Background > Dynamic).
//!
//! Storage (verified on Keynote 15.4): the slide style's fill is an image fill with the poster
//! frame (a still JPEG, so other readers still show something) plus the FillArchive extension
//! 200 `motionBackgroundFill`, which references a KN.MotionBackgroundStyleArchive. The theme lists
//! 18 presets (KN.ThemeArchive.motion_background_style_presets). An edited preset is a variation
//! (parent = the preset, is_variation) holding only the changed properties.

use std::collections::BTreeMap;

use kn_format::{KeyFile, Msg, ObjId};

use crate::edit::History;
use crate::style::{chain, color};
use crate::types::Color;
use crate::Document;

/// Double-valued properties (KN.MotionBackgroundStylePropertiesArchive).
pub const DOUBLES: &[&str] = &["color_blur_sigma", "noise_amplitude", "noise_octaves", "noise_scale", "noise_speed", "color_palette_offset", "zoom", "alpha", "border", "blend", "brightness", "border_lightness", "contrast", "damping", "density", "diffuse_power", "distortion", "entropy", "exposure", "filter", "fog_distance", "fog_power", "gamma", "grain_amount", "grain_intensity", "hue", "lightness", "line_alpha", "line_center", "line_fade", "line_fade_power", "line_length", "line_width", "noise_size_scale", "noise_size_weight", "noise_variance", "offset", "motion_background_opacity", "point_depth_alpha", "point_size", "radius", "refraction", "resolution", "saturation", "scale", "shadow_alpha", "shadow_angle", "shadow_offset", "shadow_scale", "shadow_value", "speed", "texture_power", "time", "amplitude", "envelope", "y_offset", "focus", "blur", "grain_speed", "angle", "mesh_line_width", "delta_time", "time_start", "color_contrast", "color_gamma", "color_saturation", "color_brightness", "color_hue", "color_vibrance", "noise_movement", "power", "threshold", "noise_scale_horizontal", "noise_scale_vertical", "noise_gain", "noise_lacunarity", "extrusion", "noise_offset", "color_hue_offset", "shadow_hue", "shadow_hue_offset", "shadow_saturation", "shadow_brightness", "shadow_intensity", "shadow_power"];
/// Integer-valued properties.
pub const INTS: &[&str] = &["random_number_seed", "pixels_per_division", "instance_count", "noise_octaves_iterations"];
pub const BOOLS: &[&str] = &["invert", "draw_points", "draw_lines", "draw_shadows", "symmetry", "lock_focus"];

#[derive(Clone, Debug, PartialEq)]
pub struct MotionBg {
    /// The style the slide uses (a preset or a variation of one).
    pub style: ObjId,
    /// The preset's name ("Rainbow", "Ocean Floor"…).
    pub name: String,
    /// Renderer id: KNGradientLinearBase, KNGradientTerrainBase, KNGradientTerrainFlyover,
    /// KNGradientDiffusionBase, KNWavesBase, KNWavesSymmetrical, KNGraphicSpheresBase.
    pub family: String,
    /// The "Colors" bar: gradient stops (fraction, colour).
    pub colors: Vec<(f32, Color)>,
    pub nums: BTreeMap<&'static str, f64>,
    pub flags: BTreeMap<&'static str, bool>,
    pub background_color: Option<Color>,
    pub blend_mode: Option<String>,
    /// Poster frame (still image) data id.
    pub poster: Option<u64>,
}

impl MotionBg {
    /// Same moving picture (whatever style object or poster it comes from): slides with the same
    /// background keep it playing across them.
    pub fn looks_same(&self, other: &MotionBg) -> bool {
        self.family == other.family
            && self.colors == other.colors
            && self.nums == other.nums
            && self.flags == other.flags
            && self.background_color == other.background_color
            && self.blend_mode == other.blend_mode
    }

    pub fn num(&self, name: &str, default: f64) -> f64 {
        self.nums.get(name).copied().unwrap_or(default)
    }
    pub fn flag(&self, name: &str) -> bool {
        self.flags.get(name).copied().unwrap_or(false)
    }
}

fn props(m: &Msg) -> Option<&Msg> {
    m.msg("slide_properties")
}

/// Read a motion background style, taking each property from the most specific style that sets it.
pub fn read(k: &KeyFile, style: ObjId) -> Option<MotionBg> {
    let ch = chain(k, Some(style));
    if ch.is_empty() || k.type_name(style) != "KN.MotionBackgroundStyleArchive" {
        return None;
    }
    let first = |f: &dyn Fn(&Msg) -> bool| ch.iter().copied().find(|m| props(m).is_some_and(|p| f(p)));
    let name = ch.iter().find_map(|m| m.find_super("TSS.StyleArchive").and_then(|s| s.str("name"))).unwrap_or("Dynamic").to_string();
    let family = first(&|p| p.has("background_name")).and_then(props).and_then(|p| p.str("background_name")).unwrap_or("KNGradientLinearBase").to_string();
    let colors = first(&|p| p.has("color_gradient"))
        .and_then(props)
        .and_then(|p| p.msg("color_gradient"))
        .and_then(|f| f.msg("gradient"))
        .map(|g| g.msgs("stops").map(|s| (s.f32("fraction").unwrap_or(0.0), s.msg("color").map(color).unwrap_or(Color::BLACK))).collect())
        .unwrap_or_default();
    let mut nums = BTreeMap::new();
    for &n in DOUBLES {
        if let Some(v) = first(&|p| p.has(n)).and_then(props).and_then(|p| p.f64(n)) {
            nums.insert(n, v);
        }
    }
    for &n in INTS {
        if let Some(v) = first(&|p| p.has(n)).and_then(props).and_then(|p| p.i64(n)) {
            nums.insert(n, v as f64);
        }
    }
    let mut flags = BTreeMap::new();
    for &n in BOOLS {
        if let Some(v) = first(&|p| p.has(n)).and_then(props).and_then(|p| p.bool(n)) {
            flags.insert(n, v);
        }
    }
    let background_color = first(&|p| p.has("background_color")).and_then(props).and_then(|p| p.msg("background_color")).map(color);
    let blend_mode = first(&|p| p.has("blend_mode")).and_then(props).and_then(|p| p.str("blend_mode")).map(str::to_string);
    let poster = ch.iter().find_map(|m| m.msg("poster_frame_data").and_then(|d| d.u64("identifier")));
    Some(MotionBg { style, name, family, colors, nums, flags, background_color, blend_mode, poster })
}

/// The motion style a slide fill message refers to (FillArchive extension 200).
pub fn fill_style(fill: &Msg) -> Option<ObjId> {
    fill.msg("motionBackgroundFill").and_then(|m| m.ref_id("motion_background_style"))
}

/// A slide's dynamic background (from its own style, else its layout's).
pub fn of_slide(k: &KeyFile, slide: &Msg) -> Option<MotionBg> {
    let from = |style: Option<ObjId>| -> Option<Option<ObjId>> {
        let ch = chain(k, style);
        // the most specific style that sets a fill decides (a plain fill turns Dynamic off)
        ch.iter().find_map(|m| m.msg("slide_properties").and_then(|p| p.msg("fill"))).map(fill_style)
    };
    let own = from(slide.ref_id("style"));
    let style = match own {
        Some(s) => s,
        None => slide.ref_id("template_slide").and_then(|t| k.obj(t)).and_then(|t| from(t.ref_id("style"))).flatten(),
    }?;
    read(k, style)
}

impl Document {
    /// The theme's dynamic background presets, in picker order: (style, name).
    pub fn motion_presets(&self) -> Vec<(ObjId, String)> {
        let Some((_, theme)) = self.file.first_of("KN.ThemeArchive") else { return Vec::new() };
        theme
            .ref_ids("motion_background_style_presets")
            .into_iter()
            .filter_map(|id| read(&self.file, id).map(|m| (id, m.name)))
            .collect()
    }

    pub fn motion_background(&self, slide: ObjId) -> Option<MotionBg> {
        of_slide(&self.file, self.file.obj(slide)?)
    }

    /// Format > Slide > Background > Dynamic: use motion style `style` on the slide. `poster` is
    /// a JPEG still of it (used when the style has none yet). None turns Dynamic off (white).
    pub fn set_motion_background(&mut self, h: &mut History, slide: ObjId, style: Option<ObjId>, poster: Option<Vec<u8>>) -> bool {
        let Some(style) = style else {
            return self.set_slide_background(h, slide, &crate::format::FillSpec::Color(Color::WHITE));
        };
        let Some(mb) = read(&self.file, style) else { return false };
        // the style's own still (a variation needs its own, not its preset's)
        let own_poster = self.file.obj(style).and_then(|m| m.msg("poster_frame_data")).and_then(|d| d.u64("identifier"));
        let poster_id = match own_poster.or(if poster.is_none() { mb.poster } else { None }) {
            Some(p) => Some(p),
            None => match poster {
                Some(bytes) => {
                    let sheet = self.file.stylesheet_id();
                    let touch: Vec<ObjId> = sheet.into_iter().chain([style]).collect();
                    let r: Result<u64, kn_format::Error> = self.structural(h, &touch, |k| {
                        let id = k.add_image_data(bytes, "motionBackground.jpeg")?;
                        if let Some(m) = k.obj_mut(style) {
                            m.remove("poster_frame_data");
                            m.push_msg("poster_frame_data", data_ref(id));
                        }
                        if let Some(f) = k.file_of(style).map(str::to_string) {
                            k.add_data_reference(&f, id, style);
                        }
                        Ok(id)
                    });
                    r.ok()
                }
                None => None,
            },
        };
        let size = self.slide_size();
        let mut fill = Msg::new("TSD.FillArchive");
        {
            let img = fill.msg_mut_or_create("image");
            img.set_enum("technique", "ScaleToFill");
            if let Some(pid) = poster_id {
                img.push_msg("imagedata", data_ref(pid));
            }
            let fs = img.msg_mut_or_create("fillsize");
            fs.set_f32("width", size.w);
            fs.set_f32("height", size.h);
            if let Some((_, c)) = mb.colors.first() {
                img.push_msg("referencecolor", crate::format::color_msg(*c));
            }
        }
        fill.msg_mut_or_create("motionBackgroundFill").set_ref("motion_background_style", style);
        let ok = self.set_slide_fill_msg(h, slide, fill);
        if let (true, Some(pid)) = (ok, poster_id) {
            // the slide style (in the stylesheet) now uses the poster data
            if let Some(st) = self.file.obj(slide).and_then(|s| s.ref_id("style")) {
                if let Some(f) = self.file.file_of(st).map(str::to_string) {
                    self.file.add_data_reference(&f, pid, st);
                }
            }
        }
        ok
    }

    /// Change one option of a slide's dynamic background. Edits the slide's own variation of
    /// the preset (created on first change, like Keynote: parent = preset, only changed values).
    pub fn set_motion_property(&mut self, h: &mut History, slide: ObjId, prop: MotionProp, poster: Option<Vec<u8>>) -> bool {
        let Some(cur) = self.motion_background(slide) else { return false };
        let is_var = self.file.obj(cur.style).and_then(|m| m.find_super("TSS.StyleArchive")).and_then(|s| s.bool("is_variation")) == Some(true);
        let style = if is_var {
            cur.style
        } else {
            let sheet = self.file.stylesheet_id();
            let touch: Vec<ObjId> = sheet.into_iter().collect();
            let Ok(v) = self.structural(h, &touch, |k| k.new_style_variation(cur.style)) else { return false };
            if let Some(m) = self.file.obj_mut(v) {
                m.remove("poster_frame_data");
            }
            // Keynote also lists motion-style variations in the 12.1 style set (verified)
            if let Some(sheet) = self.file.stylesheet_id() {
                if let Some(sm) = self.file.obj_mut(sheet) {
                    let vs = sm.msg_mut_or_create("styles_for_12_1");
                    vs.push_ref("styles", v);
                    let parent = cur.style;
                    let has = vs.msgs("parent_to_children_style_map").any(|e| e.ref_id("parent") == Some(parent));
                    if has {
                        if let Some(e) = vs.msgs_mut("parent_to_children_style_map").find(|e| e.ref_id("parent") == Some(parent)) {
                            e.push_ref("children", v);
                        }
                    } else {
                        let mut e = Msg::new("TSS.StylesheetArchive.StyleChildrenEntry");
                        e.set_ref("parent", parent);
                        e.push_ref("children", v);
                        vs.push_msg("parent_to_children_style_map", e);
                    }
                }
            }
            v
        };
        self.touch(h, style);
        if let Some(m) = self.file.obj_mut(style) {
            m.remove("poster_frame_data");
            let p = m.msg_mut_or_create("slide_properties");
            match &prop {
                MotionProp::Num(n, v) => {
                    if INTS.contains(n) {
                        p.set_i64(n, v.round() as i64);
                    } else {
                        p.set_f64(n, *v);
                    }
                }
                MotionProp::Background(c) => {
                    p.remove("background_color");
                    p.push_msg("background_color", crate::format::color_msg(*c));
                }
                MotionProp::Colors(stops) => {
                    p.remove("color_gradient");
                    let mut f = Msg::new("TSD.FillArchive");
                    {
                        let g = f.msg_mut_or_create("gradient");
                        g.set_enum("type", "Linear");
                        for (frac, c) in stops {
                            let mut st = Msg::new("TSD.GradientArchive.GradientStop");
                            st.push_msg("color", crate::format::color_msg(*c));
                            st.set_f32("fraction", *frac);
                            st.set_f32("inflection", 0.5);
                            g.push_msg("stops", st);
                        }
                        g.set_f32("opacity", 1.0);
                        g.set_bool("advancedGradient", false);
                    }
                    p.push_msg("color_gradient", f);
                }
            }
            let n = m.msg("slide_properties").map(|p| {
                let mut nums: Vec<u32> = p.fields.iter().map(|f| f.num).collect();
                nums.sort();
                nums.dedup();
                nums.len() as u64
            });
            m.set_u64("override_count", n.unwrap_or(0));
        }
        self.file.refresh_refs(style);
        // poster frames are per style: render a new one and point the slide at the variation
        self.set_motion_background(h, slide, Some(style), poster)
    }
}

fn data_ref(id: u64) -> Msg {
    let mut m = Msg::new("TSP.DataReference");
    m.set_u64("identifier", id);
    m
}

/// One edit to a dynamic background.
#[derive(Clone, Debug, PartialEq)]
pub enum MotionProp {
    Num(&'static str, f64),
    /// Waves: Background Color.
    Background(Color),
    Colors(Vec<(f32, Color)>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;

    fn deck(name: &str) -> Document {
        Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref").join(name)).unwrap()
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn reads_presets_and_variation() {
        let d = deck("R10-dynamic.key");
        let presets = d.motion_presets();
        assert_eq!(presets.len(), 18);
        assert_eq!(presets[0].1, "Rainbow");
        assert_eq!(presets[17].1, "Purple Vista");
        let s = d.slides();
        let first = d.motion_background(s[0].slide).unwrap();
        assert_eq!(first.family, "KNGradientLinearBase");
        assert_eq!(first.colors.len(), 6);
        let jade = d.motion_background(s[5].slide).unwrap();
        assert_eq!(jade.family, "KNGradientTerrainFlyover");
        // slide 19: Rainbow with angle 90 and speed 10, everything else inherited
        let v = d.motion_background(s[18].slide).unwrap();
        assert_eq!(v.name, "Rainbow");
        assert_eq!(v.num("angle", 0.0), 90.0);
        assert_eq!(v.num("speed", 0.0), 10.0);
        assert!((v.num("scale", 0.0) - 1.4205).abs() < 1e-3);
        assert!(d.scene(&s, 0).unwrap().motion.is_some());
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn set_preset_and_option_round_trip() {
        let jpeg = {
            let src = deck("R10-dynamic.key");
            let id = read(&src.file, src.motion_presets()[0].0).unwrap().poster.unwrap();
            src.file.data(id).unwrap().1.to_vec()
        };
        let mut d = deck("R10-standard.key");
        let s = d.slides();
        assert!(d.motion_background(s[0].slide).is_none());
        let presets = d.motion_presets();
        let ocean = presets.iter().find(|p| p.1 == "Ocean Floor").unwrap().0;
        let mut h = History::default();
        d.begin(&mut h, "Dynamic");
        assert!(d.set_motion_background(&mut h, s[0].slide, Some(ocean), Some(jpeg.clone())));
        d.end(&mut h);
        d.begin(&mut h, "Speed");
        assert!(d.set_motion_property(&mut h, s[0].slide, MotionProp::Num("noise_speed", 3.0), Some(jpeg.clone())));
        d.end(&mut h);
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let d2 = Document::from_file(k);
        let s2 = d2.slides();
        let m = d2.motion_background(s2[0].slide).unwrap();
        assert_eq!(m.name, "Ocean Floor");
        assert_eq!(m.num("noise_speed", 0.0), 3.0);
        // a variation of the preset, holding just the change
        let st = d2.file.obj(m.style).unwrap();
        assert_eq!(st.find_super("TSS.StyleArchive").unwrap().bool("is_variation"), Some(true));
        assert_eq!(st.u64("override_count"), Some(1));
        assert!(m.poster.is_some());
        // the still is in the fill for other readers
        assert!(matches!(d2.scene(&s2, 0).unwrap().background, crate::Fill::Image { .. }));
    }
}

#[cfg(test)]
mod data_tests {
    use crate::Document;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn inserted_movie_round_trips() {
        let mut d = Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R10-standard.key")).unwrap();
        let s = d.slides();
        let mut h = crate::edit::History::default();
        d.begin(&mut h, "Insert Movie");
        let id = d
            .insert_movie(&mut h, s[0].slide, crate::insert::NewMovie { bytes: vec![0u8; 1000], name: "clip.mp4".into(), poster: None, natural: Some((640.0, 360.0)), duration: 3.5, audio_only: false })
            .unwrap();
        d.end(&mut h);
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let d2 = Document::from_file(k);
        let s2 = d2.slides();
        let sc = d2.scene(&s2, 0).unwrap();
        let m = sc.drawables.iter().find(|x| x.id == id).unwrap();
        match &m.kind {
            crate::DrawableKind::Movie(mv) => {
                assert_eq!(mv.end, 3.5);
                assert_eq!(d2.file.data(mv.movie_data.unwrap()).unwrap().1.len(), 1000);
            }
            _ => panic!("not a movie"),
        }
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn replacing_data_round_trips() {
        let mut d = Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R10-dynamic.key")).unwrap();
        let id = super::read(&d.file, d.motion_presets()[0].0).unwrap().poster.unwrap();
        let bytes = d.file.data(id).unwrap().1.to_vec();
        let small = bytes[..].to_vec();
        let mut h = crate::edit::History::default();
        d.begin(&mut h, "Reduce");
        assert!(d.replace_data(&mut h, id, small.clone(), "poster small.jpeg"));
        d.end(&mut h);
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let (name, b) = k.data(id).unwrap();
        assert_eq!(b.len(), small.len());
        assert!(name.contains("poster_small"));
        d.undo(&mut h);
        assert_eq!(d.file.data(id).unwrap().1.len(), bytes.len());
    }
}
