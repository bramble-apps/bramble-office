//! Per-object build animations for the slideshow.
//!
//! A slide's *state* after `n` build steps says which objects are hidden and how completed actions
//! moved/rotated/scaled/faded them. To animate a group of steps (a click plus the builds that follow
//! it automatically), the objects the group animates are rendered once into their own layer
//! textures and everything else into static strata (keeping the z-order), all *before* the group
//! starts. Playing the group only draws textured meshes, so no CPU rendering happens mid-animation.

use std::collections::{HashMap, HashSet};
use std::f32::consts::{PI, TAU};

use egui::{pos2, vec2, Color32, Mesh, Painter, Pos2, Rect, TextureHandle, TextureId, TextureOptions, Vec2};
use kn_format::ObjId;
use kn_model::{Build, Document, Drawable, DrawableKind, Geometry, PathEl, SlideScene};
use kn_render::{tiny_skia as sk, Renderer};

use crate::textures::{color_image, guarded};

// ---------------------------------------------------------------------------------- effects

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    In,
    Out,
    Action,
    /// Starts (true) or stops (false) a movie or sound; doesn't change visibility.
    Media(bool),
    /// An image gallery's next image (its Action build: one step per image after the first).
    Gallery,
    Other,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fx {
    Appear,
    Dissolve,
    Move { fly: bool },
    Blur,
    LineDraw,
    Wipe,
    Iris,
    Pop,
    Scale { big: bool },
    Drop,
    /// Keynote's Bounce (KLNBouncy): hops in from the side.
    Bouncy,
    /// Keynote's Anvil (BUKAnvil): falls hard and lands.
    Anvil,
    FadeMove,
    FadeScale,
    Spin,
    Twirl,
    Compress,
    Flip,
    /// Keynote's Keyboard (apple:keyboard): text typed letter by letter behind a cursor.
    Keyboard,
    /// Keynote's Cube (apple:bc-3D-cube): turns in as a cube's face.
    Cube,
    /// Keynote's Flip (apple:bc-flip): turns over about its vertical axis.
    FlipOver,
    // actions
    Path,
    Rotate,
    ScaleTo,
    Opacity,
    Pulse,
    Blink,
    Bounce,
    Jiggle,
    PopAction,
    FlipAction,
    None,
}

/// Keynote's stored effect id without prefixes/suffixes, e.g. "apple:move in character" -> "move in".
pub fn effect_name(effect: &str) -> String {
    let e = effect.strip_prefix("com.apple.iWork.Keynote.").unwrap_or_else(|| effect.split_once(':').map(|x| x.1).unwrap_or(effect));
    let mut e = e.to_lowercase().replace(['-', '_'], " ");
    if let Some(x) = e.strip_prefix("bc ") {
        e = x.to_string();
    }
    for suf in [" character", " word", " line", " object"] {
        if let Some(x) = e.strip_suffix(suf) {
            e = x.to_string();
        }
    }
    e
}

pub fn kind_of(b: &Build) -> Kind {
    let e = effect_name(&b.effect);
    if e.contains("movie") || e.contains("audio") || e.contains("video") {
        return Kind::Media(!(e.contains("stop") || e.contains("pause")));
    }
    if e.contains("gallery") || b.kind == "Content" {
        return Kind::Gallery;
    }
    match b.kind.as_str() {
        "In" => Kind::In,
        "Out" => Kind::Out,
        "Action" => Kind::Action,
        _ => Kind::Other,
    }
}

pub fn fx_of(b: &Build) -> Fx {
    let e = effect_name(&b.effect);
    let has = |s: &str| e.contains(s);
    if b.kind == "Action" {
        return if has("motion") || has("path") || e == "action move" {
            Fx::Path
        } else if has("rotat") {
            Fx::Rotate
        } else if has("opacity") {
            Fx::Opacity
        } else if has("pulse") {
            Fx::Pulse
        } else if has("blink") {
            Fx::Blink
        } else if has("bounce") {
            Fx::Bounce
        } else if has("jiggle") {
            Fx::Jiggle
        } else if has("pop") {
            Fx::PopAction
        } else if has("flip") {
            Fx::FlipAction
        } else if has("scale") {
            Fx::ScaleTo
        } else {
            Fx::None
        };
    }
    if has("linedraw") || has("line draw") {
        Fx::LineDraw
    } else if has("disappear") || has("appear") {
        Fx::Appear
    } else if has("fade and move") || has("drift") || has("shift") {
        Fx::FadeMove
    } else if has("fade and scale") || has("expand") || has("fromdarkness") {
        // (Fade and Scale is stored as "com.apple.iWork.Keynote.FromDarkness", R43)
        Fx::FadeScale
    } else if has("fly") || has("sidezoom") {
        // (Fly In is "apple:sidezoom", R43: not a scale)
        Fx::Move { fly: true }
    } else if has("move") || e == "slide" {
        Fx::Move { fly: false }
    } else if has("blur") {
        Fx::Blur
    } else if has("radial") || has("iris") {
        Fx::Iris
    } else if has("wipe") {
        Fx::Wipe
    } else if has("pop") {
        Fx::Pop
    } else if has("zoom big") || has("scale big") {
        Fx::Scale { big: true }
    } else if has("zoom") || has("scale") {
        Fx::Scale { big: false }
    } else if has("klnbouncy") {
        Fx::Bouncy
    } else if has("anvil") {
        Fx::Anvil
    } else if has("drop") || has("bounc") {
        Fx::Drop
    } else if has("spin") || has("orbital") {
        Fx::Spin
    } else if has("twirl") || has("twist") || has("swoosh") {
        Fx::Twirl
    } else if has("compress") || has("squish") {
        Fx::Compress
    } else if e == "keyboard" {
        Fx::Keyboard
    } else if e == "3d cube" {
        Fx::Cube
    } else if e == "flip" {
        Fx::FlipOver
    } else if has("flip") || has("cube") || has("pivot") || has("blinds") {
        Fx::Flip
    } else {
        Fx::Dissolve
    }
}

/// Travel direction of a build (unit vector, y down). Keynote stores 11 left-to-right,
/// 12 right-to-left, 13 top-to-bottom, 14 bottom-to-top, 21..24 diagonals (text effects use 1xx).
pub fn dir_vec(dir: u32, default: Vec2) -> Vec2 {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    match dir % 100 {
        11 => vec2(1.0, 0.0),
        12 => vec2(-1.0, 0.0),
        13 => vec2(0.0, 1.0),
        14 => vec2(0.0, -1.0),
        21 => vec2(s, s),
        22 => vec2(-s, s),
        23 => vec2(s, -s),
        24 => vec2(-s, -s),
        _ => default,
    }
}

/// Keynote's Bounce at progress `t`: (share of the way in from off the slide still to go,
/// height above its place as a share of the slide height, squash 0..1).
fn bouncy(t: f32) -> (f32, f32, f32) {
    // (start, landing, apex time, apex height, travel share at start, at landing)
    const HOPS: [(f32, f32, f32, f32, f32, f32); 3] = [(0.0, 0.29, 0.0, 0.62, 1.0, 0.66), (0.333, 0.667, 0.5, 0.47, 0.66, 0.08), (0.667, 0.95, 0.808, 0.17, 0.08, 0.0)];
    let t = t.clamp(0.0, 1.0);
    let mut out: (f32, f32, f32) = (0.0, 0.0, 0.0);
    for (u0, u1, a, hgt, x0, x1) in HOPS {
        if t >= u0 && t < u1 {
            let w = (t - a) / (u1 - a);
            out = (x0 + (x1 - x0) * (t - u0) / (u1 - u0), (hgt * (1.0 - w * w)).max(0.0), 0.0);
        }
    }
    for (land, amount) in [(0.31, 0.4), (0.667, 0.3), (0.95, 0.15)] {
        let d = (t - land).abs() / 0.035;
        if d < 1.0 {
            out.2 = out.2.max(amount * (1.0 - d));
        }
    }
    out
}

/// How much Fade and Scale changes the size: custom_scale_amount is a percentage (Keynote's
/// default 70 starts the object at ~0.79 of its size, R43).
fn fade_scale_k(b: &Build) -> f32 {
    let a = b.params.scale_amount.unwrap_or(70.0);
    let a = if a > 1.0 { a / 100.0 } else { a };
    0.3 * a.clamp(0.0, 3.0)
}

/// Keyboard build: when the k-th of n letters is typed (share of the build's time, R43 movie:
/// evenly from 0.21 to 0.78).
fn keyboard_at(k: usize, n: usize) -> f32 {
    0.21 + 0.57 * k as f32 / n.max(1) as f32
}

/// Which way a spin build turns into place: -1 clockwise (Keynote's default), 1 counterclockwise
/// (direction 32, as action_rotationDirection numbers them).
fn spin_sign(b: &Build) -> f32 {
    if b.direction % 100 == 32 { 1.0 } else { -1.0 }
}

pub fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Move In with Bounce (R43 movies, from the left and from the bottom alike): almost even speed
/// until ~0.68, about 4% past its place at ~0.73, settled by ~0.93.
fn move_bounce(t: f32) -> f32 {
    const ARRIVE: f32 = 0.68;
    let t = t.clamp(0.0, 1.0);
    if t < ARRIVE {
        return t / ARRIVE;
    }
    let u = (t - ARRIVE) / 0.27;
    if u >= 1.0 {
        return 1.0;
    }
    1.0 + 0.38 * u * (1.0 - u).powi(3)
}

/// Build acceleration (0 none, 1 ease in, 2 ease out, 3 both).
fn accel(a: u32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match a {
        1 => t * t,
        2 => 1.0 - (1.0 - t) * (1.0 - t),
        3 => smooth(t),
        _ => t,
    }
}

// ---------------------------------------------------------------------------------- states

/// Accumulated result of completed actions on one object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    pub dx: f32,
    pub dy: f32,
    /// Radians, clockwise on screen.
    pub rot: f32,
    pub scale: f32,
    pub opacity: Option<f32>,
}

impl Default for Xform {
    fn default() -> Self {
        Xform { dx: 0.0, dy: 0.0, rot: 0.0, scale: 1.0, opacity: None }
    }
}

/// Motion path of a Move action as a polyline (relative to its start), with cumulative lengths.
fn motion_polyline(b: &Build) -> Vec<(Vec2, f32)> {
    let Some(p) = &b.params.motion_path else { return Vec::new() };
    let pts = flatten(&p.els);
    let Some(first) = pts.first().copied() else { return Vec::new() };
    let mut out = Vec::with_capacity(pts.len());
    let mut len = 0.0;
    let mut prev = first;
    for q in pts {
        len += (q - prev).length();
        out.push((q - first, len));
        prev = q;
    }
    out
}

fn along(poly: &[(Vec2, f32)], f: f32) -> Vec2 {
    let Some(last) = poly.last() else { return Vec2::ZERO };
    let total = last.1;
    if total <= 0.0 {
        return last.0;
    }
    let want = f.clamp(0.0, 1.0) * total;
    for w in poly.windows(2) {
        let ((a, la), (b, lb)) = (w[0], w[1]);
        if want <= lb {
            let k = if lb > la { (want - la) / (lb - la) } else { 0.0 };
            return a + (b - a) * k;
        }
    }
    last.0
}

/// Flatten a path (curves sampled) into points.
fn flatten(els: &[PathEl]) -> Vec<Vec2> {
    let mut pts = Vec::new();
    let mut cur = vec2(0.0, 0.0);
    let v = |p: kn_model::Point| vec2(p.x, p.y);
    for e in els {
        match *e {
            PathEl::Move(p) | PathEl::Line(p) => {
                cur = v(p);
                pts.push(cur);
            }
            PathEl::Quad(a, p) => {
                let (a, p) = (v(a), v(p));
                for i in 1..=16 {
                    let t = i as f32 / 16.0;
                    let u = 1.0 - t;
                    pts.push(cur * (u * u) + a * (2.0 * u * t) + p * (t * t));
                }
                cur = p;
            }
            PathEl::Cubic(a, b, p) => {
                let (a, b, p) = (v(a), v(b), v(p));
                for i in 1..=24 {
                    let t = i as f32 / 24.0;
                    let u = 1.0 - t;
                    pts.push(cur * (u * u * u) + a * (3.0 * u * u * t) + b * (3.0 * u * t * t) + p * (t * t * t));
                }
                cur = p;
            }
            PathEl::Close => {
                if let Some(f) = pts.first().copied() {
                    pts.push(f);
                }
            }
        }
    }
    pts
}

/// Which objects are hidden after `step` build steps, and where completed actions left the others.
#[derive(Clone, Default, Debug)]
pub struct State {
    pub hidden: HashSet<ObjId>,
    pub xf: HashMap<ObjId, Xform>,
    /// Text objects built by paragraph: the range of (non-empty) paragraphs shown.
    pub paras: HashMap<ObjId, (usize, usize)>,
    /// By Highlighted Paragraph: paragraphs before this one are dimmed.
    pub dim: HashMap<ObjId, usize>,
    /// Image galleries: the image shown (their build steps move it on).
    pub gallery: HashMap<ObjId, usize>,
}

impl State {
    /// The scene as this state shows it (galleries on their current image), if that differs.
    pub fn scene_for(&self, scene: &SlideScene) -> Option<SlideScene> {
        if self.gallery.is_empty() {
            return None;
        }
        let mut s = scene.clone();
        for (id, k) in &self.gallery {
            s.show_gallery_image(*id, *k);
        }
        Some(s)
    }
}

impl State {
    pub fn xform(&self, id: ObjId) -> Xform {
        self.xf.get(&id).copied().unwrap_or_default()
    }
}

fn build_of<'a>(scene: &'a SlideScene, i: usize) -> Option<&'a Build> {
    let s = scene.build_order.get(i)?;
    scene.builds.iter().find(|b| b.id == s.build)
}

/// Apply a finished action to an object's accumulated transform.
fn finish_action(x: &mut Xform, b: &Build) {
    match fx_of(b) {
        Fx::Path => {
            if let Some((d, _)) = motion_polyline(b).last() {
                x.dx += d.x;
                x.dy += d.y;
            }
        }
        Fx::Rotate => {
            let a = b.params.rotation_angle.unwrap_or(90.0).to_radians();
            x.rot += if b.params.rotation_clockwise { a } else { -a };
        }
        Fx::ScaleTo => x.scale *= b.params.scale.unwrap_or(1.5).max(0.01),
        Fx::Opacity => x.opacity = Some(b.params.opacity.unwrap_or(0.5).clamp(0.0, 1.0)),
        _ => {}
    }
}

fn non_empty_paras(d: &Drawable) -> usize {
    match &d.kind {
        DrawableKind::Shape(s) => s.text.as_ref().map(|t| t.paragraphs.iter().filter(|p| !t.text[p.start..p.end].trim().is_empty()).count()).unwrap_or(0),
        _ => 0,
    }
}

/// Paragraph range delivered by chunk `j` (0-based) of a build delivered in `chunks` chunks.
fn chunk_range(n: usize, chunks: usize, j: usize) -> (usize, usize) {
    let c = chunks.max(1);
    (j * n / c, ((j + 1) * n / c).min(n))
}

/// The (non-empty) paragraph range step `j` of `c` delivers: even split, or for By Paragraph
/// Group whole top-level paragraphs with their indented ones.
fn span_of(scene: &SlideScene, i: usize, j: usize, c: usize, n: usize) -> (usize, usize) {
    let Some(b) = build_of(scene, i) else { return chunk_range(n, c, j) };
    if !matches!(b.delivery.as_str(), "By Bullet Group" | "By Paragraph Group") {
        return chunk_range(n, c, j);
    }
    let levels: Vec<u32> = scene
        .drawables
        .iter()
        .find(|d| d.id == b.drawable)
        .and_then(|d| match &d.kind {
            DrawableKind::Shape(s) => s.text.as_ref().map(|t| t.paragraphs.iter().filter(|p| !t.text[p.start..p.end].trim().is_empty()).map(|p| p.level).collect()),
            _ => None,
        })
        .unwrap_or_default();
    let starts: Vec<usize> = levels.iter().enumerate().filter(|(k, l)| **l == 0 || *k == 0).map(|(k, _)| k).collect();
    let lo = starts.get(j).copied().unwrap_or(n);
    let hi = starts.get(j + 1).copied().unwrap_or(n);
    (lo, hi)
}

fn highlighted(scene: &SlideScene, i: usize) -> bool {
    build_of(scene, i).is_some_and(|b| matches!(b.delivery.as_str(), "By Highlighted Bullet" | "By Highlighted Paragraph"))
}

/// For build steps delivered by paragraph: (chunk ordinal, chunk count, paragraph count).
fn chunk_of(scene: &SlideScene, i: usize) -> Option<(usize, usize, usize)> {
    let b = build_of(scene, i)?;
    if !matches!(kind_of(b), Kind::In | Kind::Out) {
        return None;
    }
    let chunks = scene.build_order.iter().filter(|s| s.build == b.id).count();
    if chunks < 2 {
        return None;
    }
    let d = scene.drawables.iter().find(|d| d.id == b.drawable)?;
    let n = non_empty_paras(d);
    if n < 2 {
        return None;
    }
    let j = scene.build_order[..i].iter().filter(|s| s.build == b.id).count();
    Some((j, chunks, n))
}

pub fn state_at(scene: &SlideScene, step: usize) -> State {
    let mut st = State::default();
    // objects whose first build is a build-in start hidden
    let mut seen = HashSet::new();
    for i in 0..scene.build_order.len() {
        let Some(b) = build_of(scene, i) else { continue };
        let k = kind_of(b);
        if matches!(k, Kind::In | Kind::Out) && seen.insert(b.drawable) && k == Kind::In {
            st.hidden.insert(b.drawable);
        }
    }
    for i in 0..step.min(scene.build_order.len()) {
        let Some(b) = build_of(scene, i) else { continue };
        let chunk = chunk_of(scene, i);
        match kind_of(b) {
            Kind::In => {
                st.hidden.remove(&b.drawable);
                match chunk {
                    Some((j, c, n)) if j + 1 < c => {
                        let (lo, hi) = span_of(scene, i, j, c, n);
                        st.paras.insert(b.drawable, (0, hi));
                        if highlighted(scene, i) {
                            st.dim.insert(b.drawable, lo);
                        }
                    }
                    Some((j, c, n)) if highlighted(scene, i) => {
                        st.paras.remove(&b.drawable);
                        st.dim.insert(b.drawable, span_of(scene, i, j, c, n).0);
                    }
                    _ => {
                        st.paras.remove(&b.drawable);
                        st.dim.remove(&b.drawable);
                    }
                }
            }
            Kind::Out => match chunk {
                Some((j, c, n)) if j + 1 < c => {
                    st.paras.insert(b.drawable, (span_of(scene, i, j, c, n).1, n));
                }
                _ => {
                    st.hidden.insert(b.drawable);
                    st.paras.remove(&b.drawable);
                }
            },
            Kind::Action => finish_action(st.xf.entry(b.drawable).or_default(), b),
            Kind::Gallery => *st.gallery.entry(b.drawable).or_insert(0) += 1,
            _ => {}
        }
    }
    st
}

/// By Highlighted Paragraph: the non-empty paragraphs before `below` fade back (Keynote dims
/// them to about a third).
pub fn dimmed(mut d: Drawable, below: usize) -> Drawable {
    if below == 0 {
        return d;
    }
    if let DrawableKind::Shape(s) = &mut d.kind {
        if let Some(t) = &mut s.text {
            let text = t.text.clone();
            let mut k = 0;
            for p in &mut t.paragraphs {
                if text[p.start..p.end].trim().is_empty() {
                    continue;
                }
                if k < below {
                    for r in &mut p.runs {
                        r.style.color.a *= 0.35;
                    }
                    if let Some(c) = &mut p.label_color {
                        c.a *= 0.35;
                    }
                }
                k += 1;
            }
        }
    }
    d
}

/// A copy of a text object showing only the non-empty paragraphs in `lo..hi` (the others keep
/// their place but are transparent).
pub fn masked(d: &Drawable, lo: usize, hi: usize) -> Drawable {
    let mut d2 = d.clone();
    if let DrawableKind::Shape(s) = &mut d2.kind {
        if let Some(t) = &mut s.text {
            let text = t.text.clone();
            let mut k = 0;
            for p in &mut t.paragraphs {
                if text[p.start..p.end].trim().is_empty() {
                    continue;
                }
                if k < lo || k >= hi {
                    let clear = kn_model::Color::TRANSPARENT;
                    for r in &mut p.runs {
                        r.style.color = clear;
                        r.style.background = None;
                        r.style.outline = None;
                        r.style.shadow = None;
                        r.style.underline = false;
                        r.style.strike = false;
                    }
                    p.label_color = Some(clear);
                }
                k += 1;
            }
            // the shape itself (fill/stroke) only shows with the first paragraph
            if lo > 0 || hi == 0 {
                s.fill = kn_model::Fill::None;
                s.stroke = None;
            }
        }
    }
    d2
}

/// Object-space transform (about the object's centre) for an accumulated action transform.
fn object_transform(r: &mut Renderer, d: &Drawable, x: &Xform) -> sk::Transform {
    let c = r.effective_geometry(d).center();
    sk::Transform::from_translate(c.x + x.dx, c.y + x.dy)
        .pre_concat(sk::Transform::from_rotate(x.rot.to_degrees()))
        .pre_scale(x.scale, x.scale)
        .pre_translate(-c.x, -c.y)
}

/// Draw the objects of a state that pass `keep` (plus the background if asked).
#[allow(clippy::too_many_arguments)]
pub fn draw_state(r: &mut Renderer, doc: &Document, scene: &SlideScene, st: &State, pm: &mut sk::Pixmap, base: sk::Transform, keep: &dyn Fn(&Drawable) -> bool, background: bool) {
    let shown = st.scene_for(scene);
    let scene = shown.as_ref().unwrap_or(scene);
    if background {
        r.draw_fill_rect(doc, pm, &scene.background, scene.size.w, scene.size.h, base);
    }
    for d in &scene.drawables {
        if st.hidden.contains(&d.id) || !keep(d) {
            continue;
        }
        let part;
        let d = match (st.paras.get(&d.id), st.dim.get(&d.id)) {
            (Some((lo, hi)), dim) => {
                part = dimmed(masked(d, *lo, *hi), dim.copied().unwrap_or(0));
                &part
            }
            (None, Some(dim)) => {
                part = dimmed(d.clone(), *dim);
                &part
            }
            (None, None) => d,
        };
        match st.xf.get(&d.id) {
            Some(x) if *x != Xform::default() => {
                let t = base.pre_concat(object_transform(r, d, x));
                if let Some(o) = x.opacity {
                    let mut d2 = d.clone();
                    d2.opacity = o;
                    r.draw_drawable(doc, &d2, pm, t);
                } else {
                    r.draw_drawable(doc, d, pm, t);
                }
            }
            _ => r.draw_drawable(doc, d, pm, base),
        }
    }
}

fn error_pixmap(w: u32, h: u32) -> sk::Pixmap {
    let mut pm = sk::Pixmap::new(w.max(1), h.max(1)).unwrap();
    pm.fill(sk::Color::from_rgba8(80, 30, 30, 255));
    pm
}

/// Render (part of) a state into a slide-sized texture.
#[allow(clippy::too_many_arguments)]
pub fn state_texture(
    ctx: &egui::Context,
    r: &mut Renderer,
    doc: &Document,
    scene: &SlideScene,
    st: &State,
    width: u32,
    keep: &dyn Fn(&Drawable) -> bool,
    background: bool,
    name: &str,
) -> TextureHandle {
    let pm = guarded("slideshow", || {
        let scale = width as f32 / scene.size.w;
        let h = (scene.size.h * scale).round().max(1.0) as u32;
        let mut pm = sk::Pixmap::new(width.max(1), h)?;
        draw_state(r, doc, scene, st, &mut pm, sk::Transform::from_scale(scale, scale), keep, background);
        Some(pm)
    })
    .flatten()
    .unwrap_or_else(|| error_pixmap(width, width * 9 / 16));
    ctx.load_texture(name, color_image(&pm), TextureOptions::LINEAR)
}

// ---------------------------------------------------------------------------------- layers

/// One object rendered alone, unrotated, so the GPU can move/rotate/scale/fade it.
pub struct Layer {
    pub tex: TextureHandle,
    /// Blurred copies (for blur effects), heavy then light, and the areas they cover.
    pub low: Vec<(TextureHandle, Rect)>,
    /// Area the texture covers in slide points, in the object's unrotated frame.
    pub rect: Rect,
    /// Rotation pivot (object centre, slide points) and angle (radians, clockwise on screen).
    pub center: Pos2,
    pub angle: f32,
    /// Size of the object's frame (slide points).
    pub size: Vec2,
}

fn margin(d: &Drawable) -> f32 {
    let mut m = 6.0;
    if let Some(s) = &d.shadow {
        m += (s.blur + s.offset) * 1.5;
    }
    match &d.kind {
        DrawableKind::Shape(s) => {
            if let Some(st) = &s.stroke {
                m += st.width * if s.head.is_some() || s.tail.is_some() { 5.0 } else { 2.0 };
            }
        }
        DrawableKind::Image(i) => {
            if let Some(st) = &i.stroke {
                m += st.width * 2.0;
            }
        }
        DrawableKind::Group(ch) => m += ch.iter().map(margin).fold(0.0, f32::max),
        _ => {}
    }
    m
}

/// Render `d` (as transformed by `x`) into its own texture at `k` pixels per slide point.
pub fn render_layer(ctx: &egui::Context, r: &mut Renderer, doc: &Document, d: &Drawable, x: &Xform, k: f32, want_low: bool) -> Option<Layer> {
    guarded("layer", || {
        let g = r.effective_geometry(d);
        let c0 = g.center();
        let c = pos2(c0.x + x.dx, c0.y + x.dy);
        let mut d2 = d.clone();
        d2.geom.angle = 0.0;
        d2.opacity = 1.0;
        let s = x.scale;
        let has_text = matches!(&d.kind, DrawableKind::Shape(sh) if sh.text.is_some()) || matches!(d.kind, DrawableKind::Group(_) | DrawableKind::Table(_) | DrawableKind::Chart(_));
        let extra = if has_text { 0.5 * g.w.max(g.h) } else { 0.0 };
        let m = margin(d) * s + extra * s;
        let refl = if d.reflection.is_some_and(|r| r > 0.0) { g.h * s } else { 0.0 };
        let region = Rect::from_min_max(pos2(c.x - g.w * s / 2.0 - m, c.y - g.h * s / 2.0 - m), pos2(c.x + g.w * s / 2.0 + m, c.y + g.h * s / 2.0 + m + refl));
        // keep layers to a sane size
        let k = k.min(4096.0 / region.width().max(1.0)).min(4096.0 / region.height().max(1.0));
        let (pw, ph) = ((region.width() * k).ceil().max(1.0) as u32, (region.height() * k).ceil().max(1.0) as u32);
        let mut pm = sk::Pixmap::new(pw, ph)?;
        let t = sk::Transform::from_scale(k, k)
            .pre_translate(-region.min.x, -region.min.y)
            .pre_translate(c.x, c.y)
            .pre_scale(s, s)
            .pre_translate(-c0.x, -c0.y);
        r.draw_drawable(doc, &d2, &mut pm, t);
        // crop to the painted pixels (+2 px transparent border so clamped sampling stays clear)
        let (w, h) = (pw as usize, ph as usize);
        let data = pm.data();
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
        for (y, row) in data.chunks_exact(w * 4).enumerate() {
            let mut first = None;
            let mut last = 0;
            for (xx, px) in row.chunks_exact(4).enumerate() {
                if px[3] != 0 {
                    first.get_or_insert(xx);
                    last = xx;
                }
            }
            if let Some(f) = first {
                x0 = x0.min(f);
                x1 = x1.max(last + 1);
                y0 = y0.min(y);
                y1 = y + 1;
            }
        }
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let (x0, y0) = (x0.saturating_sub(2), y0.saturating_sub(2));
        let (x1, y1) = ((x1 + 2).min(w), (y1 + 2).min(h));
        let (cw, ch) = (x1 - x0, y1 - y0);
        let mut crop = vec![0u8; cw * ch * 4];
        for yy in 0..ch {
            let src = ((y0 + yy) * w + x0) * 4;
            crop[yy * cw * 4..(yy + 1) * cw * 4].copy_from_slice(&data[src..src + cw * 4]);
        }
        let img = egui::ColorImage::from_rgba_premultiplied([cw, ch], &crop);
        let tex = ctx.load_texture(format!("layer-{}", d.id), img, TextureOptions::LINEAR);
        let rect = Rect::from_min_max(pos2(region.min.x + x0 as f32 / k, region.min.y + y0 as f32 / k), pos2(region.min.x + x1 as f32 / k, region.min.y + y1 as f32 / k));
        let low = if want_low { blurred_copies(ctx, d.id, &crop, cw, ch, rect, k) } else { Vec::new() };
        Some(Layer { tex, low, rect, center: c, angle: x.rot - g.angle.to_radians(), size: vec2(g.w * s, g.h * s) })
    })
    .flatten()
}

// ---------------------------------------------------------------------------------- drawing

/// Slide-to-screen mapping.
#[derive(Clone, Copy)]
pub struct Map {
    pub r: Rect,
    /// Screen points per slide point.
    pub k: f32,
}

impl Map {
    pub fn new(r: Rect, slide_w: f32) -> Map {
        Map { r, k: r.width() / slide_w.max(1.0) }
    }
    pub fn pt(&self, p: Pos2) -> Pos2 {
        pos2(self.r.min.x + p.x * self.k, self.r.min.y + p.y * self.k)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Crop {
    None,
    /// Visible part grows from the side opposite to `dir` (unit vector) up to fraction `p`.
    Wipe { dir: Vec2, p: f32 },
    /// Soft-edged circle growing from the centre: `p` 1 covers the whole object (Keynote's Iris).
    Circle { p: f32 },
    /// Whole object, opacity falling from 1 at the side opposite to `dir` to `far` at the other.
    Ramp { dir: Vec2, far: f32 },
}

/// How an object looks at one moment of an animation, relative to its layer.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    pub offset: Vec2,
    pub sx: f32,
    pub sy: f32,
    pub rot: f32,
    pub alpha: f32,
    pub blur: f32,
    pub crop: Crop,
    /// Line draw progress (1 = whole line).
    pub reveal: f32,
}

impl Look {
    pub fn new(alpha: f32) -> Look {
        Look { offset: Vec2::ZERO, sx: 1.0, sy: 1.0, rot: 0.0, alpha, blur: 0.0, crop: Crop::None, reveal: 1.0 }
    }
}

/// Where a point in the object's local unrotated frame (relative to its centre) ends up.
pub fn place(center: Pos2, angle: f32, look: &Look, q: Vec2) -> Pos2 {
    let (s, c) = (angle + look.rot).sin_cos();
    let q = vec2(q.x * look.sx, q.y * look.sy);
    center + look.offset + vec2(q.x * c - q.y * s, q.x * s + q.y * c)
}

fn white(a: f32) -> Color32 {
    Color32::from_white_alpha((a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Add a convex polygon (local coords relative to `center`, per-vertex alpha) textured from `rect`.
#[allow(clippy::too_many_arguments)]
pub fn poly(m: &mut Mesh, map: &Map, rect: Rect, center: Pos2, angle: f32, look: &Look, pts: &[(Vec2, f32)]) {
    if pts.len() < 3 {
        return;
    }
    let base = m.vertices.len() as u32;
    for (q, a) in pts {
        let world_unrot = center + *q;
        let uv = pos2((world_unrot.x - rect.min.x) / rect.width().max(1e-6), (world_unrot.y - rect.min.y) / rect.height().max(1e-6));
        m.vertices.push(egui::epaint::Vertex { pos: map.pt(place(center, angle, look, *q)), uv, color: white(*a * look.alpha) });
    }
    for i in 1..pts.len() as u32 - 1 {
        m.add_triangle(base, base + i, base + i + 1);
    }
}

/// Draw a texture covering `rect` (unrotated object frame space) with a look.
#[allow(clippy::too_many_arguments)]
pub fn draw_textured(p: &Painter, map: &Map, tex: TextureId, rect: Rect, center: Pos2, angle: f32, look: &Look) {
    if look.alpha <= 0.002 {
        return;
    }
    let mut m = Mesh::with_texture(tex);
    let lo = rect.min - center;
    let hi = rect.max - center;
    match look.crop {
        Crop::None => poly(&mut m, map, rect, center, angle, look, &[(lo, 1.0), (vec2(hi.x, lo.y), 1.0), (hi, 1.0), (vec2(lo.x, hi.y), 1.0)]),
        Crop::Circle { p } => {
            // a disc and a soft ring of segments, each clipped to the object's rectangle
            let c = (lo + hi) * 0.5;
            let full = (hi - lo).length() * 0.5;
            let soft = full * 0.18;
            let r = (full + soft) * p.clamp(0.0, 1.0);
            let inner = (r - soft).max(0.0);
            let clip = |pts: Vec<Vec2>| clip_band(&clip_band(&pts, vec2(1.0, 0.0), lo.x, hi.x), vec2(0.0, 1.0), lo.y, hi.y);
            let alpha = |q: Vec2| ((r - (q - c).length()) / soft.max(1e-6)).clamp(0.0, 1.0);
            const N: usize = 48;
            let at = |rad: f32, i: usize| c + Vec2::angled(i as f32 * TAU / N as f32) * rad;
            if inner > 0.0 {
                let disc = clip((0..N).map(|i| at(inner, i)).collect());
                if disc.len() >= 3 {
                    poly(&mut m, map, rect, center, angle, look, &disc.into_iter().map(|q| (q, 1.0)).collect::<Vec<_>>());
                }
            }
            for i in 0..N {
                let quad = clip(vec![at(inner, i), at(r, i), at(r, i + 1), at(inner, i + 1)]);
                if quad.len() >= 3 {
                    poly(&mut m, map, rect, center, angle, look, &quad.into_iter().map(|q| (q, alpha(q))).collect::<Vec<_>>());
                }
            }
        }
        Crop::Ramp { dir, far } => {
            let corners = [lo, vec2(hi.x, lo.y), hi, vec2(lo.x, hi.y)];
            let proj: Vec<f32> = corners.iter().map(|c| c.dot(dir)).collect();
            let (pmin, pmax) = (proj.iter().cloned().fold(f32::INFINITY, f32::min), proj.iter().cloned().fold(f32::NEG_INFINITY, f32::max));
            let a = |q: f32| 1.0 + (far.clamp(0.0, 1.0) - 1.0) * ((q - pmin) / (pmax - pmin).max(1e-6));
            let pts: Vec<(Vec2, f32)> = corners.iter().zip(&proj).map(|(c, q)| (*c, a(*q))).collect();
            poly(&mut m, map, rect, center, angle, look, &pts);
        }
        Crop::Wipe { dir, p } => {
            // project the rect onto the wipe direction; reveal up to a soft edge
            let corners = [lo, vec2(hi.x, lo.y), hi, vec2(lo.x, hi.y)];
            let proj: Vec<f32> = corners.iter().map(|c| c.dot(dir)).collect();
            let (pmin, pmax) = (proj.iter().cloned().fold(f32::INFINITY, f32::min), proj.iter().cloned().fold(f32::NEG_INFINITY, f32::max));
            let soft = (pmax - pmin) * 0.15;
            let front = pmin - soft + (pmax - pmin + 2.0 * soft) * p.clamp(0.0, 1.0);
            // clip the rect against half-planes along `dir` and alpha-ramp the soft band
            let band = |from: f32, to: f32, a0: f32, a1: f32, m: &mut Mesh| {
                let pts = clip_band(&corners, dir, from, to);
                let pts: Vec<(Vec2, f32)> = pts
                    .into_iter()
                    .map(|q| {
                        let t = if to > from { ((q.dot(dir) - from) / (to - from)).clamp(0.0, 1.0) } else { 0.0 };
                        (q, a0 + (a1 - a0) * t)
                    })
                    .collect();
                poly(m, map, rect, center, angle, look, &pts);
            };
            band(pmin - 1.0, front - soft, 1.0, 1.0, &mut m);
            band(front - soft, front, 1.0, 0.0, &mut m);
        }
    }
    p.add(m);
}

/// Part of a convex polygon whose projection on `dir` lies within [from, to].
fn clip_band(pts: &[Vec2], dir: Vec2, from: f32, to: f32) -> Vec<Vec2> {
    let clip = |pts: Vec<Vec2>, f: &dyn Fn(Vec2) -> f32| -> Vec<Vec2> {
        let mut out = Vec::new();
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            let (fa, fb) = (f(a), f(b));
            if fa >= 0.0 {
                out.push(a);
            }
            if (fa >= 0.0) != (fb >= 0.0) {
                out.push(a + (b - a) * (fa / (fa - fb)));
            }
        }
        out
    };
    if to <= from {
        return Vec::new();
    }
    let p1 = clip(pts.to_vec(), &|q| q.dot(dir) - from);
    clip(p1, &|q| to - q.dot(dir))
}

pub fn draw_layer(p: &Painter, map: &Map, l: &Layer, look: &Look) {
    if look.blur > 0.01 {
        if let [(heavy, hrect), (light, lrect)] = &l.low[..] {
            // sharp -> light -> heavy as blur goes 0 -> 1, cross-fading between neighbours
            let b = look.blur.clamp(0.0, 1.0) * 2.0;
            let ((a, ar), (c, cr), w) = if b > 1.0 { ((light.id(), *lrect), (heavy.id(), *hrect), b - 1.0) } else { ((l.tex.id(), l.rect), (light.id(), *lrect), b) };
            // the first copy at the opacity that, with the second drawn over it, adds up to
            // the look's own
            let mut lk = *look;
            let second = look.alpha * w;
            lk.alpha = look.alpha * (1.0 - w) / (1.0 - second).max(1e-3);
            draw_textured(p, map, a, ar, l.center, l.angle, &lk);
            lk.alpha = second;
            draw_textured(p, map, c, cr, l.center, l.angle, &lk);
            return;
        }
    }
    draw_textured(p, map, l.tex.id(), l.rect, l.center, l.angle, look);
}

/// Gaussian-blurred copies of a layer's pixels (premultiplied, `cw` x `ch`, covering `rect` at
/// `k` pixels per point): σ ≈ 40 pt (Keynote's Blur build at its blurriest, R43 movie) and a
/// third of that, each worked at 1/8 resolution with a transparent margin of 3σ.
fn blurred_copies(ctx: &egui::Context, id: ObjId, crop: &[u8], cw: usize, ch: usize, rect: Rect, k: f32) -> Vec<(TextureHandle, Rect)> {
    let f = 8usize;
    let (sw, sh) = (cw.div_ceil(f), ch.div_ceil(f));
    // box-filtered small copy
    let mut small = vec![0f32; sw * sh * 4];
    for ly in 0..sh {
        for lx in 0..sw {
            let mut acc = [0f32; 4];
            let mut n = 0f32;
            for yy in ly * f..((ly + 1) * f).min(ch) {
                for xx in lx * f..((lx + 1) * f).min(cw) {
                    let p = (yy * cw + xx) * 4;
                    for (a, v) in acc.iter_mut().zip(&crop[p..p + 4]) {
                        *a += f32::from(*v);
                    }
                    n += 1.0;
                }
            }
            let o = (ly * sw + lx) * 4;
            for i in 0..4 {
                small[o + i] = acc[i] / n.max(1.0);
            }
        }
    }
    let mut out = Vec::new();
    for (n, sigma_pt) in [(0, 40.0f32), (1, 40.0 / 3.0)] {
        let sigma = (sigma_pt * k / f as f32).max(0.5);
        let m = (sigma * 3.0).ceil() as usize;
        let (lw, lh) = (sw + 2 * m, sh + 2 * m);
        let kern: Vec<f32> = {
            let raw: Vec<f32> = (0..=2 * m).map(|i| (-((i as f32 - m as f32).powi(2)) / (2.0 * sigma * sigma)).exp()).collect();
            let sum: f32 = raw.iter().sum();
            raw.into_iter().map(|v| v / sum).collect()
        };
        // horizontal pass into a padded buffer, then vertical
        let mut hpass = vec![0f32; lw * sh * 4];
        for y in 0..sh {
            for x in 0..lw {
                let mut acc = [0f32; 4];
                for (j, w) in kern.iter().enumerate() {
                    let sx = x as isize + j as isize - 2 * m as isize;
                    if sx >= 0 && (sx as usize) < sw {
                        let p = (y * sw + sx as usize) * 4;
                        for i in 0..4 {
                            acc[i] += small[p + i] * w;
                        }
                    }
                }
                hpass[(y * lw + x) * 4..(y * lw + x) * 4 + 4].copy_from_slice(&acc);
            }
        }
        let mut px = vec![0u8; lw * lh * 4];
        for y in 0..lh {
            for x in 0..lw {
                let mut acc = [0f32; 4];
                for (j, w) in kern.iter().enumerate() {
                    let sy = y as isize + j as isize - 2 * m as isize;
                    if sy >= 0 && (sy as usize) < sh {
                        let p = (sy as usize * lw + x) * 4;
                        for i in 0..4 {
                            acc[i] += hpass[p + i] * w;
                        }
                    }
                }
                for i in 0..4 {
                    px[(y * lw + x) * 4 + i] = acc[i].round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        let unit = f as f32 / k;
        let lrect = Rect::from_min_size(pos2(rect.min.x - m as f32 * unit, rect.min.y - m as f32 * unit), vec2(lw as f32 * unit, lh as f32 * unit));
        let tex = ctx.load_texture(format!("layer-blur{n}-{id}"), egui::ColorImage::from_rgba_premultiplied([lw, lh], &px), TextureOptions::LINEAR);
        out.push((tex, lrect));
    }
    out
}

/// Progressive reveal of a line along its polyline (local coords relative to the layer centre).
fn draw_line_reveal(p: &Painter, map: &Map, l: &Layer, look: &Look, line: &[Vec2], hw: f32) {
    if look.reveal >= 0.999 || line.len() < 2 {
        let mut lk = *look;
        lk.reveal = 1.0;
        draw_layer(p, map, l, &lk);
        return;
    }
    let total: f32 = line.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    let mut left = total * look.reveal.clamp(0.0, 1.0);
    let mut m = Mesh::with_texture(l.tex.id());
    for w in line.windows(2) {
        if left <= 0.0 {
            break;
        }
        let (a, b0) = (w[0], w[1]);
        let len = (b0 - a).length();
        if len < 1e-4 {
            continue;
        }
        let u = (b0 - a) / len;
        let b = if left < len { a + u * left } else { b0 };
        left -= len;
        let n = vec2(-u.y, u.x) * hw;
        let (a2, b2) = (a - u * hw, b + u * hw);
        poly(&mut m, map, l.rect, l.center, l.angle, look, &[(a2 + n, 1.0), (b2 + n, 1.0), (b2 - n, 1.0), (a2 - n, 1.0)]);
    }
    p.add(m);
}

// ---------------------------------------------------------------------------------- effect looks

/// Context of an object for effect evaluation.
struct Env {
    /// Layer bounds in slide points (unrotated, at the object's current place).
    rect: Rect,
    slide: Vec2,
}

/// Offset that moves `rect` just off the slide when travelling in `-v` from its place.
fn off_slide(env: &Env, v: Vec2) -> Vec2 {
    let r = env.rect;
    let mut o = Vec2::ZERO;
    if v.x > 0.01 {
        o.x = -r.max.x;
    } else if v.x < -0.01 {
        o.x = env.slide.x - r.min.x;
    }
    if v.y > 0.01 {
        o.y = -r.max.y;
    } else if v.y < -0.01 {
        o.y = env.slide.y - r.min.y;
    }
    o
}

/// Offset that moves `rect` just off the slide travelling along `v`.
fn exit_slide(env: &Env, v: Vec2) -> Vec2 {
    let r = env.rect;
    let mut o = Vec2::ZERO;
    if v.x > 0.01 {
        o.x = env.slide.x - r.min.x;
    } else if v.x < -0.01 {
        o.x = -r.max.x;
    }
    if v.y > 0.01 {
        o.y = env.slide.y - r.min.y;
    } else if v.y < -0.01 {
        o.y = -r.max.y;
    }
    o
}

/// Look of a build-in at progress `t` (linear 0..1); `t = 1` is the plain object.
fn look_in(lk: &mut Look, fx: Fx, b: &Build, t: f32, env: &Env) {
    let e = smooth(t);
    match fx {
        Fx::Appear => {}
        Fx::Dissolve => lk.alpha *= e,
        Fx::Move { fly: true } => {
            // Fly In (apple:sidezoom, R43 movie): from the slide's edge, tiny and clear, growing
            // as t^1.5 and fading in while it slows down
            let v = dir_vec(b.direction, vec2(1.0, 0.0));
            let r = env.rect;
            let half = vec2(r.width() * v.x.abs(), r.height() * v.y.abs()) * 0.5;
            let mut start = off_slide(env, v);
            start.x += half.x * v.x.signum();
            start.y += half.y * v.y.signum();
            lk.offset += start * (1.0 - (t * PI / 2.0).sin());
            let s = t.powf(1.5);
            lk.sx *= s;
            lk.sy *= s;
            lk.alpha *= e;
        }
        Fx::Move { fly: false } => {
            let v = dir_vec(b.direction, vec2(1.0, 0.0));
            let k = if b.params.bounce { move_bounce(t) } else { ease_out(t) };
            lk.offset += off_slide(env, v) * (1.0 - k);
        }
        Fx::Blur => {
            // R43 movie (2 s): fades in with an ease-out while staying very blurred, sharpening
            // mostly at the end
            lk.alpha *= 1.0 - (1.0 - t).powf(1.5);
            lk.blur = 1.0 - t.powf(3.5);
        }
        Fx::LineDraw => lk.reveal = accel(if b.params.acceleration == 0 { 3 } else { b.params.acceleration }, t),
        // R43 movie: an even sweep, the object uncovered by ~0.87
        Fx::Wipe => lk.crop = Crop::Wipe { dir: dir_vec(b.direction, vec2(1.0, 0.0)), p: (0.892 * t + 0.13).min(1.0) },
        // R43 movie: a soft circle growing evenly from ~0.13, covering the object by ~0.8
        Fx::Iris => lk.crop = Crop::Circle { p: ((t - 0.13) / 0.68).clamp(0.0, 1.0) },
        Fx::Pop if text_units(b) > 0 => {
            // text popping in letter by letter: a fade with a slight grow, no overshoot (R43)
            let s = 0.7 + 0.3 * ease_out(t);
            lk.sx *= s;
            lk.sy *= s;
            lk.alpha *= e;
        }
        Fx::Pop => {
            // R43 movie: grows to 1.07 at ~0.44, dips to ~0.945 at ~0.62, settles; fades in
            let s = if t < 0.44 {
                1.07 * (t / 0.44).powf(1.2)
            } else if t < 0.62 {
                1.07 - 0.125 * smooth((t - 0.44) / 0.18)
            } else {
                0.945 + 0.055 * smooth((t - 0.62) / 0.33)
            };
            lk.sx *= s;
            lk.sy *= s;
            lk.alpha *= smooth(t * 1.08);
        }
        Fx::Scale { big: false } => {
            // R43 movie: from ~0.07 of the size with an ease-out, fading in evenly
            let s = 0.07 + 0.93 * (1.0 - (1.0 - t).powf(1.7));
            lk.sx *= s;
            lk.sy *= s;
            lk.alpha *= t;
        }
        Fx::Scale { big: true } => {
            let s = 1.0 + 2.0 * (1.0 - e);
            lk.sx *= s;
            lk.sy *= s;
            lk.alpha *= e;
        }
        Fx::Drop => {
            // R43 movie: falls from above the slide, landing at 0.2, then bounces of
            // ~16% and ~5% of the slide height, settled by ~0.97
            const LAND: f32 = 0.2;
            if t < LAND {
                let k = t / LAND;
                lk.offset += off_slide(env, vec2(0.0, 1.0)) * (1.0 - k * k);
            } else {
                for (u0, u1, hgt) in [(LAND, 0.62, 0.163), (0.62, 0.87, 0.05), (0.87, 0.97, 0.012)] {
                    if t >= u0 && t < u1 {
                        let w = (t - u0) / (u1 - u0) * 2.0 - 1.0;
                        lk.offset.y -= hgt * (1.0 - w * w) * env.slide.y;
                    }
                }
            }
        }
        Fx::Bouncy => {
            // R43 movie (2 s, From Left): three hops, landing at ~0.29, 2/3 and ~0.95 of the
            // time, the first two well up the slide, squashing on each landing
            let v = dir_vec(b.direction, vec2(1.0, 0.0));
            let (along, height, squash) = bouncy(t);
            let h = env.rect.height();
            lk.offset += off_slide(env, v) * along;
            lk.offset.y -= height * env.slide.y;
            lk.sy *= 1.0 - squash;
            lk.sx *= 1.0 + squash * 0.4;
            lk.offset.y += h * squash / 2.0;
        }
        Fx::Anvil => {
            // R43 movie (1.5 s): falls from above the slide, landing at ~0.13 of the time,
            // then a small rebound settling by ~0.4
            const LAND: f32 = 0.13;
            if t < LAND {
                let k = t / LAND;
                lk.offset += off_slide(env, vec2(0.0, 1.0)) * (1.0 - k * k);
            } else {
                let k = ((t - LAND) / 0.27).min(1.0);
                lk.offset.y -= 0.011 * env.slide.y * (k * PI).sin() * (1.0 - k);
            }
        }
        Fx::FadeMove => {
            // Travel Distance is a share of the way off the slide (R40: 100% starts the text
            // just off the left edge, 11% moves the logo ~120 pt), with a cubic ease-out
            let v = dir_vec(b.direction, vec2(1.0, 0.0));
            let off = match b.params.travel_distance {
                Some(d) => off_slide(env, v) * d,
                None => -v * 0.1 * env.slide.x,
            };
            lk.offset += off * (1.0 - ease_out(t));
            lk.alpha *= e;
        }
        Fx::FadeScale => {
            // Keynote's movie (R43, amount 70): grows from ~0.79 with an ease-out, fades in
            // slowly at first (about t squared) and top first
            let s = 1.0 - fade_scale_k(b) * (1.0 - t) * (1.0 - t);
            lk.sx *= s;
            lk.sy *= s;
            lk.alpha *= t * t;
            lk.crop = Crop::Ramp { dir: vec2(0.0, 1.0), far: smooth((t - 0.1) / 0.55) };
        }
        Fx::Spin => {
            // Keynote's Twirl (apple:spin): two turns clockwise into place (R43 frames)
            lk.rot += spin_sign(b) * (1.0 - e) * 2.0 * TAU;
            lk.alpha *= e;
        }
        Fx::Twirl => {
            lk.rot += (1.0 - e) * PI * 1.5;
            lk.sx *= e;
            lk.sy *= e;
            lk.alpha *= (t * 2.0).min(1.0);
        }
        Fx::Compress => lk.sy *= e,
        Fx::Cube => {
            // R43 movie: the object is a cube's face turning in from the side (90° to face-on
            // by ~0.33, easing out), overshooting ~18° with Bounce; fades in by ~0.45
            let v = dir_vec(b.direction, vec2(1.0, 0.0));
            let mut deg = 90.0 * (1.0 - t / 0.33).max(0.0).powf(1.2);
            if b.params.bounce && t > 0.33 && t < 0.67 {
                deg -= 18.0 * (PI * (t - 0.33) / 0.34).sin();
            }
            let (sin, cos) = deg.to_radians().sin_cos();
            let r = env.rect;
            if v.x.abs() >= v.y.abs() {
                lk.sx *= cos;
                lk.offset.x -= v.x.signum() * r.width() * 0.5 * sin;
            } else {
                lk.sy *= cos;
                lk.offset.y -= v.y.signum() * r.height() * 0.5 * sin;
            }
            lk.alpha *= ((t + 0.02) / 0.45).min(1.0);
        }
        Fx::FlipOver => {
            // R43 movie: from its back (180°) to face-on by 0.4 s, gathering speed, then a
            // wobble of ~30° dying out (with Bounce); fades in by ~0.33 s
            let turn = (t / 0.4).min(1.0).powf(1.76);
            let mut deg = 180.0 * (1.0 - turn);
            if b.params.bounce && t > 0.4 && t < 0.95 {
                let u = t - 0.4;
                deg += 35.0 * (TAU * u / 0.28).sin() * (1.0 - u / 0.55).powi(2);
            }
            lk.sx *= deg.to_radians().cos();
            lk.alpha *= (t / 0.33).min(1.0).powi(2);
        }
        Fx::Flip => {
            lk.sx *= (e * PI / 2.0).sin();
            lk.alpha *= (t * 3.0).min(1.0);
        }
        _ => lk.alpha *= e,
    }
}

fn look_out(lk: &mut Look, fx: Fx, b: &Build, t: f32, env: &Env) {
    match fx {
        Fx::Move { fly } => {
            let v = dir_vec(b.direction, vec2(1.0, 0.0));
            let k = if fly {
                smooth(t)
            } else if b.params.bounce {
                // R43 movie: a small wind-up back (~3.5% of the way) then off, gathering speed
                let wind = if t < 0.45 { -0.035 * (PI * t / 0.45).sin() } else { 0.0 };
                wind + ((t - 0.4) / 0.6).clamp(0.0, 1.0).powf(1.7)
            } else {
                t * t * t
            };
            lk.offset += exit_slide(env, v) * k;
        }
        Fx::Drop | Fx::Anvil | Fx::Bouncy => lk.offset += exit_slide(env, vec2(0.0, 1.0)) * (t * t),
        Fx::Wipe => {
            let v = dir_vec(b.direction, vec2(1.0, 0.0));
            lk.crop = Crop::Wipe { dir: -v, p: 1.0 - smooth(t) };
        }
        Fx::FadeMove => {
            let v = dir_vec(b.direction, vec2(1.0, 0.0));
            let off = match b.params.travel_distance {
                Some(d) => exit_slide(env, v) * d,
                None => v * 0.1 * env.slide.x,
            };
            lk.offset += off * smooth(t) * smooth(t);
            lk.alpha *= 1.0 - smooth(t);
        }
        Fx::FadeScale => {
            let s = 1.0 + smooth(t) * fade_scale_k(b);
            lk.sx *= s;
            lk.sy *= s;
            lk.alpha *= 1.0 - smooth(t);
        }
        Fx::Scale { big } => {
            // (R43 Scale Down: size 1 − t^1.5, fading evenly; it was gone by two thirds)
            let s = if big { 1.0 + 2.0 * smooth(t) } else { 1.0 - t.clamp(0.0, 1.0).powf(1.5) };
            lk.sx *= s;
            lk.sy *= s;
            lk.alpha *= if big { 1.0 - smooth(t) } else { 1.0 - t.clamp(0.0, 1.0) };
        }
        _ => look_in(lk, fx, b, 1.0 - t, env),
    }
}

fn look_action(lk: &mut Look, fx: Fx, b: &Build, t: f32, path: &[(Vec2, f32)]) {
    let a = accel(b.params.acceleration, t);
    match fx {
        Fx::Path => lk.offset += along(path, a),
        Fx::Rotate => {
            let ang = b.params.rotation_angle.unwrap_or(90.0).to_radians();
            lk.rot += if b.params.rotation_clockwise { ang } else { -ang } * a;
        }
        Fx::ScaleTo => {
            let s = 1.0 + (b.params.scale.unwrap_or(1.5) - 1.0) * a;
            lk.sx *= s;
            lk.sy *= s;
        }
        Fx::Opacity => {} // handled by the caller (needs the starting opacity)
        Fx::Pulse | Fx::PopAction => {
            let s = 1.0 + 0.15 * (t * PI).sin();
            lk.sx *= s;
            lk.sy *= s;
        }
        Fx::Blink => lk.alpha *= if ((t * 6.0) as i32) % 2 == 0 { 1.0 } else { 0.0 },
        Fx::Bounce => lk.offset.y -= (t * PI * 2.0).sin().abs() * 40.0 * (1.0 - t),
        Fx::Jiggle => lk.rot += (t * PI * 8.0).sin() * 0.08 * (1.0 - t),
        Fx::FlipAction => lk.sx *= (t * TAU).cos(),
        _ => {}
    }
}

// ---------------------------------------------------------------------------------- groups

#[derive(Clone, Debug)]
pub struct Track {
    pub step: usize,
    pub build: Build,
    pub kind: Kind,
    pub fx: Fx,
    pub start: f32,
    pub dur: f32,
    /// Paragraphs this step delivers (text built by paragraph).
    pub paras: Option<(usize, usize)>,
    /// Text delivered piece by piece: 1 by character, 2 by word, 3 by line (0 = whole object).
    pub units: u8,
}

/// How a build delivers text: its Text Animation setting (1 by object, 2 by word, 3 by
/// character, 4 by line; R40: "fade and move character" set to By Object moves the whole box),
/// else what the effect id says.
pub fn text_units(b: &Build) -> u8 {
    match b.params.text_delivery {
        1 => return 0,
        2 => return 2,
        3 => return 1,
        4 => return 3,
        _ => {}
    }
    let e = b.effect.to_lowercase();
    if e.ends_with(" character") || e.ends_with("_character") || effect_name(&b.effect) == "keyboard" {
        1
    } else if e.ends_with(" word") {
        2
    } else if e.ends_with(" line") {
        3
    } else {
        0
    }
}

/// Boxes of a text object's characters (1), words (2) or lines (3), in slide points in the
/// object's unrotated frame, ordered as delivered (option 1 forward, 2 backward, 3 from the
/// centre, 4 from the edges, 5 random).
fn text_unit_rects(r: &mut Renderer, d: &Drawable, units: u8, option: u32) -> Vec<Rect> {
    let Some((tl, g)) = r.text_layout(d) else { return Vec::new() };
    let DrawableKind::Shape(s) = &d.kind else { return Vec::new() };
    let Some(tb) = &s.text else { return Vec::new() };
    let mut out: Vec<Rect> = Vec::new();
    let mut prev: Option<(usize, bool)> = None; // (line, ended on a space)
    for gl in tl.glyphs.iter().filter(|g| !g.label) {
        let blank = tb.text.get(gl.start..gl.end).is_none_or(|t| t.trim().is_empty());
        let rect = Rect::from_min_max(pos2(g.x + gl.x, g.y + gl.y - gl.size * 0.95), pos2(g.x + gl.x + gl.w, g.y + gl.y + gl.size * 0.3));
        let join = match (units, prev) {
            (2, Some((line, space))) => line == gl.line && !space && !blank,
            (3, Some((line, _))) => line == gl.line,
            _ => false,
        };
        prev = Some((gl.line, blank));
        if blank && units != 3 {
            continue;
        }
        match out.last_mut() {
            Some(last) if join => *last = last.union(rect),
            _ => out.push(rect),
        }
    }
    let n = out.len();
    let mid = (n as f32 - 1.0) / 2.0;
    let mut order: Vec<usize> = (0..n).collect();
    match option {
        2 => order.reverse(),
        3 => order.sort_by(|a, b| (*a as f32 - mid).abs().total_cmp(&(*b as f32 - mid).abs())),
        4 => order.sort_by(|a, b| (*b as f32 - mid).abs().total_cmp(&(*a as f32 - mid).abs())),
        5 => {
            let mut seed = 0x9e3779b9u32 ^ n as u32;
            for i in (1..n).rev() {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                order.swap(i, (seed >> 8) as usize % (i + 1));
            }
        }
        _ => {}
    }
    order.into_iter().map(|i| out[i]).collect()
}

/// Steps played by one trigger: `from` and the automatic steps after it. Returns the end step
/// and each step's timing (seconds from the trigger).
pub fn timeline(scene: &SlideScene, from: usize) -> (usize, Vec<Track>) {
    let mut tracks: Vec<Track> = Vec::new();
    let mut to = from;
    // `anchor`: start of the build the "With …" steps go with (R40: builds 3–5 "With Build 1"
    // start with build 1, not with build 2 and its 0.1 s delay)
    let (mut anchor, mut prev_end) = (0.0f32, 0.0f32);
    for i in from..scene.build_order.len() {
        let st = &scene.build_order[i];
        if i > from && !st.automatic {
            break;
        }
        to = i + 1;
        let Some(b) = build_of(scene, i) else { continue };
        let kind = kind_of(b);
        let fx = fx_of(b);
        let mut dur = if st.duration > 0.0 { st.duration } else { b.duration } as f32;
        if matches!(kind, Kind::Media(_) | Kind::Other) || (fx == Fx::Appear && kind != Kind::Gallery) || (kind == Kind::Gallery && effect_name(&b.effect).contains("appear")) {
            dur = 0.0;
        }
        let dur = dur.clamp(0.0, 30.0);
        let delay = st.delay.max(0.0) as f32;
        let start = if i == from {
            delay
        } else if st.with_previous {
            anchor + delay
        } else {
            prev_end + delay
        };
        if i == from || !st.with_previous {
            anchor = start;
        }
        prev_end = start + dur;
        let paras = chunk_of(scene, i).map(|(j, c, n)| span_of(scene, i, j, c, n));
        let units = if matches!(kind, Kind::In | Kind::Out) { text_units(b) } else { 0 };
        tracks.push(Track { step: i, build: b.clone(), kind, fx, start, dur, paras, units });
    }
    (to, tracks)
}

struct ObjPiece {
    id: ObjId,
    layer: Option<Layer>,
    visible: bool,
    alpha: f32,
    tracks: Vec<usize>,
    /// Line draw: polyline (relative to the layer centre) and half width.
    line: Option<(Vec<Vec2>, f32)>,
    /// Move action paths per track index.
    paths: HashMap<usize, Vec<(Vec2, f32)>>,
    env_rect: Rect,
    /// A gallery's image (or caption) on its way out (false) or in (true), and whether it is
    /// the caption (captions only cross-fade).
    gallery: Option<(bool, bool)>,
    /// Text built by character / word / line: each piece's box (slide points, the object's
    /// unrotated frame) in delivery order.
    units: Vec<Rect>,
}

enum Piece {
    Static(TextureHandle),
    Obj(ObjPiece),
}

/// Everything needed to play one group of build steps, prepared ahead of time.
pub struct Group {
    pub to: usize,
    pub tracks: Vec<Track>,
    pub total: f32,
    pieces: Vec<Piece>,
    slide: Vec2,
}

/// Local polyline of a line shape, relative to its (unrotated) centre, scaled by `s`.
fn line_polyline(r: &mut Renderer, d: &Drawable, s: f32) -> Option<(Vec<Vec2>, f32)> {
    let DrawableKind::Shape(sh) = &d.kind else { return None };
    let st = sh.stroke.as_ref()?;
    let mut g: Geometry = r.effective_geometry(d);
    g.angle = 0.0;
    let c = g.center();
    let pts: Vec<Vec2> = flatten(&sh.path.els)
        .into_iter()
        .map(|q| {
            let w = g.to_slide(kn_model::Point::new(q.x, q.y));
            vec2(w.x - c.x, w.y - c.y) * s
        })
        .collect();
    let ends = if sh.head.is_some() || sh.tail.is_some() { st.width * 2.5 } else { 0.0 };
    Some((pts, (st.width / 2.0 + 1.5 + ends) * s))
}

impl Group {
    /// Render the layers and strata for steps starting at `from`.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(ctx: &egui::Context, r: &mut Renderer, doc: &Document, scene: &SlideScene, from: usize, width: u32) -> Group {
        let (to, tracks) = timeline(scene, from);
        let total = tracks.iter().map(|t| t.start + t.dur).fold(0.0, f32::max);
        let st = state_at(scene, from);
        let k = width as f32 / scene.size.w;
        let animated: HashSet<ObjId> = tracks.iter().filter(|t| matches!(t.kind, Kind::In | Kind::Out | Kind::Action)).map(|t| t.build.drawable).collect();
        let mut pieces = Vec::new();
        let mut pending: Vec<ObjId> = Vec::new();
        let flush = |pending: &mut Vec<ObjId>, pieces: &mut Vec<Piece>, r: &mut Renderer| {
            let first = pieces.is_empty();
            if pending.is_empty() && !first {
                return;
            }
            let ids = std::mem::take(pending);
            let keep = move |d: &Drawable| ids.contains(&d.id);
            pieces.push(Piece::Static(state_texture(ctx, r, doc, scene, &st, width, &keep, first && scene.motion.is_none(), "play-stratum")));
        };
        // galleries moving on to their next image: the image (and caption) leaving and arriving
        let gallery_steps: Vec<usize> = tracks.iter().enumerate().filter(|(_, t)| t.kind == Kind::Gallery).map(|(i, _)| i).collect();
        let mut gallery_parts: HashMap<ObjId, (usize, ObjId)> = HashMap::new();
        for &i in &gallery_steps {
            let id = tracks[i].build.drawable;
            if gallery_parts.contains_key(&id) {
                continue;
            }
            gallery_parts.insert(id, (i, id));
            if let Some(DrawableKind::Image(im)) = scene.drawables.iter().find(|d| d.id == id).map(|d| &d.kind) {
                if let Some(c) = im.gallery_caption_id {
                    gallery_parts.insert(c, (i, id));
                }
            }
        }
        for d in &scene.drawables {
            if let Some(&(i, gid)) = gallery_parts.get(&d.id) {
                flush(&mut pending, &mut pieces, r);
                let x = st.xform(gid);
                let at = st.gallery.get(&gid).copied().unwrap_or(0);
                for (incoming, k_img) in [(false, at), (true, at + 1)] {
                    let mut sc2 = scene.clone();
                    sc2.show_gallery_image(gid, k_img);
                    let Some(dd) = sc2.drawables.iter().find(|x| x.id == d.id) else { continue };
                    let layer = render_layer(ctx, r, doc, dd, &x, k, false);
                    let env_rect = layer.as_ref().map(|l| l.rect).unwrap_or(Rect::NOTHING);
                    pieces.push(Piece::Obj(ObjPiece {
                        id: d.id,
                        layer,
                        visible: !incoming && !st.hidden.contains(&gid),
                        alpha: x.opacity.unwrap_or(d.opacity),
                        tracks: vec![i],
                        line: None,
                        paths: HashMap::new(),
                        env_rect,
                        gallery: Some((incoming, d.id != gid)),
                        units: Vec::new(),
                    }));
                }
                continue;
            }
            // movies get a piece of their own even when not animating: their live frames are
            // drawn there (flattened into a still they froze during the build, then jumped on)
            let movie = matches!(d.kind, DrawableKind::Movie(_));
            if !animated.contains(&d.id) && !movie {
                if !st.hidden.contains(&d.id) {
                    pending.push(d.id);
                }
                continue;
            }
            flush(&mut pending, &mut pieces, r);
            let x = st.xform(d.id);
            let my: Vec<usize> = tracks.iter().enumerate().filter(|(_, t)| t.build.drawable == d.id && matches!(t.kind, Kind::In | Kind::Out | Kind::Action)).map(|(i, _)| i).collect();
            if !my.is_empty() && my.iter().all(|i| tracks[*i].paras.is_some()) {
                // built by paragraph: the paragraphs already shown stay, each step's paragraphs animate
                let n = non_empty_paras(d);
                let (lo, hi) = if st.hidden.contains(&d.id) { (0, 0) } else { st.paras.get(&d.id).copied().unwrap_or((0, n)) };
                let moving: Vec<(usize, usize)> = my.iter().filter_map(|i| tracks[*i].paras).collect();
                let keep: Vec<usize> = (lo..hi).filter(|k| !moving.iter().any(|(a, b)| k >= a && k < b)).collect();
                if let (Some(a), Some(b)) = (keep.first(), keep.last()) {
                    let part = masked(d, *a, *b + 1);
                    let layer = render_layer(ctx, r, doc, &part, &x, k, false);
                    let env_rect = layer.as_ref().map(|l| l.rect).unwrap_or(Rect::NOTHING);
                    pieces.push(Piece::Obj(ObjPiece { id: d.id, layer, visible: true, alpha: x.opacity.unwrap_or(d.opacity), tracks: Vec::new(), line: None, paths: HashMap::new(), env_rect, gallery: None, units: Vec::new() }));
                }
                for &i in &my {
                    let (a, b) = tracks[i].paras.unwrap_or((0, n));
                    let part = masked(d, a, b);
                    let layer = render_layer(ctx, r, doc, &part, &x, k, tracks[i].fx == Fx::Blur);
                    let env_rect = layer.as_ref().map(|l| l.rect).unwrap_or(Rect::NOTHING);
                    pieces.push(Piece::Obj(ObjPiece {
                        id: d.id,
                        layer,
                        visible: tracks[i].kind == Kind::Out,
                        alpha: x.opacity.unwrap_or(d.opacity),
                        tracks: vec![i],
                        line: None,
                        paths: HashMap::new(),
                        env_rect,
                        gallery: None,
                        units: Vec::new(),
                    }));
                }
                continue;
            }
            let want_low = my.iter().any(|i| tracks[*i].fx == Fx::Blur);
            let layer = render_layer(ctx, r, doc, d, &x, k, want_low);
            let line = if my.iter().any(|i| tracks[*i].fx == Fx::LineDraw) { line_polyline(r, d, x.scale) } else { None };
            let paths = my.iter().filter(|i| tracks[**i].fx == Fx::Path).map(|i| (*i, motion_polyline(&tracks[*i].build))).collect();
            let env_rect = layer.as_ref().map(|l| l.rect).unwrap_or(Rect::NOTHING);
            let units = match my.iter().map(|i| tracks[*i].units).max().unwrap_or(0) {
                0 => Vec::new(),
                u => {
                    let opt = my.iter().find(|i| tracks[**i].units > 0).map(|i| tracks[*i].build.params.delivery_option).unwrap_or(0);
                    text_unit_rects(r, d, u, opt)
                }
            };
            pieces.push(Piece::Obj(ObjPiece {
                id: d.id,
                layer,
                visible: !st.hidden.contains(&d.id),
                alpha: x.opacity.unwrap_or(d.opacity),
                tracks: my,
                line,
                paths,
                env_rect,
                gallery: None,
                units,
            }));
        }
        if !pending.is_empty() || pieces.is_empty() {
            flush(&mut pending, &mut pieces, r);
        }
        if std::env::var_os("KN_DEBUG").is_some() {
            for t in &tracks {
                eprintln!("group {from}..{to}: step {} {:?} {:?} {} start {:.2} dur {:.2}", t.step, t.kind, t.fx, t.build.drawable, t.start, t.dur);
            }
            for p in &pieces {
                match p {
                    Piece::Static(_) => eprintln!("  static"),
                    Piece::Obj(o) => eprintln!("  obj {} visible {} rect {:?}", o.id, o.visible, o.layer.as_ref().map(|l| l.rect)),
                }
            }
        }
        Group { to, tracks, total, pieces, slide: vec2(scene.size.w, scene.size.h) }
    }

    /// Draw the group at `t` seconds. `video` may supply a live movie frame for an object
    /// (texture + the object's frame size), drawn instead of its poster layer.
    pub fn draw(&self, p: &Painter, map: &Map, t: f32, video: &mut dyn FnMut(ObjId) -> Option<(TextureId, Vec2)>) {
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        for piece in &self.pieces {
            match piece {
                Piece::Static(tex) => {
                    let mut m = Mesh::with_texture(tex.id());
                    m.add_rect_with_uv(map.r, full, Color32::WHITE);
                    p.add(m);
                }
                Piece::Obj(o) => {
                    let Some(l) = &o.layer else { continue };
                    let Some(look) = self.look(o, t) else { continue };
                    if let Some((tex, size)) = video(o.id) {
                        let rect = Rect::from_center_size(l.center, size);
                        draw_textured(p, map, tex, rect, l.center, l.angle, &look);
                    } else if let Some((line, hw)) = &o.line {
                        draw_line_reveal(p, map, l, &look, line, *hw);
                    } else if !o.units.is_empty() {
                        // text by character / word / line: each piece of the layer on its own clock
                        let n = o.units.len();
                        for (k, u) in o.units.iter().enumerate() {
                            let Some(lk) = self.look_at(o, t, Some((k, n))) else { continue };
                            if lk.alpha <= 0.002 {
                                continue;
                            }
                            let mut m = Mesh::with_texture(l.tex.id());
                            let (lo, hi) = (u.min - l.center, u.max - l.center);
                            poly(&mut m, map, l.rect, l.center, l.angle, &lk, &[(lo, 1.0), (vec2(hi.x, lo.y), 1.0), (hi, 1.0), (vec2(lo.x, hi.y), 1.0)]);
                            p.add(m);
                        }
                        self.draw_typing_cursor(p, map, o, l, t);
                    } else {
                        draw_layer(p, map, l, &look);
                    }
                }
            }
        }
    }

    /// Keyboard build: a thin bar after the last letter typed, from the build's start until
    /// shortly before its end (R43 movie).
    fn draw_typing_cursor(&self, p: &Painter, map: &Map, o: &ObjPiece, l: &Layer, t: f32) {
        let Some(tr) = o.tracks.iter().map(|i| &self.tracks[*i]).find(|tr| tr.fx == Fx::Keyboard && tr.kind == Kind::In) else { return };
        if !tr.build.params.cursor.unwrap_or(true) || t < tr.start || tr.dur <= 0.0 {
            return;
        }
        let u = (t - tr.start) / tr.dur;
        let n = o.units.len();
        if u >= 0.93 || n == 0 {
            return;
        }
        let typed = (0..n).take_while(|k| u >= keyboard_at(*k, n)).count();
        let (x, r) = match typed {
            0 => (o.units[0].min.x, o.units[0]),
            k => (o.units[k - 1].max.x, o.units[k - 1]),
        };
        let w = (r.height() * 0.035).max(1.0);
        let bar = Rect::from_min_max(pos2(x + w * 0.5, r.min.y), pos2(x + w * 1.5, r.max.y));
        let mut lk = Look::new(1.0);
        lk.alpha = o.alpha;
        let mut m = Mesh::default();
        let (lo, hi) = (bar.min - l.center, bar.max - l.center);
        for q in [lo, vec2(hi.x, lo.y), hi, vec2(lo.x, hi.y)] {
            m.vertices.push(egui::epaint::Vertex { pos: map.pt(place(l.center, l.angle, &lk, q)), uv: egui::epaint::WHITE_UV, color: Color32::from_black_alpha((o.alpha * 230.0) as u8) });
        }
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        p.add(m);
    }

    fn look(&self, o: &ObjPiece, t: f32) -> Option<Look> {
        self.look_at(o, t, None)
    }

    /// The look at `t`; with `unit` = (k, n), for the k-th of n text pieces delivered one after
    /// another (each overlapping the next), whose own progress replaces the build's.
    fn look_at(&self, o: &ObjPiece, t: f32, unit: Option<(usize, usize)>) -> Option<Look> {
        // a piece of text (character / word / line) travels from its own place, not the box's
        // (R43: each letter starts just off the slide and flies to its spot)
        let rect = unit.and_then(|(k, _)| o.units.get(k).copied()).unwrap_or(o.env_rect);
        let env = Env { rect, slide: self.slide };
        let mut visible = o.visible;
        let mut lk = Look::new(o.alpha);
        let mut alpha = o.alpha;
        for &i in &o.tracks {
            let tr = &self.tracks[i];
            if t < tr.start {
                continue;
            }
            let mut p = if tr.dur <= 0.0 { 1.0 } else { ((t - tr.start) / tr.dur).clamp(0.0, 1.0) };
            if let (Some((k, n)), true) = (unit, tr.units > 0) {
                // each piece starts 0.2 of a piece's time after the one before (R43 frames:
                // a soft front about five letters wide, done as the build ends)
                const OVERLAP: f32 = 0.2;
                if tr.fx == Fx::Keyboard {
                    // typed: each letter appears whole, evenly from 0.21 to 0.78 (R43 movie)
                    p = if p >= keyboard_at(k, n) { 1.0 } else { 0.0 };
                } else {
                    let len = 1.0 / (1.0 + (n.saturating_sub(1)) as f32 * OVERLAP);
                    let from = k as f32 * OVERLAP * len;
                    p = ((p - from) / len).clamp(0.0, 1.0);
                }
                if p <= 0.0 && tr.kind == Kind::In {
                    continue;
                }
            }
            match tr.kind {
                Kind::In => {
                    visible = true;
                    look_in(&mut lk, tr.fx, &tr.build, p, &env);
                }
                Kind::Out => {
                    if p >= 1.0 {
                        visible = false;
                    } else {
                        look_out(&mut lk, tr.fx, &tr.build, p, &env);
                    }
                }
                Kind::Action => {
                    if tr.fx == Fx::Opacity {
                        let target = tr.build.params.opacity.unwrap_or(0.5).clamp(0.0, 1.0);
                        let na = alpha + (target - alpha) * accel(tr.build.params.acceleration, p);
                        lk.alpha *= na / alpha.max(1e-3);
                        if p >= 1.0 {
                            alpha = target;
                        }
                    } else {
                        let empty = Vec::new();
                        look_action(&mut lk, tr.fx, &tr.build, p, o.paths.get(&i).unwrap_or(&empty));
                    }
                }
                Kind::Gallery => {
                    let Some((incoming, caption)) = o.gallery else { continue };
                    let e = smooth(p);
                    let name = effect_name(&tr.build.effect);
                    let fade = caption || name.contains("dissolve") || name.contains("fade");
                    if tr.dur <= 0.0 || name.contains("appear") {
                        visible = incoming;
                        continue;
                    }
                    if incoming {
                        visible = true;
                    } else if p >= 1.0 {
                        visible = false;
                        continue;
                    }
                    if fade {
                        lk.alpha *= if incoming { e } else { 1.0 - e };
                    }
                    if !caption && name.contains("move") {
                        // Move In: the images travel as a strip, right to left, with a gap
                        // (Fade and Move: a short move while fading)
                        let dist = if name.contains("fade") { env.rect.width() * 0.25 } else { env.rect.width() + 40.0 };
                        lk.offset.x += if incoming { dist * (1.0 - e) } else { -dist * e };
                    }
                }
                _ => {}
            }
        }
        visible.then_some(lk)
    }
}

/// Draw a state's live movie frames over its static texture.
pub fn draw_movie(p: &Painter, map: &Map, d: &Drawable, g: Geometry, x: &Xform, tex: TextureId) {
    let c = g.center();
    let center = pos2(c.x + x.dx, c.y + x.dy);
    let size = vec2(g.w, g.h) * x.scale;
    let mut lk = Look::new(x.opacity.unwrap_or(d.opacity));
    if g.hflip {
        lk.sx = -1.0;
    }
    if g.vflip {
        lk.sy = -1.0;
    }
    draw_textured(p, map, tex, Rect::from_center_size(center, size), center, x.rot - g.angle.to_radians(), &lk);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kn_model::{BuildParams, BuildStep};

    fn catalog() -> (Document, Vec<kn_model::SlideInfo>) {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let doc = Document::open(p).unwrap();
        let s = doc.slides();
        (doc, s)
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn in_action_out_states() {
        let (doc, slides) = catalog();
        let sc = doc.scene(&slides, 130).unwrap();
        let id = sc.builds[0].drawable;
        // build order: pop in, rotate 45° counter-clockwise, dissolve out
        assert!(state_at(&sc, 0).hidden.contains(&id));
        let s1 = state_at(&sc, 1);
        assert!(!s1.hidden.contains(&id));
        assert_eq!(s1.xform(id), Xform::default());
        let s2 = state_at(&sc, 2);
        assert!((s2.xform(id).rot + 45f32.to_radians()).abs() < 1e-4);
        assert!(state_at(&sc, 3).hidden.contains(&id));
        let (to, tracks) = timeline(&sc, 0);
        assert_eq!(to, 1);
        assert_eq!(tracks[0].fx, Fx::Pop);
    }

    #[test]
    fn effect_names() {
        let b = |kind: &str, effect: &str| Build { id: 1, drawable: 2, kind: kind.into(), effect: effect.into(), delivery: String::new(), duration: 1.0, direction: 0, params: BuildParams::default() };
        assert_eq!(fx_of(&b("In", "apple:move in character")), Fx::Move { fly: false });
        // R43: Keynote 15.4's stored ids
        assert_eq!(fx_of(&b("In", "apple:sidezoom")), Fx::Move { fly: true });
        assert_eq!(fx_of(&b("In", "com.apple.iWork.Keynote.FromDarkness")), Fx::FadeScale);
        assert_eq!(fx_of(&b("In", "com.apple.iWork.Keynote.BUKAnvil")), Fx::Anvil);
        assert_eq!(fx_of(&b("In", "com.apple.iWork.Keynote.KLNBouncy")), Fx::Bouncy);
        assert_eq!(fx_of(&b("In", "apple:wipe-iris")), Fx::Iris);
        assert_eq!(fx_of(&b("In", "apple:bc-3D-cube")), Fx::Cube);
        assert_eq!(fx_of(&b("In", "apple:bc-flip")), Fx::FlipOver);
        assert_eq!(fx_of(&b("In", "apple:zoom character")), Fx::Scale { big: false });
        assert_eq!(fx_of(&b("In", "com.apple.iWork.Keynote.Blur")), Fx::Blur);
        assert_eq!(fx_of(&b("In", "com.apple.iWork.Keynote.LineDrawForLine")), Fx::LineDraw);
        assert_eq!(fx_of(&b("In", "apple:bc-appear")), Fx::Appear);
        assert_eq!(fx_of(&b("In", "apple:bc-zoom-big")), Fx::Scale { big: true });
        assert_eq!(fx_of(&b("Action", "apple:action-motion-path")), Fx::Path);
        assert_eq!(fx_of(&b("In", "apple:something-new")), Fx::Dissolve);
        assert_eq!(kind_of(&b("In", "apple:movie-start")), Kind::Media(true));
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn delivery_by_paragraph() {
        let (doc, slides) = catalog();
        let (mut sc, d) = (0..slides.len())
            .find_map(|i| {
                let sc = doc.scene(&slides, i)?;
                let d = sc.drawables.iter().find(|d| !d.from_template && non_empty_paras(d) >= 3)?.clone();
                Some((sc, d))
            })
            .expect("a slide with a 3+ paragraph text box");
        let n = non_empty_paras(&d);
        sc.builds = vec![Build { id: 99, drawable: d.id, kind: "In".into(), effect: "apple:dissolve".into(), delivery: "By Paragraph".into(), duration: 1.0, direction: 0, params: BuildParams::default() }];
        sc.build_order = (0..n).map(|c| BuildStep { build: 99, delay: 0.0, duration: 1.0, automatic: false, with_previous: false, chunk: c as u32 + 1 }).collect();
        assert!(state_at(&sc, 0).hidden.contains(&d.id));
        assert_eq!(state_at(&sc, 1).paras.get(&d.id), Some(&(0, 1)));
        assert_eq!(state_at(&sc, 2).paras.get(&d.id), Some(&(0, 2)));
        let done = state_at(&sc, n);
        assert!(!done.hidden.contains(&d.id) && !done.paras.contains_key(&d.id));
        let (_, tracks) = timeline(&sc, 1);
        assert_eq!(tracks[0].paras, Some((1, 2)));
        // paragraphs outside the range turn transparent
        let m = masked(&d, 0, 1);
        let DrawableKind::Shape(s) = &m.kind else { panic!() };
        let t = s.text.as_ref().unwrap();
        let visible: Vec<bool> = t.paragraphs.iter().filter(|p| !t.text[p.start..p.end].trim().is_empty()).map(|p| p.runs.iter().any(|r| r.style.color.a > 0.0)).collect();
        assert!(visible[0] && !visible[1]);
    }
}
