//! Style lookup. Every style (TSS.StyleArchive at the bottom of its `super` chain) may have a
//! `parent`; a property is taken from the most specific style that sets it.

use kn_format::{KeyFile, Msg, ObjId};

use crate::types::*;

/// Style messages from most specific to root (cycle-safe).
pub fn chain(k: &KeyFile, id: Option<ObjId>) -> Vec<&Msg> {
    let mut out = Vec::new();
    let mut cur = id;
    while let Some(id) = cur {
        let Some(m) = k.obj(id) else { break };
        if out.len() > 32 || out.iter().any(|x: &&Msg| std::ptr::eq(*x, m)) {
            break;
        }
        out.push(m);
        cur = m.find_super("TSS.StyleArchive").and_then(|s| s.ref_id("parent"));
    }
    out
}

/// First value produced by `get` along the chain.
pub fn lookup<'a, T>(chain: &[&'a Msg], get: impl Fn(&'a Msg) -> Option<T>) -> Option<T> {
    chain.iter().find_map(|m| get(m))
}

pub fn color(m: &Msg) -> Color {
    Color {
        r: m.f32("r").unwrap_or(0.0),
        g: m.f32("g").unwrap_or(0.0),
        b: m.f32("b").unwrap_or(0.0),
        a: m.f32("a").unwrap_or(1.0),
    }
}

pub fn fill(m: &Msg) -> Fill {
    if let Some(c) = m.msg("color") {
        return Fill::Color(color(c));
    }
    if let Some(g) = m.msg("gradient") {
        let stops: Vec<(f32, Color)> = g
            .msgs("stops")
            .map(|s| (s.f32("fraction").unwrap_or(0.0), s.msg("color").map(color).unwrap_or(Color::BLACK)))
            .collect();
        let kind = if g.enum_name("type") == Some("Radial") {
            GradientKind::Radial
        } else {
            // Keynote stores radians (270° = 4.712389, R33 / R37); the model uses degrees
            GradientKind::Linear { angle: g.msg("anglegradient").and_then(|a| a.f32("gradientangle")).unwrap_or(0.0).to_degrees() }
        };
        let inflections = g.msgs("stops").map(|s| s.f32("inflection").unwrap_or(0.5)).collect();
        return Fill::Gradient { kind, stops, opacity: g.f32("opacity").unwrap_or(1.0), inflections, advanced: g.bool("advancedGradient").unwrap_or(false) };
    }
    if let Some(i) = m.msg("image") {
        let data_id = i.msg("imagedata").and_then(|d| d.u64("identifier"));
        if let Some(data_id) = data_id {
            let technique = match i.enum_name("technique") {
                Some("NaturalSize") => ImageFillTechnique::Natural,
                Some("Stretch") => ImageFillTechnique::Stretch,
                Some("Tile") => ImageFillTechnique::Tile,
                Some("ScaleToFit") => ImageFillTechnique::ScaleToFit,
                _ => ImageFillTechnique::ScaleToFill,
            };
            let size = i.msg("fillsize").and_then(|s| Some((s.f32("width")?, s.f32("height")?)));
            return Fill::Image { data_id, technique, tint: i.msg("tint").map(color), reference: i.msg("referencecolor").map(color), size };
        }
    }
    Fill::None
}

pub fn stroke(m: &Msg) -> Option<Stroke> {
    let pattern = m.msg("pattern");
    let ptype = pattern.and_then(|p| p.enum_name("type")).unwrap_or("TSDSolidPattern");
    if ptype == "TSDEmptyPattern" {
        return None;
    }
    let width = m.f32("width").unwrap_or(1.0);
    if width <= 0.0 {
        return None;
    }
    let dash = if ptype == "TSDPattern" {
        let p = pattern.unwrap();
        let n = p.u32("count").unwrap_or(0) as usize;
        p.f32s("pattern").into_iter().take(n).collect()
    } else {
        Vec::new()
    };
    Some(Stroke {
        color: m.msg("color").map(color).unwrap_or(Color::BLACK),
        width,
        cap: match m.enum_name("cap") {
            Some("RoundCap") => LineCap::Round,
            Some("SquareCap") => LineCap::Square,
            _ => LineCap::Butt,
        },
        join: match m.enum_name("join") {
            Some("RoundJoin") => LineJoin::Round,
            Some("BevelJoin") => LineJoin::Bevel,
            _ => LineJoin::Miter,
        },
        miter_limit: m.f32("miter_limit").unwrap_or(4.0),
        dash,
        phase: pattern.and_then(|p| p.f32("phase")).unwrap_or(0.0),
        brush: m.msg("smart_stroke").and_then(|s| s.str("stroke_name")).filter(|n| !n.is_empty()).map(str::to_string),
    })
}

pub fn shadow(m: &Msg) -> Option<Shadow> {
    if !m.bool("is_enabled").unwrap_or(false) {
        return None;
    }
    Some(Shadow {
        color: m.msg("color").map(color).unwrap_or(Color::BLACK),
        angle: m.f32("angle").unwrap_or(315.0),
        offset: m.f32("offset").unwrap_or(5.0),
        blur: m.i64("radius").unwrap_or(1) as f32,
        opacity: m.f32("opacity").unwrap_or(1.0),
        kind: match m.enum_name("type") {
            Some("TSDContactShadow") => 1,
            Some("TSDCurvedShadow") => 2,
            _ => 0,
        },
        height: m.msg("contactShadow").and_then(|c| c.f32("height")).unwrap_or(0.2),
        curve: m.msg("curvedShadow").and_then(|c| c.f32("curve")).unwrap_or(0.6),
    })
}

/// Graphic properties of a shape/media style (TSD.ShapeStylePropertiesArchive or
/// TSD.MediaStylePropertiesArchive), resolved through the chain.
pub struct Graphic {
    pub fill: Fill,
    pub stroke: Option<Stroke>,
    pub opacity: f32,
    pub shadow: Option<Shadow>,
    pub reflection: Option<f32>,
    pub head: Option<LineEnd>,
    pub tail: Option<LineEnd>,
}

fn graphic_props(m: &Msg) -> Option<&Msg> {
    // TSWP.ShapeStyleArchive -> super (TSD.ShapeStyleArchive) -> shape_properties
    if let Some(s) = m.find_super("TSD.ShapeStyleArchive") {
        return s.msg("shape_properties");
    }
    if let Some(s) = m.find_super("TSD.MediaStyleArchive") {
        return s.msg("media_properties");
    }
    None
}

pub fn line_end(m: &Msg) -> Option<LineEnd> {
    let id = m.str("identifier").unwrap_or("").to_string();
    if id.is_empty() || id == "none" {
        return None;
    }
    let path = m.msg("path").map(crate::path::from_tsp_path).unwrap_or_default();
    let ep = m.msg("end_point").map(|p| Point::new(p.f32("x").unwrap_or(0.0), p.f32("y").unwrap_or(0.0)));
    // no is_filled: closed shapes are filled ("simple arrow"), open ones stroked ("open arrow")
    let closed = path.els.is_empty() || path.els.iter().any(|e| matches!(e, crate::PathEl::Close));
    Some(LineEnd { path, end_point: ep.unwrap_or_default(), filled: m.bool("is_filled").unwrap_or(closed), id })
}

pub fn graphic(k: &KeyFile, style: Option<ObjId>) -> Graphic {
    let ch = chain(k, style);
    let props: Vec<&Msg> = ch.iter().filter_map(|m| graphic_props(m)).collect();
    Graphic {
        fill: lookup(&props, |p| p.msg("fill").map(fill)).unwrap_or(Fill::None),
        stroke: lookup(&props, |p| p.msg("stroke").map(stroke)).flatten(),
        opacity: lookup(&props, |p| p.f32("opacity")).unwrap_or(1.0),
        shadow: lookup(&props, |p| p.msg("shadow").map(shadow)).flatten(),
        reflection: lookup(&props, |p| p.msg("reflection").map(|r| r.f32("opacity"))).flatten(),
        head: lookup(&props, |p| p.msg("head_line_end").map(line_end)).flatten(),
        tail: lookup(&props, |p| p.msg("tail_line_end").map(line_end)).flatten(),
    }
}

/// TSWP shape (text layout) properties: vertical alignment, padding, shrink, columns.
pub struct TextLayout {
    pub valign: VAlign,
    pub padding: [f32; 4],
    pub shrink_to_fit: bool,
    pub columns: u32,
    pub column_gap: f32,
}

pub fn text_layout(k: &KeyFile, style: Option<ObjId>, overrides: Option<&Msg>) -> TextLayout {
    let ch = chain(k, style);
    let mut props: Vec<&Msg> = Vec::new();
    if let Some(o) = overrides {
        props.push(o);
    }
    props.extend(ch.iter().filter_map(|m| if m.type_name() == "TSWP.ShapeStyleArchive" { m.msg("shape_properties") } else { None }));
    let valign = match lookup(&props, |p| p.enum_name("vertical_alignment")) {
        Some("kFrameAlignMiddle") => VAlign::Middle,
        Some("kFrameAlignBottom") => VAlign::Bottom,
        _ => VAlign::Top,
    };
    let padding = lookup(&props, |p| {
        p.msg("padding").map(|pd| {
            [
                pd.f32("left").unwrap_or(0.0),
                pd.f32("top").unwrap_or(0.0),
                pd.f32("right").unwrap_or(0.0),
                pd.f32("bottom").unwrap_or(0.0),
            ]
        })
    })
    .unwrap_or([4.0; 4]);
    let cols = lookup(&props, |p| p.msg("columns"));
    let (columns, column_gap) = cols
        .and_then(|c| c.msg("equal_columns"))
        .map(|e| (e.u32("count").unwrap_or(1).max(1), e.f32("gap").unwrap_or(0.0)))
        .unwrap_or((1, 0.0));
    TextLayout {
        valign,
        padding,
        shrink_to_fit: lookup(&props, |p| p.bool("shrink_to_fit")).unwrap_or(false),
        columns,
        column_gap,
    }
}

/// Paragraph style id that a shape style gives to its text by default.
pub fn shape_paragraph_style(k: &KeyFile, style: Option<ObjId>) -> Option<ObjId> {
    let ch = chain(k, style);
    lookup(&ch, |m| if m.type_name() == "TSWP.ShapeStyleArchive" { m.msg("shape_properties")?.ref_id("paragraph_style") } else { None })
}

/// Slide background fill (KN.SlideStyleArchive chain).
pub fn slide_fill(k: &KeyFile, style: Option<ObjId>) -> Option<Fill> {
    let ch = chain(k, style);
    lookup(&ch, |m| m.msg("slide_properties").and_then(|p| p.msg("fill")).map(fill))
}

// ---------------------------------------------------------------------------------- text styles

/// Apply the character properties set in `p` on top of `cs`.
pub fn apply_char_props(cs: &mut CharStyle, p: &Msg) {
    if let Some(v) = p.str("font_name") {
        cs.font_name = v.to_string();
    }
    if let Some(v) = p.f32("font_size") {
        cs.size = v;
    }
    if let Some(v) = p.bool("bold") {
        cs.bold = v;
    }
    if let Some(v) = p.bool("italic") {
        cs.italic = v;
    }
    // Keynote stores the text colour both as font_color and as a TSD fill; the fill wins.
    if let Some(c) = p.msg("font_color") {
        cs.color = color(c);
    }
    if let Some(Fill::Color(c)) = p.msg("tsd_fill").map(fill) {
        cs.color = c;
    }
    if let Some(v) = p.enum_name("underline") {
        cs.underline = v != "kNoUnderline";
    }
    if let Some(v) = p.enum_name("strikethru") {
        cs.strike = v != "kNoStrikethru";
    }
    if let Some(v) = p.enum_name("superscript") {
        cs.script = match v {
            "kSuperscript" => 1,
            "kSubscript" => -1,
            _ => 0,
        };
    }
    if let Some(v) = p.f32("baseline_shift") {
        cs.baseline_shift = v;
    }
    if let Some(v) = p.enum_name("capitalization") {
        cs.caps = match v {
            "kAllCaps" => Caps::All,
            "kSmallCaps" => Caps::Small,
            "kTitled" if p.bool("capitalization_uses_linguistics") == Some(false) => Caps::Start,
            "kTitled" => Caps::Title,
            _ => Caps::None,
        };
    }
    if let Some(v) = p.f32("tracking") {
        cs.tracking = v;
    }
    if p.bool("background_color_null") == Some(true) {
        cs.background = None;
    } else if let Some(c) = p.msg("background_color") {
        cs.background = Some(color(c));
    }
    if p.bool("shadow_null") == Some(true) {
        cs.shadow = None;
    } else if let Some(s) = p.msg("shadow") {
        cs.shadow = shadow(s);
    }
    if p.bool("tsd_stroke_null") == Some(true) {
        cs.outline = None;
    } else if let Some(s) = p.msg("tsd_stroke").and_then(stroke) {
        cs.outline = Some((s.color, s.width));
    }
}

/// Resolve the character style of a paragraph style + optional character style.
pub fn char_style(k: &KeyFile, para_style: Option<ObjId>, char_style: Option<ObjId>) -> CharStyle {
    let mut cs = CharStyle::default();
    for m in chain(k, para_style).iter().rev() {
        if let Some(p) = m.msg("char_properties") {
            apply_char_props(&mut cs, p);
        }
    }
    for m in chain(k, char_style).iter().rev() {
        if let Some(p) = m.msg("char_properties") {
            apply_char_props(&mut cs, p);
        }
    }
    cs
}

pub struct ParaProps {
    pub align: Align,
    pub line_spacing: LineSpacing,
    pub space_before: f32,
    pub space_after: f32,
    pub left_indent: f32,
    pub right_indent: f32,
    pub first_line_indent: f32,
    pub list_style: Option<ObjId>,
    pub rtl: bool,
}

pub fn para_props(k: &KeyFile, para_style: Option<ObjId>) -> ParaProps {
    let ch = chain(k, para_style);
    let props: Vec<&Msg> = ch.iter().filter_map(|m| m.msg("para_properties")).collect();
    para_props_of(&props)
}

/// Paragraph properties from a list of para_properties messages, nearest first.
pub fn para_props_of(props: &[&Msg]) -> ParaProps {
    let props = props.to_vec();
    let align = match lookup(&props, |p| p.enum_name("alignment")) {
        Some("TATvalue1") => Align::Right,
        Some("TATvalue2") => Align::Center,
        Some("TATvalue3") => Align::Justify,
        Some("TATvalue4") => Align::Natural,
        _ => Align::Left,
    };
    let line_spacing = lookup(&props, |p| {
        let ls = p.msg("line_spacing")?;
        if ls.fields.is_empty() {
            return None;
        }
        let amount = ls.f32("amount").unwrap_or(1.0);
        Some(match ls.enum_name("mode") {
            Some("kMinimumLineSpacing") => LineSpacing::AtLeast(amount),
            Some("kExactLineSpacing") => LineSpacing::Exactly(amount),
            Some("kMaximumLineSpacing") => LineSpacing::AtMost(amount),
            Some("kSpaceBetweenLineSpacing") => LineSpacing::Between(amount),
            _ => LineSpacing::Relative(amount),
        })
    })
    .unwrap_or(LineSpacing::Relative(1.0));
    ParaProps {
        align,
        line_spacing,
        space_before: lookup(&props, |p| p.f32("space_before")).unwrap_or(0.0),
        space_after: lookup(&props, |p| p.f32("space_after")).unwrap_or(0.0),
        left_indent: lookup(&props, |p| p.f32("left_indent")).unwrap_or(0.0),
        right_indent: lookup(&props, |p| p.f32("right_indent")).unwrap_or(0.0),
        first_line_indent: lookup(&props, |p| p.f32("first_line_indent")).unwrap_or(0.0),
        list_style: lookup(&props, |p| p.ref_id("list_style")),
        rtl: lookup(&props, |p| p.enum_name("writing_direction")) == Some("kWritingDirectionRightToLeft"),
    }
}
