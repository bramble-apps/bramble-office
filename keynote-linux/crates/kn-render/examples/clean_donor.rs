//! clean_donor <in> <out> [media-dir]: an Apple-made document (a built-in theme, template or
//! donor) with every image replaced by an original one of the same size, its movies, sounds and
//! PDFs by the `placeholder-<data id>.<ext>` files in media-dir, and new preview images, so that
//! only the document's structure is left of the original. Works on .key, .numbers and .pages.
//! `clean_donor --list <in>` prints the data ids and names that aren't images.
use std::hash::{Hash, Hasher};

use image::{ImageEncoder, Rgb, RgbImage, Rgba, RgbaImage};
use kn_format::keyfile::{Entry, EntryData};
use kn_format::KeyFile;

/// A soft two-tone wash with a few large rings, its colours and layout chosen by `seed` (every
/// replacement differs: Keynote won't open a file where two data files share a digest).
fn wash(w: u32, h: u32, seed: u64) -> RgbImage {
    let mut hs = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut hs);
    let r = hs.finish();
    let pick = |n: u32| ((r >> (n * 8)) & 0xff) as f32 / 255.0;
    // muted greens, greys and warm neutrals
    let a = [0.78 + 0.12 * pick(0), 0.80 + 0.10 * pick(1), 0.74 + 0.10 * pick(2)];
    let b = [0.42 + 0.20 * pick(3), 0.50 + 0.16 * pick(4), 0.44 + 0.14 * pick(5)];
    let rings: Vec<(f32, f32, f32)> = (0..3).map(|i| (pick(i + 2), pick(i + 4), 0.18 + 0.25 * pick(i + 1))).collect();
    let (wf, hf) = (w.max(1) as f32, h.max(1) as f32);
    let mut im = RgbImage::new(w.max(1), h.max(1));
    for (x, y, px) in im.enumerate_pixels_mut() {
        let (u, v) = (x as f32 / wf, y as f32 / hf);
        let t = (u * 0.6 + v * 0.4).clamp(0.0, 1.0);
        let mut c = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
        for &(cx, cy, rad) in &rings {
            let d = (((u - cx) * wf).hypot((v - cy) * hf)) / wf.max(hf);
            let ring = (1.0 - ((d - rad).abs() / 0.012).min(1.0)) * 0.10;
            c = c.map(|k| (k + ring).min(1.0));
        }
        *px = Rgb(c.map(|k| (k * 255.0) as u8));
    }
    // (and a pixel that only this seed has)
    im.put_pixel(0, 0, Rgb([(r & 0xff) as u8, (r >> 8 & 0xff) as u8, (r >> 16 & 0xff) as u8]));
    im
}

fn encode_jpeg(im: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82).encode_image(im).expect("jpeg");
    out
}

fn encode_png(im: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out).write_image(im.as_raw(), im.width(), im.height(), image::ExtendedColorType::Rgba8).expect("png");
    out
}

/// A blank page for a preview: white with a light border.
fn blank_page(w: u32, h: u32) -> RgbImage {
    let mut im = RgbImage::from_pixel(w.max(1), h.max(1), Rgb([250, 250, 248]));
    let (w, h) = (im.width(), im.height());
    for x in 0..w {
        im.put_pixel(x, 0, Rgb([226, 226, 222]));
        im.put_pixel(x, h - 1, Rgb([226, 226, 222]));
    }
    for y in 0..h {
        im.put_pixel(0, y, Rgb([226, 226, 222]));
        im.put_pixel(w - 1, y, Rgb([226, 226, 222]));
    }
    im
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() == 3 && a[1] == "--list" {
        let k = KeyFile::from_zip(&std::fs::read(&a[2]).expect("read input")).expect("open input");
        let ids: Vec<u64> = k.package_metadata().map(|pm| pm.msgs("datas").filter_map(|d| d.u64("identifier")).collect()).unwrap_or_default();
        for id in ids {
            if let Some((name, bytes)) = k.data(id) {
                if image::load_from_memory(&bytes).is_err() {
                    println!("{id}\t{name}\t{}", bytes.len());
                }
            }
        }
        return;
    }
    if a.len() < 3 {
        eprintln!("usage: clean_donor <in> <out> [media-dir]");
        std::process::exit(2);
    }
    let mut k = KeyFile::from_zip(&std::fs::read(&a[1]).expect("read input")).expect("open input");
    let media = a.get(3).map(std::path::PathBuf::from);
    let ids: Vec<u64> = k.package_metadata().map(|pm| pm.msgs("datas").filter_map(|d| d.u64("identifier")).collect()).unwrap_or_default();
    let (mut images, mut files, mut kept) = (0, 0, Vec::new());
    for id in ids {
        // (artwork named only by a resource locator has no bytes in the file: nothing to replace)
        let Some((name, bytes)) = k.data(id).map(|(n, d)| (n.to_string(), d.into_owned())) else { continue };
        let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        if let Ok(im) = image::load_from_memory(&bytes) {
            let png = ext == "png" || im.color().has_alpha();
            let wash = wash(im.width(), im.height(), id);
            let out = if png {
                let mut rgba = RgbaImage::new(wash.width(), wash.height());
                for (x, y, p) in wash.enumerate_pixels() {
                    rgba.put_pixel(x, y, Rgba([p[0], p[1], p[2], 255]));
                }
                encode_png(&rgba)
            } else {
                encode_jpeg(&wash)
            };
            k.replace_data_bytes(id, out, if png { "placeholder.png" } else { "placeholder.jpg" });
            images += 1;
        } else if let Some(b) = media.as_ref().and_then(|d| std::fs::read(d.join(format!("placeholder-{id}.{ext}"))).ok()) {
            // (one original file per data id, so no two share a digest)
            k.replace_data_bytes(id, b, &format!("placeholder.{ext}"));
            files += 1;
        } else {
            kept.push(name);
        }
    }
    // previews and thumbnails outside Data/: blank pages of the same size
    let previews: Vec<String> = k
        .entries
        .iter()
        .filter(|e| matches!(e.data, EntryData::Raw(_)) && !e.name.starts_with("Data/") && !e.name.starts_with("Index/"))
        .filter(|e| [".jpg", ".jpeg", ".png", ".tiff"].iter().any(|x| e.name.to_ascii_lowercase().ends_with(x)))
        .map(|e| e.name.clone())
        .collect();
    let mut prev = 0;
    for n in previews {
        let Some(Entry { data: EntryData::Raw(raw), .. }) = k.entry(&n) else { continue };
        let Ok(im) = image::load_from_memory(raw) else { continue };
        let page = blank_page(im.width(), im.height());
        let bytes = if n.to_ascii_lowercase().ends_with(".png") {
            let mut rgba = RgbaImage::new(page.width(), page.height());
            for (x, y, p) in page.enumerate_pixels() {
                rgba.put_pixel(x, y, Rgba([p[0], p[1], p[2], 255]));
            }
            encode_png(&rgba)
        } else {
            encode_jpeg(&page)
        };
        k.remove_entry(&n);
        k.add_entry(Entry { name: n, data: EntryData::Raw(std::sync::Arc::new(bytes)) });
        prev += 1;
    }
    std::fs::write(&a[2], k.to_zip().expect("write")).expect("save");
    println!("{}: {images} images, {files} other files, {prev} previews replaced{}", a[1], if kept.is_empty() { String::new() } else { format!("; kept {}", kept.join(", ")) });
}
