//! Word mode: a .docx opened in Letters stays one. Plain Word mode saves only what Word holds;
//! Word + Letters (compatibility mode, as Values' Excel + Values) also keeps the whole Letters
//! document inside the .docx (letters/document.pages) with a fingerprint of the Word part it
//! was saved with. Opening such a file uses the Letters copy, unless another app (Word,
//! OnlyOffice, LibreOffice) changed the Word part since: then the Word content is read and the
//! Letters-only extras are gone.

use crate::Pages;

// (the app was called Cardstock when these parts were named: the names stay, so files made
// then still open)
pub const PAGES_PART: &str = "cardstock/document.pages";
pub const PRINT_PART: &str = "cardstock/fingerprint.txt";
const REL_TYPE: &str = "https://letters.app/relationships/pages-document";

/// FNV-1a: a fingerprint that stays the same across builds (std's hasher doesn't promise that).
fn fingerprint(b: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for x in b {
        h ^= *x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// The document as a single-file .pages (cleaned as a save is).
pub fn pages_bytes(p: &mut Pages) -> Result<Vec<u8>, String> {
    p.doc.declare_external_refs();
    let mut k = p.doc.file.clone();
    crate::gc::sweep(&mut k);
    k.to_zip().map_err(|e| e.to_string())
}

/// Save as Word: plain (`hybrid` false) or with the Letters copy inside.
pub fn save_docx(p: &mut Pages, path: &std::path::Path, hybrid: bool) -> Result<(), String> {
    let mut files = crate::export::docx_files(p)?;
    if hybrid {
        let doc = files.iter().find(|f| f.0 == "word/document.xml").map(|f| fingerprint(&f.1)).unwrap_or_default();
        let bytes = pages_bytes(p)?;
        // registered with the package (a type for each part, a relationship from the root), so
        // the file stays valid Office Open XML; Word and OnlyOffice ignore what they don't know
        for f in files.iter_mut() {
            if f.0 == "[Content_Types].xml" {
                let s = String::from_utf8_lossy(&f.1).replace("<Default Extension=\"rels\"", "<Default Extension=\"pages\" ContentType=\"application/vnd.apple.pages\"/><Default Extension=\"txt\" ContentType=\"text/plain\"/><Default Extension=\"rels\"");
                f.1 = s.into_bytes();
            }
            if f.0 == "_rels/.rels" {
                let s = String::from_utf8_lossy(&f.1).replace("</Relationships>", &format!("<Relationship Id=\"rIdLetters\" Type=\"{REL_TYPE}\" Target=\"{PAGES_PART}\"/></Relationships>"));
                f.1 = s.into_bytes();
            }
        }
        files.push((PAGES_PART.into(), bytes, true));
        files.push((PRINT_PART.into(), doc.into_bytes(), false));
    }
    // written beside, then moved over (a failed save leaves the old file)
    let tmp = path.with_extension("docx.tmp");
    crate::export::zip_files(&tmp, files)?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    p.doc.dirty = false;
    Ok(())
}

pub struct Opened {
    pub pages: Pages,
    /// The file has a Letters copy inside (Word + Letters).
    pub hybrid: bool,
    /// It did, but another app changed the Word part since: the Word content was read.
    pub changed_elsewhere: bool,
}

/// Open a .docx: its Letters copy when it has a current one, else what Word holds.
pub fn open_docx(path: &std::path::Path) -> Result<Opened, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut read = |name: &str| -> Option<Vec<u8>> {
        let mut e = z.by_name(name).ok()?;
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut e, &mut b).ok()?;
        Some(b)
    };
    let copy = read(PAGES_PART);
    let saved_print = read(PRINT_PART).map(|b| String::from_utf8_lossy(&b).trim().to_string());
    let now_print = read("word/document.xml").map(|b| fingerprint(&b));
    if let Some(bytes) = &copy {
        if saved_print.is_some() && saved_print == now_print {
            if let Ok(mut pages) = Pages::from_bytes(bytes) {
                pages.doc.path = None;
                pages.doc.dirty = false;
                // still a Word document: laid out as Word lays it out
                pages.word_layout = true;
                return Ok(Opened { pages, hybrid: true, changed_elsewhere: false });
            }
        }
    }
    let w = crate::docx::read(path)?;
    let mut pages = crate::docx::build(&w);
    pages.doc.path = None;
    pages.doc.dirty = false;
    Ok(Opened { pages, hybrid: copy.is_some(), changed_elsewhere: copy.is_some() })
}
