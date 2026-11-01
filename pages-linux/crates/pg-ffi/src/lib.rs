//! Letters' engine behind a C interface, for front ends other than the egui app (the Flutter app
//! for Android and, later, iOS). One document open at a time. Strings are UTF-8, structured
//! answers JSON; strings returned to the caller are freed with `lt_free_str`. Positions in the
//! body text are byte offsets into its stored text; page geometry is in page points.

use std::ffi::{CStr, CString, c_char};
use std::path::Path;
use std::sync::Mutex;

use pg_core::Pages;
use pg_core::flow::{self, Layout};
use pg_core::kn_format::{Msg, ObjId};
use pg_core::kn_model::{self, edit::History};
use pg_core::kn_render::Renderer;
use serde_json::{Value, json};

struct Session {
    p: Pages,
    hist: History,
    r: Renderer,
    cache: flow::Cache,
    layout: Option<Layout>,
    anchor: usize,
    caret: usize,
    /// Character changes for what's typed next (Bold with nothing selected).
    typing: Vec<(String, f64)>,
    dirty: bool,
}

static S: Mutex<Option<Session>> = Mutex::new(None);
static ERR: Mutex<String> = Mutex::new(String::new());

fn with<T: Default>(f: impl FnOnce(&mut Session) -> T) -> T {
    let mut g = S.lock().unwrap_or_else(|e| e.into_inner());
    match g.as_mut() {
        Some(s) => f(s),
        None => T::default(),
    }
}

fn fail(e: impl ToString) -> i32 {
    *ERR.lock().unwrap_or_else(|e| e.into_inner()) = e.to_string();
    -1
}

fn out(s: String) -> *mut c_char {
    CString::new(s.replace('\0', "")).map(CString::into_raw).unwrap_or(std::ptr::null_mut())
}

fn arg(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

impl Session {
    fn new(p: Pages) -> Session {
        let caret = 0;
        Session { p, hist: History::default(), r: Renderer::new(), cache: flow::Cache::default(), layout: None, anchor: caret, caret, typing: Vec::new(), dirty: false }
    }

    fn body(&self) -> Option<ObjId> {
        self.p.body_storage()
    }

    fn text(&self) -> String {
        self.body().map(|b| self.p.doc.storage_text(b)).unwrap_or_default()
    }

    fn layout(&mut self) -> &Layout {
        if self.layout.is_none() {
            let opts = flow::Options { show_deletions: true, ..flow::Options::default() };
            self.layout = Some(flow::layout(&self.p, &mut self.r.fonts, &mut self.cache, &opts));
        }
        self.layout.as_ref().unwrap()
    }

    fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.caret), self.anchor.max(self.caret))
    }

    /// An edit of the document as one undo step; the layout is made again after.
    fn edit(&mut self, label: &str, f: impl FnOnce(&mut kn_model::Document, &mut History)) {
        self.p.doc.begin(&mut self.hist, label);
        f(&mut self.p.doc, &mut self.hist);
        self.p.doc.end(&mut self.hist);
        self.changed();
    }

    fn changed(&mut self) {
        self.layout = None;
        self.dirty = true;
        let n = self.text().len();
        self.anchor = self.anchor.min(n);
        self.caret = self.caret.min(n);
    }

    /// Replaces the selection with `s` (typing, paste); the caret goes after it.
    fn insert(&mut self, s: &str) {
        let Some(st) = self.body() else { return };
        let (a, b) = self.range();
        let typing = std::mem::take(&mut self.typing);
        self.edit("Typing", |d, h| {
            pg_core::edit::replace(d, h, st, a, b, s);
            if !typing.is_empty() && !s.is_empty() {
                pg_core::edit::set_char_props(d, h, st, a, a + s.len(), &|cp| {
                    for (k, v) in &typing {
                        char_prop(cp, k, *v);
                    }
                });
            }
        });
        self.anchor = a + s.len();
        self.caret = self.anchor;
    }

    fn caret_rect(&mut self) -> Option<(usize, f32, f32, f32)> {
        let caret = self.caret;
        let l = self.layout();
        let body = l.body.as_ref()?;
        l.caret(body.to_shown(caret))
    }

    /// The selection's highlight: (page, x0, y0, x1, y1) per line, in page points.
    fn rects(&mut self) -> Vec<(usize, f32, f32, f32, f32)> {
        let (a, b) = self.range();
        let l = self.layout();
        let Some(body) = l.body.as_ref() else { return Vec::new() };
        let (a, b) = (body.to_shown(a), body.to_shown(b));
        let mut out = Vec::new();
        if a >= b {
            return out;
        }
        for (pi, page) in l.pages.iter().enumerate() {
            if page.end < a || page.start > b {
                continue;
            }
            for line in &page.lines {
                let (ls, le) = line.range();
                if le < a || ls > b || (ls == le && !(a <= ls && ls < b)) {
                    continue;
                }
                let tl = &line.piece.tl;
                let (mut x0, mut x1) = (f32::INFINITY, f32::NEG_INFINITY);
                for &gi in &line.piece.line_glyphs[line.line] {
                    let g = &tl.glyphs[gi];
                    if g.label || g.end <= a || g.start >= b {
                        continue;
                    }
                    x0 = x0.min(g.x);
                    x1 = x1.max(g.x + g.w);
                }
                let (top, bottom) = line.band();
                if b > le && a <= le {
                    let lx = tl.lines[line.line].x + tl.lines[line.line].width;
                    if x0.is_infinite() {
                        x0 = lx;
                    }
                    x1 = x1.max(lx + 4.0);
                }
                if x0.is_finite() && x1 > x0 {
                    out.push((pi, x0 + line.dx, top, x1 + line.dx, bottom));
                }
            }
        }
        out
    }

    /// The paragraph's own and its style's properties, nearest first.
    fn para_chain(&self) -> Vec<Msg> {
        let (Some(st), (a, _)) = (self.body(), self.range()) else { return Vec::new() };
        let ps = pg_core::edit::para_style_at(&self.p.doc, st, a);
        kn_model::style::chain(&self.p.doc.file, ps).into_iter().filter_map(|m| m.msg("para_properties").cloned()).collect()
    }

    fn state(&mut self) -> Value {
        let caret = self.caret_rect().map(|(p, x, t, h)| json!([p, x, t, h]));
        let rects: Vec<Value> = self.rects().into_iter().map(|(p, a, b, c, d)| json!([p, a, b, c, d])).collect();
        let (a, b) = self.range();
        let st = self.body();
        let (mut cs, style) = match st {
            Some(st) => {
                let ps = pg_core::edit::para_style_at(&self.p.doc, st, a);
                let chs = pg_core::edit::char_style_at(&self.p.doc, st, if a < b { a + 1 } else { a });
                (kn_model::style::char_style(&self.p.doc.file, ps, chs), pg_core::edit::named_style(&self.p.doc, ps).1)
            }
            None => (Default::default(), String::new()),
        };
        for (k, v) in &self.typing {
            match k.as_str() {
                "bold" => cs.bold = *v != 0.0,
                "italic" => cs.italic = *v != 0.0,
                "underline" => cs.underline = *v != 0.0,
                "strike" => cs.strike = *v != 0.0,
                "size" => cs.size = *v as f32,
                _ => {}
            }
        }
        let align = match self.para_chain().iter().find_map(|p| p.enum_name("alignment").map(str::to_string)).as_deref() {
            Some("TATvalue2") => 1,
            Some("TATvalue1") => 2,
            Some("TATvalue3") => 3,
            _ => 0,
        };
        let list = st.map(|st| pg_core::edit::list_at(&self.p.doc, st, a).0).flatten();
        let list_name = list.and_then(|id| pg_core::edit::list_styles(&self.p.doc).into_iter().find(|(i, _)| *i == id).map(|(_, n)| n)).unwrap_or_default();
        let text = self.text();
        let selected = text.get(a..b).unwrap_or("").to_string();
        json!({
            "anchor": self.anchor, "caret": self.caret, "caretRect": caret, "rects": rects,
            "selected": selected,
            "style": style, "font": cs.font_name, "size": cs.size, "bold": cs.bold, "italic": cs.italic,
            "underline": cs.underline, "strike": cs.strike, "align": align, "list": list_name,
            "canUndo": self.hist.can_undo(), "canRedo": self.hist.can_redo(), "dirty": self.dirty,
        })
    }
}

/// A character property by name (bold, italic, underline, strike: 0 or 1; size: points).
fn char_prop(cp: &mut Msg, k: &str, v: f64) {
    match k {
        "bold" => cp.set_bool("bold", v != 0.0),
        "italic" => cp.set_bool("italic", v != 0.0),
        "underline" => cp.set_enum("underline", if v != 0.0 { "kSingleUnderline" } else { "kNoUnderline" }),
        "strike" => cp.set_enum("strikethru", if v != 0.0 { "kSingleStrikethru" } else { "kNoStrikethru" }),
        "size" => cp.set_f32("font_size", (v as f32).clamp(1.0, 1638.0)),
        _ => {}
    }
}

/// The word around `at` in `text` (a double tap).
fn word_at(text: &str, at: usize) -> (usize, usize) {
    let is_word = |c: char| c.is_alphanumeric() || c == '\'' || c == '’' || c == '_';
    let mut a = at.min(text.len());
    while !text.is_char_boundary(a) {
        a -= 1;
    }
    let mut b = a;
    while a > 0 {
        let c = text[..a].chars().next_back().unwrap();
        if !is_word(c) {
            break;
        }
        a -= c.len_utf8();
    }
    while b < text.len() {
        let c = text[b..].chars().next().unwrap();
        if !is_word(c) {
            break;
        }
        b += c.len_utf8();
    }
    (a, b)
}

// ---- the C interface ----

#[unsafe(no_mangle)]
pub extern "C" fn lt_init() {
    pg_core::init();
}

/// A new blank document.
#[unsafe(no_mangle)]
pub extern "C" fn lt_new() -> i32 {
    let s = Session::new(Pages::new_blank(false));
    *S.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
    0
}

/// Opens a .pages document (or imports .docx, .rtf, .md, .txt). 0, or -1 (see lt_error).
#[unsafe(no_mangle)]
pub extern "C" fn lt_open(path: *const c_char) -> i32 {
    let path = arg(path);
    let p = Path::new(&path);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let doc = if ext == "pages" || p.is_dir() { Pages::open(p) } else { pg_core::import::import_file(p) };
    match doc {
        Ok(d) => {
            *S.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session::new(d));
            0
        }
        Err(e) => fail(e),
    }
}

/// Saves as .pages, or exports by the extension (.docx, .pdf, .rtf, .txt, .epub, .html).
#[unsafe(no_mangle)]
pub extern "C" fn lt_save(path: *const c_char) -> i32 {
    let path = arg(path);
    let p = Path::new(&path);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let mut g = S.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = g.as_mut() else { return fail("no document") };
    let r = if ext == "pages" { s.p.save_as(p).map_err(|e| e.to_string()) } else { pg_core::export::export(&s.p, p) };
    match r {
        Ok(()) => {
            if ext == "pages" {
                s.dirty = false;
            }
            0
        }
        Err(e) => {
            drop(g);
            fail(e)
        }
    }
}

/// The last error's message.
#[unsafe(no_mangle)]
pub extern "C" fn lt_error() -> *mut c_char {
    out(ERR.lock().unwrap_or_else(|e| e.into_inner()).clone())
}

#[unsafe(no_mangle)]
pub extern "C" fn lt_free_str(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// The pages: {"pages": [[width, height], …], "styles": [paragraph style names], "lists": […]}.
#[unsafe(no_mangle)]
pub extern "C" fn lt_info() -> *mut c_char {
    let v = with(|s| {
        let l = s.layout();
        let pages: Vec<Value> = (0..l.page_count()).map(|i| {
            let ps = l.page_setup(i);
            json!([ps.width, ps.height])
        }).collect();
        let order = ["Title", "Subtitle", "Heading", "Heading 2", "Heading 3", "Heading Red", "Body", "Caption", "Header & Footer", "Footnote", "Label", "Label Dark", "Quote", "Attribution"];
        let mut styles: Vec<String> = s.p.doc.paragraph_styles().into_iter().map(|(_, n)| n).collect();
        styles.sort_by_key(|n| order.iter().position(|o| o == n).unwrap_or(100));
        let lists: Vec<String> = pg_core::edit::list_styles(&s.p.doc).into_iter().map(|(_, n)| n).collect();
        Some(json!({ "pages": pages, "styles": styles, "lists": lists }))
    });
    out(v.unwrap_or(Value::Null).to_string())
}

/// Draws page `page` at `scale` pixels per point into `buf` (RGBA, premultiplied, w × h).
#[unsafe(no_mangle)]
pub extern "C" fn lt_render(page: u32, scale: f32, buf: *mut u8, w: u32, h: u32) -> i32 {
    with(|s| {
        s.layout();
        let l = s.layout.as_ref().unwrap();
        let Some(pm) = pg_core::render::render_page(&s.p, l, page as usize, &mut s.r, scale, Default::default()) else { return -1 };
        let out = unsafe { std::slice::from_raw_parts_mut(buf, (w * h * 4) as usize) };
        let (pw, ph) = (pm.width().min(w) as usize, pm.height().min(h) as usize);
        let src = pm.data();
        for y in 0..ph {
            let s0 = y * pm.width() as usize * 4;
            let d0 = y * w as usize * 4;
            out[d0..d0 + pw * 4].copy_from_slice(&src[s0..s0 + pw * 4]);
        }
        0
    })
}

/// The body text's position at (x, y) on page `page`, or -1.
#[unsafe(no_mangle)]
pub extern "C" fn lt_hit(page: u32, x: f32, y: f32) -> i64 {
    with(|s| {
        let l = s.layout();
        let Some(body) = l.body.as_ref() else { return -1 };
        l.hit(page as usize, x, y).map(|sh| body.to_stored(sh) as i64).unwrap_or(-1)
    })
}

/// Selects from `anchor` to `caret` (equal: the caret alone).
#[unsafe(no_mangle)]
pub extern "C" fn lt_select(anchor: i64, caret: i64) {
    with(|s| {
        let n = s.text().len() as i64;
        s.anchor = anchor.clamp(0, n) as usize;
        s.caret = caret.clamp(0, n) as usize;
        s.typing.clear();
    })
}

/// Selects the word at `at` (a double tap).
#[unsafe(no_mangle)]
pub extern "C" fn lt_select_word(at: i64) {
    with(|s| {
        let (a, b) = word_at(&s.text(), at.max(0) as usize);
        s.anchor = a;
        s.caret = b;
        s.typing.clear();
    })
}

/// Selects all of the body text.
#[unsafe(no_mangle)]
pub extern "C" fn lt_select_all() {
    with(|s| {
        s.anchor = 0;
        s.caret = s.text().len();
    })
}

/// The caret, selection and the look of the text there (JSON; see Session::state).
#[unsafe(no_mangle)]
pub extern "C" fn lt_state() -> *mut c_char {
    out(with(|s| Some(s.state())).unwrap_or(Value::Null).to_string())
}

/// Replaces the selection with `text` (typing and pasting).
#[unsafe(no_mangle)]
pub extern "C" fn lt_insert(text: *const c_char) {
    let t = arg(text).replace("\r\n", "\n").replace('\r', "\n");
    with(|s| s.insert(&t))
}

/// Deletes the selection, or the character before the caret.
#[unsafe(no_mangle)]
pub extern "C" fn lt_backspace() {
    with(|s| {
        if s.anchor == s.caret {
            let text = s.text();
            let c = s.caret.min(text.len());
            let Some(ch) = text[..c].chars().next_back() else { return };
            s.anchor = c - ch.len_utf8();
        }
        s.insert("");
    })
}

/// Bold, italic, underline or strike on (1) or off (0), or the size in points: the selection,
/// or what's typed next.
#[unsafe(no_mangle)]
pub extern "C" fn lt_char(prop: *const c_char, value: f64) {
    let k = arg(prop);
    with(|s| {
        let (a, b) = s.range();
        let Some(st) = s.body() else { return };
        if a == b {
            s.typing.retain(|(p, _)| *p != k);
            s.typing.push((k, value));
            return;
        }
        let k2 = k.clone();
        s.edit("Format", |d, h| {
            pg_core::edit::set_char_props(d, h, st, a, b, &|cp| char_prop(cp, &k2, value));
        });
    })
}

/// The paragraphs' alignment: 0 left, 1 centre, 2 right, 3 justified.
#[unsafe(no_mangle)]
pub extern "C" fn lt_align(align: u32) {
    let v = ["TATvalue0", "TATvalue2", "TATvalue1", "TATvalue3"][align.min(3) as usize];
    with(|s| {
        let (a, b) = s.range();
        let Some(st) = s.body() else { return };
        s.edit("Alignment", |d, h| {
            pg_core::edit::set_para_props(d, h, st, a, b, &|pp| pp.set_enum("alignment", v));
        });
    })
}

/// The paragraphs' style, by name.
#[unsafe(no_mangle)]
pub extern "C" fn lt_para_style(name: *const c_char) {
    let name = arg(name);
    with(|s| {
        let (a, b) = s.range();
        let (Some(st), Some(style)) = (s.body(), pg_core::edit::para_style_named(&s.p.doc, &name)) else { return };
        s.edit("Paragraph Style", |d, h| {
            pg_core::edit::set_para_style(d, h, st, a, b, style);
        });
    })
}

/// The paragraphs' list style by name ("" for none).
#[unsafe(no_mangle)]
pub extern "C" fn lt_list(name: *const c_char) {
    let name = arg(name);
    with(|s| {
        let (a, b) = s.range();
        let Some(st) = s.body() else { return };
        let list = pg_core::edit::list_styles(&s.p.doc).into_iter().find(|(_, n)| *n == name).map(|(i, _)| i);
        s.edit("List", |d, h| {
            pg_core::edit::set_list_style(d, h, st, a, b, list);
        });
    })
}

/// 1 when something was undone.
#[unsafe(no_mangle)]
pub extern "C" fn lt_undo() -> i32 {
    with(|s| {
        let done = s.p.doc.undo(&mut s.hist);
        if done {
            s.changed();
        }
        done as i32
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn lt_redo() -> i32 {
    with(|s| {
        let done = s.p.doc.redo(&mut s.hist);
        if done {
            s.changed();
        }
        done as i32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Value {
        let p = lt_state();
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        lt_free_str(p);
        serde_json::from_str(&s).unwrap()
    }

    #[test]
    fn type_select_format_undo_and_draw() {
        lt_init();
        assert_eq!(lt_new(), 0);
        let t = CString::new("Hello brave world").unwrap();
        lt_insert(t.as_ptr());
        assert_eq!(state()["caret"], 17);
        // a double tap on "brave"
        lt_select_word(8);
        let s = state();
        assert_eq!((s["anchor"].as_u64(), s["caret"].as_u64(), s["selected"].as_str()), (Some(6), Some(11), Some("brave")));
        assert!(!s["rects"].as_array().unwrap().is_empty());
        let bold = CString::new("bold").unwrap();
        lt_char(bold.as_ptr(), 1.0);
        assert_eq!(state()["bold"], true);
        assert_eq!(lt_undo(), 1);
        assert_eq!(state()["bold"], false);
        // backspace at the end
        lt_select(17, 17);
        lt_backspace();
        assert_eq!(state()["caret"], 16);
        // a page drawn
        let info: Value = {
            let p = lt_info();
            let v = serde_json::from_str(&unsafe { CStr::from_ptr(p) }.to_string_lossy()).unwrap();
            lt_free_str(p);
            v
        };
        let [w, h] = [info["pages"][0][0].as_f64().unwrap(), info["pages"][0][1].as_f64().unwrap()];
        let (pw, ph) = ((w * 0.5).ceil() as u32, (h * 0.5).ceil() as u32);
        let mut buf = vec![0u8; (pw * ph * 4) as usize];
        assert_eq!(lt_render(0, 0.5, buf.as_mut_ptr(), pw, ph), 0);
        assert!(buf.chunks(4).any(|p| p[0] < 128 && p[3] == 255), "some dark text pixels");
        // a tap on the text finds a position in it
        let c = state()["caretRect"].clone();
        let hit = lt_hit(0, c[1].as_f64().unwrap() as f32 - 2.0, c[2].as_f64().unwrap() as f32 + 4.0);
        assert!((14..=16).contains(&hit), "{hit}");
    }
}
