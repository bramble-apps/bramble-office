//! The system clipboard beyond plain text, as Notes takes it: a picture pastes as a picture
//! ("Pasted Image"), files copied in Files / Nautilus paste as attachments, a page's rich text
//! keeps its formatting and its pictures; a picture copied here pastes as a picture in other
//! apps. (Reading and writing the formats: kn_ui::clipboard.)

use std::path::{Path, PathBuf};

use nt_core::model;

use super::doc::{self, Att, AttKind, Fmt, OBJ, Para};
use super::ops::Frag;

/// What a paste found on the clipboard.
#[derive(Default)]
pub struct Incoming {
    /// copied files (a file manager's Copy)
    pub files: Vec<PathBuf>,
    /// a picture, PNG-encoded, and its pixels' key
    pub image: Option<(Vec<u8>, u64)>,
    pub html: Option<String>,
    pub text: Option<String>,
}

/// Where pasted pictures are written until the note is saved (the library then keeps them).
fn pasted_dir() -> Option<PathBuf> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    // (a folder per picture, so each is plainly "Pasted Image" in the note)
    let dir = std::env::temp_dir().join("bramble-notes-pasted").join(format!("{}-{}-{}", std::process::id(), (crate::dates::now() * 1000.0) as u64, N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The file extension a picture's bytes call for (None: not a picture we keep).
fn picture_ext(bytes: &[u8]) -> Option<&'static str> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => Some("png"),
        image::ImageFormat::Jpeg => Some("jpeg"),
        image::ImageFormat::Gif => Some("gif"),
        image::ImageFormat::WebP => Some("webp"),
        _ => None,
    }
}

/// A picture's bytes as an image attachment named "Pasted Image" (as Notes names them).
pub fn pasted_image(bytes: &[u8]) -> Option<Att> {
    let ext = picture_ext(bytes)?;
    let path = pasted_dir()?.join(format!("Pasted Image.{ext}"));
    std::fs::write(&path, bytes).ok()?;
    Some(super::media::att_for_file(&path))
}

/// A paragraph holding one block attachment.
fn block_para(att: &Att) -> Para {
    let mut p = Para::default();
    p.insert(0, &OBJ.to_string(), &Fmt { att: Some(att.id.clone()), ..Default::default() });
    p
}

/// Attachments as a fragment: each on its own line, with a line to type in after each.
pub fn blocks_frag(atts: Vec<Att>) -> Frag {
    let mut paras = vec![Para::default()];
    for a in &atts {
        paras.push(block_para(a));
        paras.push(Para::default());
    }
    Frag { paras, atts, open_end: false }
}

/// The words of a text (letters and digits), to compare a page's HTML with its plain text.
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

const MARK_A: char = '\u{E000}';
const MARK_B: char = '\u{E001}';

/// An `<img>` tag's `src`.
fn img_src(tag: &str) -> Option<String> {
    let low = tag.to_ascii_lowercase();
    let mut from = 0;
    // (`src=`, not `data-src=` or `srcset=`)
    let at = loop {
        let i = low[from..].find("src")? + from;
        let before = low[..i].chars().next_back();
        let after = low[i + 3..].trim_start();
        if before.is_some_and(char::is_whitespace) && after.starts_with('=') {
            break i + 3 + (low[i + 3..].len() - after.len()) + 1;
        }
        from = i + 3;
    };
    let v = tag[at..].trim_start();
    let v = match v.chars().next()? {
        q @ ('"' | '\'') => v[1..].split(q).next()?,
        _ => v.split(|c: char| c.is_whitespace() || c == '>' || c == '/').next()?,
    };
    Some(v.replace("&amp;", "&"))
}

/// Rich text (HTML) as a fragment: Notes' paragraph styles, bold, italic, links (crate::import's
/// reading), with its pictures as image attachments. `fetch` gives a picture's bytes for an
/// `<img src>`. None when the HTML isn't worth more than its plain text.
pub fn html_frag(html: &str, plain: Option<&str>, fetch: &(dyn Fn(&str) -> Option<Vec<u8>> + Sync)) -> Option<Frag> {
    // each <img> becomes a marked line of its own: "\u{E000}n\u{E001}"
    let mut src = String::with_capacity(html.len());
    let mut pics: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.to_ascii_lowercase().find("<img") {
        src.push_str(&rest[..i]);
        let end = rest[i..].find('>').map_or(rest.len(), |e| i + e + 1);
        if let Some(s) = img_src(&rest[i..end]) {
            src.push_str(&format!("<br>{MARK_A}{}{MARK_B}<br>", pics.len()));
            pics.push(s);
        }
        rest = &rest[end..];
    }
    src.push_str(rest);
    let low = html.to_ascii_lowercase();
    let formatted = ["<b>", "<b ", "<strong", "<i>", "<i ", "<em", "<u>", "<s>", "<strike", "<del", "<a ", "<h1", "<h2", "<h3", "<h4", "<h5", "<h6", "<ul", "<ol", "<li", "<blockquote", "type=\"checkbox\"", "type=checkbox"].iter().any(|t| low.contains(t));
    if pics.is_empty() && !formatted {
        return None;
    }
    // code copied from an editor: its plain text keeps the indents the HTML's spans lose
    if !low.contains("<pre") && plain.is_some_and(|p| p.lines().any(|l| l.starts_with("  ") || l.starts_with('\t'))) && pics.is_empty() {
        return None;
    }
    let body = crate::import::html(&src);
    // the pictures' own lines; the empty lines the markers' <br>s made go
    let mut out: Vec<model::Paragraph> = Vec::new();
    let mut pic_at: Vec<(usize, usize)> = Vec::new();
    let is_mark = |t: &str| -> Option<usize> { t.trim().strip_prefix(MARK_A)?.strip_suffix(MARK_B)?.parse().ok() };
    for p in body.paragraphs {
        let t = p.text();
        if let Some(n) = is_mark(&t) {
            if out.last().is_some_and(|q| q.text().trim().is_empty()) {
                out.pop();
            }
            pic_at.push((out.len(), n));
            out.push(model::Paragraph::default());
            continue;
        }
        if t.trim().is_empty() && pic_at.last().is_some_and(|&(i, _)| i + 1 == out.len()) {
            continue;
        }
        // a marker inside text (a picture the <br>s didn't set apart): the text without it
        let mut p = p;
        if t.contains(MARK_A) {
            for s in &mut p.spans {
                while let (Some(a), Some(b)) = (s.text.find(MARK_A), s.text.find(MARK_B)) {
                    if b < a {
                        break;
                    }
                    s.text.replace_range(a..b + MARK_B.len_utf8(), "");
                }
            }
        }
        out.push(p);
    }
    // the words must be the plain text's (a page's HTML can hide or add text)
    if let Some(plain) = plain {
        let mine: Vec<String> = out.iter().flat_map(|p| words(&p.text())).collect();
        if mine != words(plain) {
            return None;
        }
    }
    let d = doc::from_body(&model::Body { paragraphs: out });
    let mut paras = d.paras;
    let mut atts = d.atts;
    let mut kept = 0;
    // (fetched together: a page's pictures load at once)
    let urls: Vec<&String> = pic_at.iter().map(|&(_, n)| &pics[n]).collect();
    let got: Vec<Option<Vec<u8>>> = std::thread::scope(|s| {
        let hs: Vec<_> = urls.iter().take(40).map(|u| s.spawn(|| fetch(u))).collect();
        hs.into_iter().map(|h| h.join().ok().flatten()).collect()
    });
    for (k, &(i, _)) in pic_at.iter().enumerate().rev() {
        match got.get(k).cloned().flatten().and_then(|b| pasted_image(&b)) {
            Some(a) => {
                paras[i] = block_para(&a);
                atts.push(a);
                kept += 1;
            }
            None => {
                paras.remove(i);
            }
        }
    }
    if paras.is_empty() || (kept == 0 && !formatted) {
        return None;
    }
    // blocks never start or end the fragment, and a line to type in follows each
    let is_block = |p: &Para, atts: &[Att]| p.sole_att().is_some_and(|id| atts.iter().any(|a| a.id == id && a.is_block()));
    let mut k = 0;
    while k < paras.len() {
        if is_block(&paras[k], &atts) && paras.get(k + 1).is_none_or(|q| is_block(q, &atts)) {
            paras.insert(k + 1, Para::default());
        }
        k += 1;
    }
    if is_block(&paras[0], &atts) {
        paras.insert(0, Para::default());
    }
    Some(Frag { paras, atts, open_end: false })
}

/// The picture to put on the clipboard for an attachment: its RGBA pixels.
pub fn picture_pixels(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?.to_rgba8();
    Some((img.width(), img.height(), img.into_raw()))
}

/// A fragment that is one picture (or a drawing's picture): the file to copy out as a picture.
pub fn sole_picture(frag: &Frag) -> Option<PathBuf> {
    if frag.paras.len() != 1 {
        return None;
    }
    let id = frag.paras[0].sole_att()?;
    match &frag.atts.iter().find(|a| a.id == id)?.kind {
        AttKind::Image { path: Some(p), .. } | AttKind::Drawing { path: Some(p), .. } => Some(p.clone()),
        _ => None,
    }
}

/// A fragment that is one attachment with no text of its own: its name, as its plain text.
pub fn sole_name(frag: &Frag) -> Option<String> {
    if frag.paras.len() != 1 {
        return None;
    }
    let id = frag.paras[0].sole_att()?;
    match &frag.atts.iter().find(|a| a.id == id)?.kind {
        AttKind::Image { name, .. } | AttKind::File { name, .. } | AttKind::Pdf { name, .. } => Some(name.clone()).filter(|n| !n.is_empty()),
        _ => None,
    }
}
