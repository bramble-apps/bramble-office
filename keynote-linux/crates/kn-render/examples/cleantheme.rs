//! cleantheme <in.key> <out.key> <photo-ids>: a theme whose photos were already swapped for
//! openly licensed ones, made into a file of our own. `photo-ids` lists the data ids of those
//! photos (comma-separated); they're kept. Everything else Apple made is redrawn here: the
//! layout and slide thumbnails (rendered from the layouts), the dynamic backgrounds' still
//! frames (rendered by kn-render's motion renderer), each photo's low-resolution copy (made from
//! the photo it belongs to), the package previews (the first slide), and the package's
//! identifiers (a new document id; Keydeck instead of Keynote's build list). Textures,
//! backgrounds, decorative rules, title strips and bullets become ones drawn here with the
//! original's size and colours; stock photos (Apple's, or Getty's by link) become photos from
//! the pool directory (printed as PHOTO lines, for the credits). Nothing of the original is
//! left but the structure.
use std::collections::{BTreeMap, BTreeSet};

use image::{DynamicImage, ImageEncoder, RgbaImage};
use kn_format::keyfile::{Entry, EntryData};
use kn_format::{KeyFile, Msg, Val};
use kn_model::Document;

fn encode(im: &RgbaImage, png: bool) -> Vec<u8> {
    let mut out = Vec::new();
    if png {
        image::codecs::png::PngEncoder::new(&mut out).write_image(im.as_raw(), im.width(), im.height(), image::ExtendedColorType::Rgba8).unwrap();
    } else {
        let rgb = DynamicImage::ImageRgba8(im.clone()).to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88).write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8).unwrap();
    }
    out
}

/// Every data id a message (and its sub-messages) refers to.
fn data_refs(m: &Msg, out: &mut Vec<u64>) {
    if m.type_name() == "TSP.DataReference"
        && let Some(id) = m.u64("identifier")
    {
        out.push(id);
    }
    for f in &m.fields {
        if let Val::Msg(c) = &f.val {
            data_refs(c, out);
        }
    }
}

/// A scene rendered `w` × `h` (its dynamic background drawn at t = 0).
fn render(doc: &Document, sc: &kn_model::SlideScene, w: u32, h: u32) -> RgbaImage {
    let mut pm = tiny_skia::Pixmap::new(w.max(1), h.max(1)).unwrap();
    if let Some(m) = &sc.motion {
        pm.data_mut().copy_from_slice(&kn_render::motion::render(m, 0.0, w.max(1), h.max(1)));
    }
    let k = (w as f32 / sc.size.w).min(h as f32 / sc.size.h);
    let mut r = kn_render::Renderer::new();
    r.draw_scene(doc, sc, &mut pm, tiny_skia::Transform::from_scale(k, k), kn_render::Filter::All, sc.motion.is_none());
    // (tiny-skia keeps premultiplied pixels)
    let mut im = RgbaImage::new(pm.width(), pm.height());
    for (p, q) in pm.pixels().iter().zip(im.pixels_mut()) {
        let c = p.demultiply();
        *q = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    im
}

/// A 20 × 20 pt PDF holding a filled dot (or square) of grey `c` (0 black .. 1 white).
fn bullet_pdf(c: f32, square: bool) -> Vec<u8> {
    let path = if square {
        "4 4 12 12 re f".to_string()
    } else {
        let (cx, cy, r) = (10.0f32, 10.0f32, 6.0f32);
        let k = 0.5523 * r;
        format!(
            "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c f",
            cx + r, cy,
            cx + r, cy + k, cx + k, cy + r, cx, cy + r,
            cx - k, cy + r, cx - r, cy + k, cx - r, cy,
            cx - r, cy - k, cx - k, cy - r, cx, cy - r,
            cx + k, cy - r, cx + r, cy - k, cx + r, cy
        )
    };
    let content = format!("{c:.3} g {path}\n");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Contents 4 0 R /Resources << >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
    ];
    let mut out = String::from("%PDF-1.4\n");
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
    }
    let xref = out.len();
    out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1));
    for o in offs {
        out.push_str(&format!("{o:010} 00000 n \n"));
    }
    out.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1));
    out.into_bytes()
}

/// The openly licensed photos (named by their credit's data id) standing in for stock photos.
#[derive(Default)]
struct Pool(Vec<(String, std::path::PathBuf)>);

impl Pool {
    fn load(dir: &String) -> Pool {
        let mut v: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter_map(|p| Some((p.file_stem()?.to_string_lossy().to_string(), p)))
            .collect();
        v.sort();
        Pool(v)
    }
    fn pick(&self, seed: u64) -> Option<(String, image::DynamicImage)> {
        if self.0.is_empty() {
            return None;
        }
        let (id, p) = &self.0[(seed.wrapping_mul(2654435761) % self.0.len() as u64) as usize];
        Some((id.clone(), image::open(p).ok()?))
    }
}

/// "…_1869x1399…" in a file name.
fn regex_size(name: &str) -> Option<(u32, u32)> {
    name.split(|c: char| !c.is_ascii_alphanumeric()).find_map(|part| {
        let (a, b) = part.split_once('x')?;
        let (w, h) = (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?);
        (w >= 100 && h >= 100).then_some((w, h))
    })
}

/// What an image is, by its name.
fn classify(n: &str) -> &'static str {
    if n.contains("blankmovieposter") {
        "movie poster"
    } else if n.contains("line") {
        "line"
    } else if n.contains("tile") || n.contains("texture") || n.contains("noise") || n.contains("grain") {
        "texture"
    } else if n.contains("_hd") || n.contains("gradient") {
        "background"
    } else if n.contains("scrapbook") || n.contains("title_hi") {
        "strip"
    } else if n.contains("bullet") {
        "bullet"
    } else {
        "photo"
    }
}

/// Mean and spread of each channel (opaque pixels only), and the share that's opaque.
fn stats(im: &image::DynamicImage) -> ([f32; 3], [f32; 3], f32) {
    let rgba = im.to_rgba8();
    let (mut s, mut q, mut n) = ([0f64; 3], [0f64; 3], 0f64);
    for p in rgba.pixels() {
        if p[3] < 128 {
            continue;
        }
        for c in 0..3 {
            let v = p[c] as f64;
            s[c] += v;
            q[c] += v * v;
        }
        n += 1.0;
    }
    let n1 = n.max(1.0);
    let mean = [0, 1, 2].map(|c| (s[c] / n1) as f32);
    let sd = [0, 1, 2].map(|c| ((q[c] / n1 - (s[c] / n1).powi(2)).max(0.0).sqrt()) as f32);
    (mean, sd, (n / rgba.pixels().len().max(1) as f64) as f32)
}

/// A small deterministic noise source.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32 - 0.5
    }
}

/// A grain texture of our own with the original's size, colour and roughness (tiles cleanly:
/// the grain is smoothed with wrap-around).
fn texture(o: &image::DynamicImage, seed: u64) -> RgbaImage {
    let (w, h) = (o.width().max(1), o.height().max(1));
    let (mean, sd, _) = stats(o);
    let mut rng = Rng(seed | 1);
    let raw: Vec<f32> = (0..w * h).map(|_| rng.next()).collect();
    let at = |x: i64, y: i64| raw[(y.rem_euclid(h as i64) * w as i64 + x.rem_euclid(w as i64)) as usize];
    let mut im = RgbaImage::new(w, h);
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let v = (at(x, y) * 2.0 + at(x - 1, y) + at(x + 1, y) + at(x, y - 1) + at(x, y + 1)) / 6.0 * 3.2;
            let px = [0, 1, 2].map(|c| (mean[c] + v * sd[c].max(2.0) * 1.6).clamp(0.0, 255.0) as u8);
            im.put_pixel(x as u32, y as u32, image::Rgba([px[0], px[1], px[2], 255]));
        }
    }
    im
}

/// A smooth background of our own: the original's colours on a coarse 4 × 4 grid, blended
/// across, with a fine grain of its roughness.
fn gradient(o: &image::DynamicImage, seed: u64, streaks: bool) -> RgbaImage {
    let (w, h) = (o.width().max(1), o.height().max(1));
    let small = o.resize_exact(4, 4, image::imageops::FilterType::Triangle).to_rgb8();
    let (_, sd, _) = stats(&o.resize(256, 256, image::imageops::FilterType::Triangle));
    let mut rng = Rng(seed | 1);
    // brushed metal: fine vertical streaks, each column a little lighter or darker
    let cols: Vec<f32> = (0..w).map(|_| if streaks { rng.next() * 14.0 } else { 0.0 }).collect();
    let mut im = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f32 / w as f32 * 3.0, y as f32 / h as f32 * 3.0);
            let (x0, y0) = (u.floor() as u32, v.floor() as u32);
            let (x1, y1) = ((x0 + 1).min(3), (y0 + 1).min(3));
            let (fx, fy) = (u - x0 as f32, v - y0 as f32);
            let s = |a: u32, b: u32, c: usize| small.get_pixel(a, b)[c] as f32;
            let g = rng.next() * 6.0 + cols[x as usize];
            let px = [0, 1, 2].map(|c| {
                let top = s(x0, y0, c) * (1.0 - fx) + s(x1, y0, c) * fx;
                let bot = s(x0, y1, c) * (1.0 - fx) + s(x1, y1, c) * fx;
                (top * (1.0 - fy) + bot * fy + g * (sd[c] / 40.0).min(1.0)).clamp(0.0, 255.0) as u8
            });
            im.put_pixel(x, y, image::Rgba([px[0], px[1], px[2], 255]));
        }
    }
    im
}

/// A decorative rule of our own (dots, pluses, triangles, dashes, a wave, a chalk stroke or a
/// plain line) the original's size and colour, along its longer side.
fn line(o: &image::DynamicImage, name: &str) -> RgbaImage {
    let (w, h) = (o.width().max(1), o.height().max(1));
    let (mean, _, _) = stats(o);
    // the original's opacity where it draws, and how much of its width the drawing fills
    let rgba = o.to_rgba8();
    let drawn: Vec<u8> = rgba.pixels().map(|p| p[3]).filter(|&a| a > 16).collect();
    let alpha = if drawn.is_empty() { 255 } else { (drawn.iter().map(|&a| a as u32).sum::<u32>() / drawn.len() as u32) as u8 };
    let col = image::Rgba([mean[0] as u8, mean[1] as u8, mean[2] as u8, alpha]);
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(col[0], col[1], col[2], alpha);
    paint.anti_alias = true;
    let horiz = w >= h;
    let (len, full) = if horiz { (w as f32, h as f32) } else { (h as f32, w as f32) };
    // (the rule's own thickness: the rows across it that the original draws in)
    let across = (0..full as u32)
        .filter(|&b| (0..len as u32).any(|a| if horiz { rgba.get_pixel(a, b)[3] > 40 } else { rgba.get_pixel(b, a)[3] > 40 }))
        .count() as f32;
    let th = across.clamp(1.0, full);
    let at = |a: f32, b: f32| if horiz { (a, b) } else { (b, a) };
    let mut pb = tiny_skia::PathBuilder::new();
    let step = (th * 2.2).max(3.0);
    let fill_rect = |pb: &mut tiny_skia::PathBuilder, a: f32, b: f32, la: f32, lb: f32| {
        let ((x, y), (x2, y2)) = (at(a, b), at(a + la, b + lb));
        if let Some(r) = tiny_skia::Rect::from_ltrb(x.min(x2), y.min(y2), x.max(x2), y.max(y2)) {
            pb.push_rect(r);
        }
    };
    if name.contains("dots") {
        let mut a = th / 2.0;
        while a < len {
            let (x, y) = at(a, th / 2.0);
            pb.push_circle(x, y, th * 0.42);
            a += step;
        }
    } else if name.contains("plus") {
        let mut a = th / 2.0;
        while a < len {
            fill_rect(&mut pb, a - th * 0.45, th * 0.38, th * 0.9, th * 0.24);
            fill_rect(&mut pb, a - th * 0.12, 0.05 * th, th * 0.24, th * 0.9);
            a += step * 1.3;
        }
    } else if name.contains("tri") {
        let mut a = 0.0;
        while a < len {
            let (p, q, r) = (at(a, th * 0.95), at(a + th, th * 0.95), at(a + th / 2.0, th * 0.05));
            pb.move_to(p.0, p.1);
            pb.line_to(q.0, q.1);
            pb.line_to(r.0, r.1);
            pb.close();
            a += step * 1.2;
        }
    } else if name.contains("dash") {
        let mut a = 0.0;
        while a < len {
            fill_rect(&mut pb, a, th * 0.3, th * 3.0, th * 0.4);
            a += th * 4.5;
        }
    } else if name.contains("wave") {
        let n = (len / (th * 4.0).max(4.0)).ceil().max(1.0) as usize * 8;
        for i in 0..=n {
            let a = len * i as f32 / n as f32;
            let b = th / 2.0 + (a / (th * 4.0).max(4.0) * std::f32::consts::TAU).sin() * th * 0.3;
            let (x, y) = at(a, b);
            if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
        }
        let path = pb.finish().unwrap();
        let stroke = tiny_skia::Stroke { width: (th * 0.25).max(1.0), ..Default::default() };
        pm.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
        return pixmap_rgba(&pm);
    } else if name.contains("chalk") {
        // a loose doodle: a thin dotted line across the picture with a few loops in it
        let (wf, hf) = (w as f32, h as f32);
        let r = (wf.min(hf) * 0.007).max(1.0);
        let loop_r = wf.min(hf) * 0.06;
        let mut last: Option<(f32, f32)> = None;
        let n = 4000;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            // a wandering line plus a trochoid: small loops along it
            let a = t * std::f32::consts::TAU * 3.0;
            let (mut x, mut y) = if horiz {
                (wf * t, hf * (0.5 + 0.3 * (t * std::f32::consts::TAU * 1.1).sin()))
            } else {
                (wf * (0.5 + 0.3 * (t * std::f32::consts::TAU * 1.1).sin()), hf * t)
            };
            x += loop_r * a.sin();
            y += loop_r * (1.0 - a.cos()) * 0.8;
            if last.is_none_or(|(lx, ly)| (x - lx).hypot(y - ly) >= r * 3.2) {
                pb.push_circle(x, y, r);
                last = Some((x, y));
            }
        }
        paint.set_color_rgba8(col[0], col[1], col[2], alpha.min(200));
        if let Some(path) = pb.finish() {
            pm.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
        }
        return pixmap_rgba(&pm);
    } else {
        fill_rect(&mut pb, 0.0, th * 0.25, len, th * 0.5);
    }
    if let Some(path) = pb.finish() {
        pm.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
    }
    pixmap_rgba(&pm)
}

fn pixmap_rgba(pm: &tiny_skia::Pixmap) -> RgbaImage {
    let mut im = RgbaImage::new(pm.width(), pm.height());
    for (p, q) in pm.pixels().iter().zip(im.pixels_mut()) {
        let c = p.demultiply();
        *q = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    im
}

/// A paper strip of our own (a title banner): the original's colour with a light grain and
/// slightly ragged edges.
fn strip(o: &image::DynamicImage, seed: u64) -> RgbaImage {
    let (w, h) = (o.width().max(1), o.height().max(1));
    let (mean, _, _) = stats(o);
    let mut rng = Rng(seed | 3);
    let mut im = RgbaImage::from_fn(w, h, |_, _| {
        let g = rng.next() * 5.0;
        image::Rgba([0, 1, 2].map(|c| (mean[c] + g).clamp(0.0, 255.0) as u8).into_iter().chain([255]).collect::<Vec<_>>().try_into().unwrap())
    });
    let edge = (h as f32 * 0.025).max(1.0);
    for x in 0..w {
        let (t, b) = (edge * (0.6 + rng.next()), edge * (0.6 + rng.next()));
        for y in 0..h {
            let yf = y as f32;
            if yf < t || yf > h as f32 - b {
                im.get_pixel_mut(x, y)[3] = 0;
            }
        }
    }
    im
}

/// A round bullet of our own in the original's colour.
fn dot(o: &image::DynamicImage) -> RgbaImage {
    let (w, h) = (o.width().max(1), o.height().max(1));
    let (mean, _, _) = stats(o);
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(mean[0] as u8, mean[1] as u8, mean[2] as u8, 255);
    paint.anti_alias = true;
    if let Some(c) = tiny_skia::PathBuilder::from_circle(w as f32 / 2.0, h as f32 / 2.0, w.min(h) as f32 * 0.4) {
        pm.fill_path(&c, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
    }
    pixmap_rgba(&pm)
}

fn wash(w: u32, h: u32, seed: u64) -> RgbaImage {
    let g = 200 + (seed % 40) as u8;
    let mut im = RgbaImage::from_pixel(w.max(1), h.max(1), image::Rgba([g, g, g, 255]));
    im.put_pixel(0, 0, image::Rgba([(seed & 255) as u8, (seed >> 8 & 255) as u8, (seed >> 16 & 255) as u8, 255]));
    im
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: cleantheme <in.key> <out.key> <photo ids, comma-separated> [photo pool dir]");
        std::process::exit(2);
    }
    let photos: BTreeSet<u64> = a[3].split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let doc = Document::open(&a[1]).expect("open");
    let mut k = KeyFile::from_zip(&std::fs::read(&a[1]).unwrap()).unwrap();

    // what each image belongs to: a slide or layout (its thumbnails), a dynamic background
    // (its still frame)
    let layouts: BTreeSet<_> = doc.layouts().into_iter().map(|(id, _)| id).collect();
    let slides = doc.slides();
    let mut owner: BTreeMap<u64, kn_format::ObjId> = BTreeMap::new();
    for (_, ar) in k.archives() {
        let ty = k.type_name(ar.id());
        // (thumbnails hang off the slide's node in the navigator)
        let slide = match ty {
            "KN.SlideArchive" => Some(ar.id()),
            "KN.SlideNodeArchive" => ar.obj().ref_ids("slide").first().copied(),
            _ => None,
        };
        if let Some(slide) = slide {
            let mut ids = Vec::new();
            data_refs(ar.obj(), &mut ids);
            for d in ids {
                owner.entry(d).or_insert(slide);
            }
        }
    }
    if std::env::var_os("CLEAN_DEBUG").is_some() {
        let mut any: BTreeMap<u64, Vec<&str>> = BTreeMap::new();
        for (_, ar) in k.archives() {
            let mut ids = Vec::new();
            data_refs(ar.obj(), &mut ids);
            for d in ids {
                any.entry(d).or_default().push(k.type_name(ar.id()));
            }
        }
        for (d, t) in &any {
            if !owner.contains_key(d) {
                eprintln!("{d} {:?} {:?}", k.data(*d).map(|x| x.0.to_string()), t);
            }
        }
    }
    let mut poster: BTreeMap<u64, kn_model::motionbg::MotionBg> = BTreeMap::new();
    let styles: Vec<_> = k.objects_of("KN.MotionBackgroundStyleArchive").map(|(id, _)| id).collect();
    for s in styles {
        if let Some(m) = kn_model::motionbg::read(&k, s)
            && let Some(p) = m.poster
        {
            poster.insert(p, m);
        }
    }

    // the data, photos first, so a low-resolution copy can follow its photo's replacement
    let datas: Vec<(u64, String, Option<(u32, u32)>)> = k
        .package_metadata()
        .map(|pm| {
            pm.msgs("datas")
                .filter_map(|d| {
                    let id = d.u64("identifier")?;
                    let name = d.str("preferred_file_name").or(d.str("file_name")).unwrap_or("").to_string();
                    let attr = d.msg("attributes").and_then(|a| Some((a.f32("width")? as u32, a.f32("height")? as u32)));
                    Some((id, name, attr))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut order: Vec<_> = datas.iter().collect();
    order.sort_by_key(|(_, n, _)| n.contains("-small"));
    let pool = a.get(4).map(Pool::load).unwrap_or_default();
    let mut replaced: BTreeMap<String, image::DynamicImage> = BTreeMap::new();
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for (id, name, attr) in order.into_iter().map(|(i, n, a)| (*i, n.clone(), *a)) {
        let bytes = k.data(id).map(|(_, d)| d.into_owned()).unwrap_or_default();
        let lname = name.to_ascii_lowercase();
        let ext = lname.rsplit('.').next().unwrap_or("jpg").to_string();
        let base = |n: &str| n.rsplit('/').next().unwrap_or(n).replace("-small", "").rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or_default();
        if lname.ends_with(".pdf") {
            // image bullets: a dot of our own, white or dark as the original
            let c = if lname.contains("white") { 1.0 } else if lname.contains("charcoal") { 0.2 } else { 0.0 };
            k.replace_data_bytes(id, bullet_pdf(c, lname.contains("headlines") || lname.contains("metal")), "bullet.pdf");
            *count.entry("bullets").or_default() += 1;
            continue;
        }
        if !["jpg", "jpeg", "png", "tif", "tiff", "gif", "heic"].contains(&ext.as_str()) {
            continue;
        }
        let im = image::load_from_memory(&bytes).ok();
        if photos.contains(&id) {
            // (kept, under a plain name: the old ones named the Apple theme)
            if let Some(im) = &im {
                replaced.insert(base(&name), im.clone());
            }
            k.replace_data_bytes(id, bytes, &format!("photo.{ext}"));
            *count.entry("photos kept").or_default() += 1;
            continue;
        }
        // the size: the picture's own, else its recorded size, else one in its name (1869x1399)
        let named = regex_size(&name);
        let (w, h) = im.as_ref().map(|i| (i.width(), i.height())).or(attr).or(named).unwrap_or((1600, 1067));
        let png = ext == "png" || im.as_ref().is_some_and(|i| i.color().has_alpha());
        let kind = classify(&lname);
        let (out, what): (RgbaImage, &str) = if let Some(m) = poster.get(&id) {
            let px = kn_render::motion::render(m, 0.0, w, h);
            (RgbaImage::from_raw(w, h, px).unwrap_or_else(|| wash(w, h, id)), "dynamic background frames")
        } else if let Some(&s) = owner.get(&id) {
            let sc = if layouts.contains(&s) { doc.layout_scene(s) } else { slides.iter().position(|x| x.slide == s).and_then(|i| doc.scene(&slides, i)) };
            match sc {
                Some(sc) => (render(&doc, &sc, w, h), "thumbnails"),
                None => (wash(w, h, id), "washes"),
            }
        } else if lname.contains("-small") && replaced.contains_key(&base(&name)) {
            (replaced[&base(&name)].resize_to_fill(w, h, image::imageops::FilterType::Triangle).to_rgba8(), "low-resolution copies")
        } else if lname.contains("-small") && let Some(b) = photos.range(..id).next_back().and_then(|p| k.data(*p)).and_then(|(_, b)| image::load_from_memory(&b).ok()) {
            (b.resize_to_fill(w, h, image::imageops::FilterType::Triangle).to_rgba8(), "low-resolution copies")
        } else {
            match (kind, &im) {
                ("movie poster", _) => (wash(w, h, id), "washes"),
                ("texture", Some(o)) if lname.contains("metal") => (gradient(o, id, true), "textures"),
                ("texture", Some(o)) => (texture(o, id), "textures"),
                ("background", Some(o)) => (gradient(o, id, lname.contains("industrial") || lname.contains("metal")), "backgrounds"),
                ("line", Some(o)) => (line(o, &lname), "lines"),
                ("strip", Some(o)) => (strip(o, id), "strips"),
                ("bullet", Some(o)) => (dot(o), "bullets"),
                _ => {
                    // a stock photo (Apple's or Getty's): one of the openly licensed ones
                    match pool.pick(id) {
                        Some((pid, p)) => {
                            println!("PHOTO\t{id}\t{pid}");
                            let out = p.resize_to_fill(w, h, image::imageops::FilterType::Triangle);
                            replaced.insert(base(&name), out.clone());
                            (out.to_rgba8(), "photos replaced")
                        }
                        None => (wash(w, h, id), "washes"),
                    }
                }
            }
        };
        if what == "washes" && std::env::var_os("CLEAN_LIST").is_some() {
            eprintln!("WASH\t{id}\t{name}\t{w}x{h}");
        }
        *count.entry(what).or_default() += 1;
        // (plain names too: the old ones named Apple's themes and stock photos)
        let new_name = match what {
            "dynamic background frames" => format!("motionBackground.{ext}"),
            "low-resolution copies" => format!("photo-small.{ext}"),
            "photos replaced" => format!("photo.{ext}"),
            // (Keynote's own prefixes: mt- a layout's thumbnail, st- a slide's)
            "thumbnails" => format!("{}-thumbnail.{ext}", if name.rsplit('/').next().unwrap_or("").starts_with("mt-") { "mt" } else { "st" }),
            "textures" | "backgrounds" => format!("texture.{ext}"),
            _ => format!("image.{ext}"),
        };
        k.replace_data_bytes(id, encode(&out, png), &new_name);
    }

    // the package previews: the first slide
    let first = doc.scene(&slides, 0);
    let previews: Vec<String> = k
        .entries
        .iter()
        .filter(|e| matches!(e.data, EntryData::Raw(_)) && !e.name.starts_with("Data/") && !e.name.starts_with("Index/"))
        .filter(|e| [".jpg", ".jpeg", ".png", ".tiff"].iter().any(|x| e.name.to_ascii_lowercase().ends_with(x)))
        .map(|e| e.name.clone())
        .collect();
    for n in previews {
        let Some(Entry { data: EntryData::Raw(raw), .. }) = k.entry(&n) else { continue };
        let Ok(im) = image::load_from_memory(raw) else { continue };
        let out = match &first {
            Some(sc) => render(&doc, sc, im.width(), im.height()),
            None => wash(im.width(), im.height(), 1),
        };
        let png = n.to_ascii_lowercase().ends_with(".png");
        k.remove_entry(&n);
        k.add_entry(Entry { name: n, data: EntryData::Raw(std::sync::Arc::new(encode(&out, png))) });
        *count.entry("previews").or_default() += 1;
    }

    // the package's own identifiers
    let uuid = {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let x = t ^ (std::process::id() as u128) << 64 ^ a[2].bytes().fold(0u128, |h, b| h.wrapping_mul(131).wrapping_add(b as u128));
        let h = format!("{x:032X}");
        format!("{}-{}-4{}-A{}-{}", &h[0..8], &h[8..12], &h[13..16], &h[17..20], &h[20..32])
    };
    for (name, text) in [
        ("Metadata/DocumentIdentifier", uuid.clone()),
        ("Metadata/BuildVersionHistory.plist", "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<array>\n\t<string>Keydeck</string>\n</array>\n</plist>\n".to_string()),
    ] {
        if k.entry(name).is_some() {
            k.remove_entry(name);
            k.add_entry(Entry { name: name.to_string(), data: EntryData::Raw(std::sync::Arc::new(text.into_bytes())) });
        }
    }
    if let Some(Entry { data: EntryData::Raw(raw), .. }) = k.entry("Metadata/Properties.plist") {
        let s = String::from_utf8_lossy(raw).to_string();
        let mut out = String::new();
        let mut next_is_uuid = false;
        for line in s.lines() {
            if next_is_uuid && line.trim_start().starts_with("<string>") {
                out.push_str(&format!("\t<string>{uuid}</string>\n"));
                next_is_uuid = false;
                continue;
            }
            next_is_uuid = line.contains("<key>documentUUID</key>");
            out.push_str(line);
            out.push('\n');
        }
        k.remove_entry("Metadata/Properties.plist");
        k.add_entry(Entry { name: "Metadata/Properties.plist".into(), data: EntryData::Raw(std::sync::Arc::new(out.into_bytes())) });
    }
    std::fs::write(&a[2], k.to_zip().expect("write")).expect("save");
    Document::open(&a[2]).expect("reopens");
    println!("{}: {count:?}", a[2]);
}
