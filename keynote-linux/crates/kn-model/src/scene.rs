//! Builds a [`SlideScene`] for a slide: layout (master) objects, then the slide's own objects.

use kn_format::{KeyFile, Msg, ObjId};

use crate::style;
use crate::text::{self, TextContext, TextDefaults};
use crate::types::*;

fn size_of(m: Option<&Msg>) -> Size {
    m.map(|s| Size { w: s.f32("width").unwrap_or(0.0), h: s.f32("height").unwrap_or(0.0) }).unwrap_or_default()
}

pub fn geometry(drawable: &Msg) -> Geometry {
    let g = drawable.msg("geometry");
    let pos = g.and_then(|g| g.msg("position"));
    let sz = size_of(g.and_then(|g| g.msg("size")));
    let flags = g.and_then(|g| g.u32("flags")).unwrap_or(3);
    Geometry {
        x: pos.and_then(|p| p.f32("x")).unwrap_or(0.0),
        y: pos.and_then(|p| p.f32("y")).unwrap_or(0.0),
        w: sz.w,
        h: sz.h,
        angle: g.and_then(|g| g.f32("angle")).unwrap_or(0.0),
        hflip: flags & 4 != 0,
        vflip: flags & 8 != 0,
        auto_width: flags & 1 == 0,
        auto_height: flags & 2 == 0,
    }
}

fn base(id: ObjId, d: &Msg, kind: DrawableKind) -> Drawable {
    Drawable {
        id,
        geom: geometry(d),
        opacity: 1.0,
        shadow: None,
        reflection: None,
        locked: d.bool("locked").unwrap_or(false),
        hyperlink: d.str("hyperlink_url").map(str::to_string),
        alt_text: d.str("accessibility_description").map(str::to_string),
        placeholder: None,
        from_template: false,
        kind,
    }
}

/// Build one drawable (recursively for groups). Returns None for objects that aren't drawables.
pub fn drawable(k: &KeyFile, id: ObjId, ctx: TextContext) -> Option<Drawable> {
    let m = k.obj(id)?;
    let tn = m.type_name();
    let d = m.find_super("TSD.DrawableArchive")?;
    match tn {
        "KN.PlaceholderArchive" | "TSWP.ShapeInfoArchive" | "TSD.ShapeArchive" | "TSD.ConnectionLineArchive" | "TSA.CaptionInfoArchive" => {
            let shape = m.find_super("TSD.ShapeArchive")?;
            let mut dr = base(id, d, DrawableKind::Unsupported(String::new()));
            let style_id = shape.ref_id("style");
            let g = style::graphic(k, style_id);
            let (w, h) = (dr.geom.w, dr.geom.h);
            let ps = shape.msg("pathsource");
            let path = ps.map(|p| crate::path::from_path_source(p, w, h)).unwrap_or_else(|| Path::rect(w, h));
            if let Some(ps) = ps {
                dr.geom.hflip ^= ps.bool("horizontalFlip").unwrap_or(false);
                dr.geom.vflip ^= ps.bool("verticalFlip").unwrap_or(false);
            }
            let is_line = !path.is_closed() || tn == "TSD.ConnectionLineArchive";
            // a drawable's own line ends override the style's
            let head = shape.msg("head_line_end").map(style::line_end).unwrap_or(g.head);
            let tail = shape.msg("tail_line_end").map(style::line_end).unwrap_or(g.tail);
            let info = m.find_super("TSWP.ShapeInfoArchive");
            let is_text_box = info.and_then(|i| i.bool("is_text_box")).unwrap_or(false);
            let text = info.and_then(|i| i.ref_id("owned_storage").or(i.ref_id("deprecated_storage"))).and_then(|sid| {
                let layout = style::text_layout(k, style_id, None);
                let defaults = TextDefaults { para_style: style::shape_paragraph_style(k, style_id) };
                let tb = text::text_box(k, sid, &defaults, layout, ctx)?;
                if tb.text.is_empty() && !is_text_box && tn != "KN.PlaceholderArchive" {
                    None
                } else {
                    Some(tb)
                }
            });
            dr.opacity = g.opacity;
            dr.shadow = g.shadow;
            dr.reflection = g.reflection;
            dr.placeholder = (tn == "KN.PlaceholderArchive").then(|| match m.enum_name("kind") {
                Some("kKindTitlePlaceholder") => PlaceholderKind::Title,
                Some("kKindBodyPlaceholder") => PlaceholderKind::Body,
                Some("kKindSlideNumberPlaceholder") => PlaceholderKind::SlideNumber,
                Some("kKindObjectPlaceholder") => PlaceholderKind::Object,
                _ => PlaceholderKind::Other,
            });
            let mut stroke = g.stroke;
            let mut fill = g.fill;
            if is_line {
                fill = Fill::None;
                if stroke.is_none() && tn == "TSD.ConnectionLineArchive" {
                    stroke = Some(Stroke {
                        color: Color::BLACK,
                        width: 1.0,
                        cap: LineCap::Butt,
                        join: LineJoin::Miter,
                        miter_limit: 4.0,
                        dash: vec![],
                        phase: 0.0,
                        brush: None,
                    });
                }
            }
            dr.kind = DrawableKind::Shape(ShapeData { path, fill, stroke, head, tail, text, is_line, is_text_box });
            Some(dr)
        }
        "TSD.ImageArchive" => {
            let style_id = m.ref_id("style");
            let g = style::graphic(k, style_id);
            let mask = m.ref_id("mask").and_then(|mid| {
                let mm = k.obj(mid)?;
                let md = mm.find_super("TSD.DrawableArchive")?;
                let mg = geometry(md);
                let path = mm
                    .msg("pathsource")
                    .map(|p| crate::path::from_path_source(p, mg.w, mg.h))
                    .unwrap_or_else(|| Path::rect(mg.w, mg.h));
                Some((mg, path))
            });
            let data_id = m
                .msg("adjustedImageData")
                .or(m.msg("data"))
                .and_then(|r| r.u64("identifier"));
            let equation = m.str("equation_source_text").or(m.str("equation_source_old")).map(str::to_string);
            let caption_box = |s: ObjId| text::text_box(k, s, &TextDefaults { para_style: None }, style::text_layout(k, None, None), ctx);
            let gallery = m
                .msg("gallery_info")
                .map(|g| {
                    g.ref_ids("items")
                        .into_iter()
                        .filter_map(|i| {
                            let it = k.obj(i)?;
                            Some(GalleryItem {
                                id: i,
                                data_id: it.msg("adjusted_image_data").or(it.msg("image_data")).and_then(|r| r.u64("identifier")),
                                caption: it.ref_id("caption_storage").map(|s| crate::text::plain_text(k, s)).unwrap_or_default(),
                                caption_box: it.ref_id("caption_storage").and_then(caption_box),
                                offset: it.msg("offset").map(|o| (o.f32("x").unwrap_or(0.0), o.f32("y").unwrap_or(0.0))).unwrap_or((0.0, 0.0)),
                                scale: it.f32("scale").filter(|s| *s > 0.0).unwrap_or(1.0),
                                description: it.str("accessibility_description").unwrap_or("").to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let gallery_caption_mode = m.msg("gallery_info").map(|g| match g.enum_name("caption_mode") {
                Some("GalleryCaptionModePerImage") => 1,
                Some("GalleryCaptionModeAllImages") => 2,
                _ => g.u32("caption_mode").unwrap_or(0).min(2) as u8,
            }).unwrap_or(0);
            let gallery_caption = m.msg("gallery_info").and_then(|g| g.ref_id("caption_storage")).and_then(caption_box);
            let web_video = m.msg("web_video_info").map(|w| WebVideo {
                url: w.str("url").unwrap_or("").to_string(),
                title: w.msg("attribution").and_then(|a| a.str("title")).unwrap_or("").to_string(),
                author: w.msg("attribution").and_then(|a| a.str("author_name")).unwrap_or("").to_string(),
            });
            let mut dr = base(
                id,
                d,
                DrawableKind::Image(ImageData {
                    data_id,
                    thumb_id: m.msg("thumbnailData").and_then(|r| r.u64("identifier")),
                    mask, stroke: g.stroke, natural: size_of(m.msg("naturalSize")), equation, gallery, gallery_shown: 0, gallery_caption_mode, gallery_caption, gallery_caption_id: None, web_video }),
            );
            let flags = m.u32("flags").unwrap_or(0);
            let _ = flags;
            dr.opacity = g.opacity;
            dr.shadow = g.shadow;
            dr.reflection = g.reflection;
            Some(dr)
        }
        "TSD.MovieArchive" => {
            let style_id = m.ref_id("style");
            let g = style::graphic(k, style_id);
            let mut dr = base(
                id,
                d,
                DrawableKind::Movie(MovieData {
                    movie_data: m.msg("movieData").and_then(|r| r.u64("identifier")),
                    poster_data: m
                        .msg("posterImageData")
                        .or(m.msg("audioOnlyImageData"))
                        .and_then(|r| r.u64("identifier")),
                    audio_only: m.bool("audioOnly").unwrap_or(false),
                    volume: m.f32("volume").unwrap_or(1.0),
                    loop_mode: m.u32("loop_option").or(m.u32("loopOptionAsInteger")).unwrap_or(0),
                    start: m.f32("startTime").unwrap_or(0.0),
                    end: m.f32("endTime").unwrap_or(0.0),
                    autoplay: m.bool("autoPlay").unwrap_or(false),
                    plays_across_slides: m.bool("playsAcrossSlides").unwrap_or(false),
                    poster_time: m.f32("posterTime").unwrap_or(0.0),
                    live: m.bool("is_live_video").unwrap_or(false),
                }),
            );
            dr.opacity = g.opacity;
            dr.shadow = g.shadow;
            Some(dr)
        }
        "TSD.GroupArchive" => {
            let children = m.ref_ids("children").into_iter().filter_map(|c| drawable(k, c, ctx)).collect();
            Some(base(id, d, DrawableKind::Group(children)))
        }
        "TST.TableInfoArchive" => {
            let t = crate::table::table(k, m).unwrap_or_default();
            Some(base(id, d, DrawableKind::Table(t)))
        }
        "TSCH.ChartDrawableArchive" => {
            let c = crate::chart::chart(k, m).unwrap_or_default();
            Some(base(id, d, DrawableKind::Chart(c)))
        }
        other => Some(base(id, d, DrawableKind::Unsupported(other.to_string()))),
    }
}

pub fn transition(slide: &Msg) -> Option<Transition> {
    let a = slide.msg("transition")?.msg("attributes")?;
    let aa = a.msg("animationAttributes");
    let effect = aa.and_then(|x| x.str("effect")).or(a.str("database_effect"))?.to_string();
    if effect == "none" || effect.is_empty() {
        return None;
    }
    Some(Transition {
        effect,
        duration: aa.and_then(|x| x.f64("duration")).or(a.f64("database_duration")).unwrap_or(1.0),
        delay: aa.and_then(|x| x.f64("delay")).unwrap_or(0.0),
        automatic: aa.and_then(|x| x.bool("is_automatic")).unwrap_or(false),
        direction: aa.and_then(|x| x.u32("direction")).unwrap_or(0),
        color: aa.and_then(|x| x.msg("color")).map(style::color),
    })
}

/// Effect options of a KN.BuildAttributesArchive.
pub fn build_params(a: &Msg) -> BuildParams {
    BuildParams {
        acceleration: a.u32("action_acceleration").unwrap_or(0),
        rotation_angle: a.f32("action_rotationAngle"),
        rotation_clockwise: a.u32("action_rotationDirection") == Some(31),
        scale: a.f32("action_scaleSize"),
        opacity: a.f32("action_colorAlpha"),
        motion_path: a.msg("action_motionPathSource").and_then(crate::path::raw_path_source).filter(|p| !p.els.is_empty()),
        bounce: a.bool("custom_bounce").unwrap_or(false),
        text_delivery: a.u32("custom_textDelivery").unwrap_or(0),
        delivery_option: a.u32("custom_deliveryOption").unwrap_or(0),
        repeat_count: a.u32("custom_action_repeatCount").unwrap_or(0),
        scale_amount: a.f32("custom_scale_amount"),
        travel_distance: a.f32("custom_travel_distance"),
        angle: a.f32("custom_angle"),
        include_endpoints: a.bool("custom_include_endpoints").unwrap_or(false),
        cursor: a.bool("custom_cursor"),
    }
}

pub fn builds(k: &KeyFile, slide: &Msg) -> (Vec<Build>, Vec<BuildStep>) {
    let mut builds = Vec::new();
    for bid in slide.ref_ids("builds") {
        let Some(b) = k.obj(bid) else { continue };
        let attrs = b.msg("attributes");
        let aa = attrs.and_then(|a| a.msg("animationAttributes"));
        builds.push(Build {
            id: bid,
            drawable: b.ref_id("drawable").unwrap_or(0),
            kind: aa
                .and_then(|x| x.str("animation_type"))
                .or(attrs.and_then(|a| a.str("database_animationType")))
                .unwrap_or("In")
                .to_string(),
            effect: aa
                .and_then(|x| x.str("effect"))
                .or(attrs.and_then(|a| a.str("database_effect")))
                .unwrap_or("apple:appear")
                .to_string(),
            delivery: b.str("delivery").unwrap_or("").to_string(),
            duration: aa.and_then(|x| x.f64("duration")).or(b.f64("duration")).unwrap_or(1.0),
            direction: aa.and_then(|x| x.u32("direction")).or(attrs.and_then(|a| a.u32("database_direction"))).unwrap_or(0),
            params: attrs.map(build_params).unwrap_or_default(),
        });
    }
    let mut steps = Vec::new();
    for cid in slide.ref_ids("buildChunks") {
        let Some(c) = k.obj(cid) else { continue };
        steps.push(BuildStep {
            build: c.ref_id("build").unwrap_or(0),
            delay: c.f64("delay").unwrap_or(0.0),
            duration: c.f64("duration").unwrap_or(0.0),
            automatic: c.bool("automatic").unwrap_or(false),
            // referent: 1 = timed from the previous build's end ("After"), 0 = from its start
            // ("With"). On Click steps always have 1 (every Keynote file we have).
            with_previous: c.bool("automatic").unwrap_or(false) && !c.bool("referent").unwrap_or(true),
            chunk: c.msg("build_chunk_identifier").and_then(|i| i.u32("build_chunk_id")).unwrap_or(1),
        });
    }
    if steps.is_empty() {
        steps = builds
            .iter()
            .map(|b| BuildStep { build: b.id, delay: 0.0, duration: b.duration, automatic: false, with_previous: false, chunk: 1 })
            .collect();
    }
    (builds, steps)
}

/// Layout objects drawn under the slide: everything on the template except placeholders and the
/// media/text placeholders that were copied into the slide (listed in sage_tag_to_info_map).
pub fn template_drawables(k: &KeyFile, template: ObjId, ctx: TextContext) -> Vec<Drawable> {
    let Some(t) = k.obj(template) else { return Vec::new() };
    let tagged: Vec<ObjId> = t.msgs("sage_tag_to_info_map").filter_map(|e| e.ref_id("info")).collect();
    t.ref_ids("drawables_z_order")
        .into_iter()
        .filter(|id| !tagged.contains(id) && k.type_name(*id) != "KN.PlaceholderArchive")
        .filter_map(|id| drawable(k, id, ctx))
        .map(|mut d| {
            d.from_template = true;
            d
        })
        .collect()
}

pub fn scene(k: &KeyFile, slide_id: ObjId, number: usize, size: Size) -> Option<SlideScene> {
    let slide = k.obj(slide_id)?;
    let ctx = TextContext { slide_number: number };
    let template = slide.ref_id("template_slide");
    let background = style::slide_fill(k, slide.ref_id("style"))
        .or_else(|| template.and_then(|t| k.obj(t)).and_then(|t| style::slide_fill(k, t.ref_id("style"))))
        .unwrap_or(Fill::Color(Color::WHITE));
    let mut drawables = template.map(|t| template_drawables(k, t, ctx)).unwrap_or_default();
    let z = slide.ref_ids("drawables_z_order");
    let order = if z.is_empty() { slide.ref_ids("owned_drawables") } else { z };
    for id in order {
        let Some(d) = drawable(k, id, ctx) else { continue };
        let g = d.geom;
        drawables.push(d);
        // shown object titles (above) and captions (below): TSA.CaptionInfoArchive text boxes
        let Some(dm) = k.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")) else { continue };
        let me = drawables.len() - 1;
        for (field, hidden, title) in [("title", "title_hidden", true), ("caption", "caption_hidden", false)] {
            if dm.bool(hidden) != Some(false) {
                continue;
            }
            // a gallery's caption shows the shown image's caption (or the one for all images)
            let gallery_cap = match (&drawables.get(me).map(|d| &d.kind), title) {
                (Some(DrawableKind::Image(im)), false) if !im.gallery.is_empty() && im.gallery_caption_mode > 0 => {
                    Some(if im.gallery_caption_mode == 2 { im.gallery_caption.clone() } else { im.gallery.first().and_then(|i| i.caption_box.clone()) })
                }
                _ => None,
            };
            let mut cd = if let Some(tb) = gallery_cap {
                let Some(cid) = dm.ref_id(field) else { continue };
                if let Some(DrawableKind::Image(im)) = drawables.get_mut(me).map(|d| &mut d.kind) {
                    im.gallery_caption_id = Some(cid);
                }
                let mut cd = drawables[me].clone();
                cd.id = cid;
                cd.opacity = 1.0;
                cd.shadow = None;
                cd.reflection = None;
                cd.kind = DrawableKind::Shape(ShapeData { path: Path::rect(g.w, 40.0), fill: Fill::None, stroke: None, head: None, tail: None, text: tb, is_line: false, is_text_box: true });
                cd
            } else {
                let Some(cid) = dm.ref_id(field).filter(|c| k.type_name(*c) == "TSA.CaptionInfoArchive") else { continue };
                let Some(cd) = drawable(k, cid, ctx) else { continue };
                cd
            };
            let lines = match &cd.kind {
                DrawableKind::Shape(s) => s.text.as_ref().map(|t| (t.text.lines().count().max(1) as f32, t.paragraphs.first().and_then(|p| p.runs.first()).map(|r| r.style.size).unwrap_or(20.0))),
                _ => None,
            };
            let (n, size) = lines.unwrap_or((1.0, 20.0));
            let hgt = n * size * 1.25 + 8.0;
            cd.geom.x = g.x;
            cd.geom.w = g.w;
            cd.geom.h = hgt;
            cd.geom.angle = 0.0;
            cd.geom.y = if title { g.y - hgt } else { g.y + g.h };
            drawables.push(cd);
        }
    }
    let (builds, build_order) = builds(k, slide);
    let motion = crate::motionbg::of_slide(k, slide);
    Some(SlideScene { slide: slide_id, number, size, background, drawables, transition: transition(slide), builds, build_order, motion })
}

impl SlideScene {
    /// Show image `index` of gallery `id`: its picture (with its pan and zoom) and its caption.
    pub fn show_gallery_image(&mut self, id: ObjId, index: usize) {
        let mut caption: Option<(ObjId, Option<TextBox>)> = None;
        for d in &mut self.drawables {
            if d.id != id {
                continue;
            }
            if let DrawableKind::Image(im) = &mut d.kind {
                let Some(it) = im.gallery.get(index.min(im.gallery.len().saturating_sub(1))).cloned() else { return };
                im.gallery_shown = index.min(im.gallery.len() - 1);
                im.data_id = it.data_id;
                im.thumb_id = None;
                if let Some(cid) = im.gallery_caption_id {
                    caption = Some((cid, if im.gallery_caption_mode == 2 { im.gallery_caption.clone() } else { it.caption_box.clone() }));
                }
            }
        }
        if let Some((cid, tb)) = caption {
            for d in &mut self.drawables {
                if d.id == cid {
                    if let DrawableKind::Shape(s) = &mut d.kind {
                        s.text = tb.clone();
                    }
                }
            }
        }
    }
}
