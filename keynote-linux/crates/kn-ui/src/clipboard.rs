//! The system clipboard beyond plain text: pictures, copied files and rich text (HTML), for every
//! suite app.
//!
//! egui only hands an app the clipboard's text (`Event::Paste`) or, when there is no text, its
//! picture decoded to pixels (`Event::PasteImage`). A copied file (Files / Nautilus), a page's
//! rich text with its pictures, or a picture that comes with text need the other formats, which
//! these read on demand (call them when a paste event arrives, or on Edit > Paste).
//!
//! Linux: Wayland compositors with the data-control protocol (KDE, wlroots) are read directly;
//! GNOME has no such protocol, so arboard goes through XWayland's clipboard, which Mutter keeps
//! in step with the Wayland one (every format, not only text). X11, Windows and macOS: arboard.
//!
//! Writing a picture (`set_image`) replaces the clipboard's text: copy text with egui's
//! `ctx.copy_text` instead, never both in one frame (egui writes its text after the frame and
//! would replace the picture).

use std::path::{Path, PathBuf};

/// A picture from the clipboard, PNG-encoded.
pub struct Image {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// `pixels_key` of its pixels: tells a picture this app copied when it comes back
    pub key: u64,
}

/// A fingerprint of a picture's straight-alpha RGBA pixels (to recognise a picture the app put
/// on the clipboard itself when it is pasted back).
pub fn pixels_key(width: u32, height: u32, rgba: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (width, height).hash(&mut h);
    rgba.hash(&mut h);
    h.finish()
}

/// RGBA pixels as a PNG file's bytes.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// egui's pasted picture (`Event::PasteImage`, premultiplied) as a PNG, when `image()` can't
/// read the clipboard itself.
pub fn color_image_png(img: &egui::ColorImage) -> Option<Image> {
    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let (w, h) = (img.size[0] as u32, img.size[1] as u32);
    Some(Image { png: encode_png(w, h, &rgba)?, width: w, height: h, key: pixels_key(w, h, &rgba) })
}

/// The files a `text/uri-list` names (local `file:` URIs only, percent-decoded; comments and
/// other schemes skipped).
pub fn paths_from_uri_list(list: &str) -> Vec<PathBuf> {
    list.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.strip_prefix("file://"))
        // file:///a, file://localhost/a
        .filter_map(|l| l.strip_prefix("localhost").or(Some(l)).filter(|l| l.starts_with('/')))
        .filter_map(percent_decode)
        .map(PathBuf::from)
        .collect()
}

/// GNOME's `x-special/gnome-copied-files`: "copy" or "cut", then one URI per line.
pub fn paths_from_gnome_copied_files(s: &str) -> Vec<PathBuf> {
    let mut lines = s.lines();
    match lines.next().map(str::trim) {
        Some("copy" | "cut") => paths_from_uri_list(&lines.collect::<Vec<_>>().join("\n")),
        _ => paths_from_uri_list(s),
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A local path as a `file://` URI (for `text/uri-list` and HTML).
pub fn file_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for &c in path.to_string_lossy().as_bytes() {
        if c.is_ascii_alphanumeric() || b"/-_.~".contains(&c) {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

/// A `data:` URI's bytes and media type (`data:image/png;base64,…`, or percent-encoded).
pub fn data_uri(uri: &str) -> Option<(String, Vec<u8>)> {
    let rest = uri.trim().strip_prefix("data:")?;
    let (head, data) = rest.split_once(',')?;
    let mime = head.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let bytes = if head.split(';').any(|p| p.trim().eq_ignore_ascii_case("base64")) { base64(data)? } else { percent_decode(data)?.into_bytes() };
    Some((mime, bytes))
}

/// Standard / URL-safe base64, whitespace ignored.
pub fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A picture's bytes for an HTML `<img src>` pasted from a page: inline data, a local file, or
/// the web (at most a few seconds: the paste waits for it).
pub fn fetch_src(src: &str) -> Option<Vec<u8>> {
    let src = src.trim();
    if src.starts_with("data:") {
        return data_uri(src).map(|(_, b)| b);
    }
    if src.starts_with("file://") {
        return std::fs::read(paths_from_uri_list(src).into_iter().next()?).ok();
    }
    if src.starts_with('/') && !src.starts_with("//") {
        return std::fs::read(src).ok();
    }
    let url = if src.starts_with("//") { format!("https:{src}") } else { src.to_string() };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(4))).build().into();
    let mut resp = agent.get(&url).call().ok()?;
    resp.body_mut().with_config().limit(40 << 20).read_to_vec().ok()
}

#[cfg(not(target_os = "android"))]
mod system {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    // (one for the process: on X11 the clipboard's contents are served from it, so what the app
    // copies stays pasteable while it runs)
    fn with<R>(f: impl FnOnce(&mut arboard::Clipboard) -> Option<R>) -> Option<R> {
        static CB: OnceLock<Mutex<Option<arboard::Clipboard>>> = OnceLock::new();
        let m = CB.get_or_init(|| Mutex::new(None));
        let mut g = m.lock().ok()?;
        if g.is_none() {
            *g = arboard::Clipboard::new().ok();
        }
        f(g.as_mut()?)
    }

    /// The clipboard's picture (a screenshot, an image copied in a browser or an editor).
    pub fn image() -> Option<Image> {
        let img = with(|c| c.get_image().ok())?;
        let (w, h) = (img.width as u32, img.height as u32);
        if w == 0 || h == 0 {
            return None;
        }
        Some(Image { png: encode_png(w, h, &img.bytes)?, width: w, height: h, key: pixels_key(w, h, &img.bytes) })
    }

    /// The files copied in a file manager (that still exist).
    pub fn files() -> Vec<PathBuf> {
        with(|c| c.get().file_list().ok()).unwrap_or_default().into_iter().filter(|p| p.exists()).collect()
    }

    /// The clipboard's rich text as HTML (a page or a document copied with its formatting).
    pub fn html() -> Option<String> {
        with(|c| c.get().html().ok()).filter(|h| !h.trim().is_empty())
    }

    /// The clipboard's plain text.
    pub fn text() -> Option<String> {
        with(|c| c.get_text().ok())
    }

    /// Puts a picture on the clipboard (as image/png); returns its `pixels_key`.
    pub fn set_image(width: u32, height: u32, rgba: &[u8]) -> Option<u64> {
        let data = arboard::ImageData { width: width as usize, height: height as usize, bytes: std::borrow::Cow::Borrowed(rgba) };
        with(|c| c.set_image(data).ok())?;
        Some(pixels_key(width, height, rgba))
    }

    /// Puts files on the clipboard (text/uri-list): pasted in a file manager they are copied.
    pub fn set_files(paths: &[PathBuf]) -> bool {
        with(|c| c.set().file_list(paths).ok()).is_some()
    }

    /// Puts rich text on the clipboard: HTML, with its plain text for apps that take only text.
    pub fn set_html(html: &str, plain: &str) -> bool {
        with(|c| c.set_html(html, Some(plain)).ok()).is_some()
    }
}

#[cfg(target_os = "android")]
mod system {
    use super::*;
    pub fn image() -> Option<Image> {
        None
    }
    pub fn files() -> Vec<PathBuf> {
        Vec::new()
    }
    pub fn html() -> Option<String> {
        None
    }
    pub fn text() -> Option<String> {
        None
    }
    pub fn set_image(_: u32, _: u32, _: &[u8]) -> Option<u64> {
        None
    }
    pub fn set_files(_: &[PathBuf]) -> bool {
        false
    }
    pub fn set_html(_: &str, _: &str) -> bool {
        false
    }
}

pub use system::{files, image, set_files, set_image, text};

static HOSTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The app runs under a front end that owns the clipboard (ffi.rs: it takes the copied text
/// from egui and hands pastes in): rich text then goes through it as plain text, not past it.
pub fn set_hosted(on: bool) {
    HOSTED.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The clipboard's rich text as HTML (a page or a document copied with its formatting).
pub fn html() -> Option<String> {
    if HOSTED.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    system::html()
}

/// Puts rich text on the clipboard: HTML, with its plain text for apps that take only text.
/// False when it couldn't (the caller then copies the text as usual).
pub fn set_html(html: &str, plain: &str) -> bool {
    !HOSTED.load(std::sync::atomic::Ordering::Relaxed) && system::set_html(html, plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_lists() {
        let l = "# from Files\r\nfile:///home/me/My%20Pictures/a%C3%A9.png\r\nfile://localhost/tmp/b.pdf\nhttps://x.org/c.png\n\n";
        assert_eq!(paths_from_uri_list(l), vec![PathBuf::from("/home/me/My Pictures/aé.png"), PathBuf::from("/tmp/b.pdf")]);
        assert_eq!(paths_from_gnome_copied_files("copy\nfile:///a/b%20c.txt"), vec![PathBuf::from("/a/b c.txt")]);
        assert_eq!(paths_from_gnome_copied_files("cut\nfile:///x\nfile:///y"), vec![PathBuf::from("/x"), PathBuf::from("/y")]);
    }

    #[test]
    fn file_uris_round_trip() {
        let p = Path::new("/home/me/Pasted Image #2 (é).png");
        let u = file_uri(p);
        assert!(u.starts_with("file:///home/me/Pasted%20Image%20%232"));
        assert_eq!(paths_from_uri_list(&u), vec![p.to_path_buf()]);
    }

    #[test]
    fn data_uris() {
        assert_eq!(base64("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert_eq!(base64("aGVs\nbG8").unwrap(), b"hello");
        let (m, b) = data_uri("data:image/png;base64,iVBORw0KGgo=").unwrap();
        assert_eq!(m, "image/png");
        assert_eq!(b, [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        let (m, b) = data_uri("data:image/svg+xml,%3Csvg%3E").unwrap();
        assert_eq!((m.as_str(), b.as_slice()), ("image/svg+xml", b"<svg>".as_slice()));
        assert!(data_uri("https://x.org/a.png").is_none());
    }

    #[test]
    fn pictures_encode_and_match() {
        let rgba = [255u8, 0, 0, 255, 0, 0, 255, 128];
        let png = encode_png(2, 1, &rgba).unwrap();
        let back = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(back.as_raw().as_slice(), rgba.as_slice());
        assert_eq!(pixels_key(2, 1, &rgba), pixels_key(2, 1, back.as_raw()));
        assert_ne!(pixels_key(2, 1, &rgba), pixels_key(1, 2, &rgba));
    }
}
