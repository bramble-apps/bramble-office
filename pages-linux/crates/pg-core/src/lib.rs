//! Apple Pages (`.pages`, Pages 15.4) documents on top of the suite's iWork crates.
//!
//! The container, IWA framing and the shared TSWP/TSD/TST/TSCH packages are the same as in
//! Keynote and Numbers, so `kn-format` reads and writes the file once [`init`] has swapped in
//! the Pages schema and registry (TP.* in place of KN.*; id 10011 is TP.SectionArchive here).
//! What is Pages-specific lives in this crate: the one flowing body text of a word-processing
//! document, sections with their header/footer masters, floating objects per page, and the
//! layout of all of it into pages ([`flow`]).

use kn_format::{KeyFile, Msg, ObjId};
use kn_model::Document;
use kn_model::edit::History;

pub mod body;
pub mod docx;
pub mod docx_out;
pub mod edit;
pub mod export;
pub mod fields;
pub mod gc;
pub mod guides;
pub mod flow;
pub mod import;
pub mod notes;
pub mod objects;
pub mod paper;
pub mod render;
pub mod styles;
pub mod wordmode;

pub use kn_format;
pub use kn_model;
pub use kn_render;

/// Pages' Blank template (word processing, US Letter). Apple's artwork: local use only.
pub const BLANK: &[u8] = include_bytes!("../data/Blank.pages");
/// Pages' Blank Layout template (page layout).
pub const BLANK_LAYOUT: &[u8] = include_bytes!("../data/BlankLayout.pages");

/// Stored text as plain text: breaks as new lines, attachment characters left out.
pub fn text_plain(stored: &str) -> String {
    stored
        .chars()
        .filter_map(|c| match c {
            '\u{FFFC}' | '\u{E}' => None,
            '\u{5}' | '\u{c}' | '\u{4}' | '\r' | '\u{2028}' | '\u{2029}' => Some('\n'),
            c => Some(c),
        })
        .collect()
}

/// Load the Pages schema; call once before opening any file.
pub fn init() {
    kn_format::schema::use_schema(include_str!("../data/schema.json"), include_str!("../data/registry.json"));
}

/// Page size, margins and the Document inspector's switches (TP.DocumentArchive + TP.SettingsArchive).
#[derive(Debug, Clone, PartialEq)]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub header_margin: f32,
    pub footer_margin: f32,
    pub landscape: bool,
    pub paper_id: String,
    /// true: word processing (one flowing body); false: page layout.
    pub body: bool,
    pub headers: bool,
    pub footers: bool,
    pub facing_pages: bool,
    pub hyphenation: bool,
    pub ligatures: bool,
    pub change_tracking: bool,
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup {
            width: 612.0,
            height: 792.0,
            left: 72.0,
            right: 72.0,
            top: 72.0,
            bottom: 72.0,
            header_margin: 36.0,
            footer_margin: 43.2,
            landscape: false,
            paper_id: "na-letter".into(),
            body: true,
            headers: true,
            footers: true,
            facing_pages: false,
            hyphenation: false,
            ligatures: true,
            change_tracking: false,
        }
    }
}

impl PageSetup {
    /// The text column's width.
    pub fn text_width(&self) -> f32 {
        (self.width - self.left - self.right).max(36.0)
    }
    pub fn text_height(&self) -> f32 {
        (self.height - self.top - self.bottom).max(36.0)
    }
}

/// One section of the body (TP.SectionArchive): where it starts and its masters.
#[derive(Debug, Clone)]
pub struct Section {
    pub id: ObjId,
    /// UTF-16 index in the body where the section starts.
    pub start16: usize,
    /// First / even / odd page masters (TP.SectionTemplateArchive).
    pub first: Option<ObjId>,
    pub even: Option<ObjId>,
    pub odd: Option<ObjId>,
    pub first_page_different: bool,
    pub even_odd_different: bool,
    pub first_page_hides_header_footer: bool,
    pub inherit_previous: bool,
    /// 0: continue from the previous section, else start at `page_number_start`.
    pub page_number_kind: u64,
    pub page_number_start: u64,
    pub background: Option<kn_model::Fill>,
}

impl Section {
    /// The master for a page of this section (0-based page in the section, 1-based page number).
    pub fn master(&self, page_in_section: usize, page_number: usize) -> Option<ObjId> {
        if page_in_section == 0 && self.first_page_different {
            return self.first.or(self.odd);
        }
        if self.even_odd_different && page_number % 2 == 0 {
            return self.even.or(self.odd);
        }
        self.odd.or(self.first)
    }
}

/// A Pages document.
pub struct Pages {
    pub doc: Document,
    /// A Word document (opened from .docx): laid out as Word / OnlyOffice lay it out (their
    /// font stand-ins, line heights and spacing) rather than as Pages does.
    pub word_layout: bool,
    /// A Word document's sections with their own page size, orientation and margins (Word sets
    /// them per section; a Pages document has one page setup). Keyed by section object.
    pub section_pages: std::collections::HashMap<ObjId, PageSetup>,
    /// A Word document's tables kept whole on a page (rows that don't split, each kept with
    /// the next): a paragraph kept with one moves with all of it.
    pub keep_tables: std::collections::HashSet<ObjId>,
    /// A Word document's tables' exact-height rows (Word's hRule="exact"), by table.
    pub exact_rows: std::collections::HashMap<ObjId, Vec<bool>>,
}

impl Pages {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Pages, kn_format::Error> {
        init();
        Ok(Pages { doc: Document::open(path)?, word_layout: false, section_pages: Default::default(), keep_tables: Default::default(), exact_rows: Default::default() })
    }

    pub fn open_with_password(path: impl AsRef<std::path::Path>, password: &str, hint: &str) -> Result<Pages, kn_format::Error> {
        init();
        Ok(Pages { doc: Document::open_with_password(path, password, hint)?, word_layout: false, section_pages: Default::default(), keep_tables: Default::default(), exact_rows: Default::default() })
    }

    /// A new untitled document from the Blank (word processing) or Blank Layout template.
    pub fn new_blank(layout: bool) -> Pages {
        init();
        let k = KeyFile::from_zip(if layout { BLANK_LAYOUT } else { BLANK }).expect("template opens");
        let mut doc = Document::from_file(k);
        doc.path = None;
        Pages { doc, word_layout: false, section_pages: Default::default(), keep_tables: Default::default(), exact_rows: Default::default() }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Pages, kn_format::Error> {
        init();
        Ok(Pages { doc: Document::from_file(KeyFile::from_zip(bytes)?), word_layout: false, section_pages: Default::default(), keep_tables: Default::default(), exact_rows: Default::default() })
    }

    pub fn file(&self) -> &KeyFile {
        &self.doc.file
    }

    /// TP.DocumentArchive (id and message).
    pub fn root(&self) -> Option<(ObjId, &Msg)> {
        self.file().first_of("TP.DocumentArchive")
    }

    pub fn root_id(&self) -> Option<ObjId> {
        self.root().map(|r| r.0)
    }

    pub fn settings_id(&self) -> Option<ObjId> {
        self.root()?.1.ref_id("settings")
    }

    /// The main text flow (word-processing documents; page-layout documents keep an empty one).
    pub fn body_storage(&self) -> Option<ObjId> {
        self.root()?.1.ref_id("body_storage")
    }

    pub fn stylesheet(&self) -> Option<ObjId> {
        self.root()?.1.ref_id("stylesheet")
    }

    pub fn is_word_processing(&self) -> bool {
        self.setup().body
    }

    pub fn setup(&self) -> PageSetup {
        let mut s = PageSetup::default();
        let Some((_, d)) = self.root() else { return s };
        s.width = d.f32("page_width").unwrap_or(s.width);
        s.height = d.f32("page_height").unwrap_or(s.height);
        s.left = d.f32("left_margin").unwrap_or(s.left);
        s.right = d.f32("right_margin").unwrap_or(s.right);
        s.top = d.f32("top_margin").unwrap_or(s.top);
        s.bottom = d.f32("bottom_margin").unwrap_or(s.bottom);
        s.header_margin = d.f32("header_margin").unwrap_or(s.header_margin);
        s.footer_margin = d.f32("footer_margin").unwrap_or(s.footer_margin);
        s.landscape = d.u64("orientation").unwrap_or(0) == 1;
        s.paper_id = d.str("paper_id").unwrap_or("na-letter").to_string();
        s.change_tracking = d.bool("change_tracking_enabled").unwrap_or(false);
        if let Some(st) = d.ref_id("settings").and_then(|id| self.file().obj(id)) {
            s.body = st.bool("body").unwrap_or(true);
            s.headers = st.bool("headers").unwrap_or(true);
            s.footers = st.bool("footers").unwrap_or(true);
            s.facing_pages = st.bool("facing_pages").unwrap_or(false);
            s.hyphenation = st.bool("hyphenation").unwrap_or(false);
            s.ligatures = st.bool("use_ligatures").unwrap_or(true);
        }
        s
    }

    /// Write the page setup back (one undo step when called between begin/end).
    pub fn set_setup(&mut self, h: &mut History, s: &PageSetup) {
        let (Some(root), settings) = (self.root_id(), self.settings_id()) else { return };
        self.doc.touch(h, root);
        if let Some(d) = self.doc.file.obj_mut(root) {
            d.set_f32("page_width", s.width);
            d.set_f32("page_height", s.height);
            d.set_f32("left_margin", s.left);
            d.set_f32("right_margin", s.right);
            d.set_f32("top_margin", s.top);
            d.set_f32("bottom_margin", s.bottom);
            d.set_f32("header_margin", s.header_margin);
            d.set_f32("footer_margin", s.footer_margin);
            d.set_u64("orientation", u64::from(s.landscape));
            d.set_str("paper_id", &s.paper_id);
            d.set_bool("change_tracking_enabled", s.change_tracking);
        }
        if let Some(sid) = settings {
            self.doc.touch(h, sid);
            if let Some(st) = self.doc.file.obj_mut(sid) {
                st.set_bool("body", s.body);
                st.set_bool("headers", s.headers);
                st.set_bool("footers", s.footers);
                st.set_bool("facing_pages", s.facing_pages);
                st.set_bool("hyphenation", s.hyphenation);
                st.set_bool("use_ligatures", s.ligatures);
            }
        }
    }

    /// TP.SettingsArchive string/bool fields not covered by [`PageSetup`].
    pub fn setting_bool(&self, name: &str) -> Option<bool> {
        self.settings_id().and_then(|id| self.file().obj(id)).and_then(|m| m.bool(name))
    }

    pub fn set_setting_bool(&mut self, h: &mut History, name: &str, v: bool) {
        let Some(sid) = self.settings_id() else { return };
        self.doc.touch(h, sid);
        if let Some(m) = self.doc.file.obj_mut(sid) {
            m.set_bool(name, v);
        }
    }

    /// Sections of the body in text order.
    pub fn sections(&self) -> Vec<Section> {
        let k = self.file();
        let Some(body) = self.body_storage().and_then(|b| k.obj(b)) else { return Vec::new() };
        let mut out: Vec<Section> = body
            .msg("table_section")
            .map(|t| {
                t.msgs("entries")
                    .filter_map(|e| {
                        let id = e.ref_id("object")?;
                        Some(section_of(k, id, e.u64("character_index").unwrap_or(0) as usize))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if out.is_empty() {
            // page-layout documents: the first section object in the file
            if let Some((id, _)) = k.first_of("TP.SectionArchive") {
                out.push(section_of(k, id, 0));
            }
        }
        out.sort_by_key(|s| s.start16);
        out
    }

    /// Header (or footer) storages of a master: left, centre, right.
    pub fn header_footer_storages(&self, master: ObjId, footer: bool) -> Vec<ObjId> {
        self.file().obj(master).map(|m| m.ref_ids(if footer { "footers" } else { "headers" })).unwrap_or_default()
    }

    /// Floating ("Stay on Page") drawables by page index, in their stored order.
    pub fn floating(&self) -> Vec<(usize, ObjId)> {
        let k = self.file();
        let Some(f) = self.root().and_then(|r| r.1.ref_id("floating_drawables")).and_then(|id| k.obj(id)) else { return Vec::new() };
        let mut v = Vec::new();
        for g in f.msgs("page_groups") {
            let page = g.u64("page_index").unwrap_or(0) as usize;
            for d in g.msgs("drawables") {
                if let Some(id) = d.ref_id("drawable") {
                    v.push((page, id));
                }
            }
        }
        // back to front, as TP.DrawablesZOrderArchive lists them
        let z = self.zorder();
        v.sort_by_key(|(_, id)| z.iter().position(|x| x == id).unwrap_or(usize::MAX));
        v
    }

    pub fn zorder(&self) -> Vec<ObjId> {
        let k = self.file();
        self.root().and_then(|r| r.1.ref_id("drawables_zorder")).and_then(|id| k.obj(id)).map(|z| z.ref_ids("drawables")).unwrap_or_default()
    }

    /// Page templates of a page-layout document (TP.PageTemplateArchive: id, name).
    pub fn page_templates(&self) -> Vec<(ObjId, String)> {
        let k = self.file();
        self.root()
            .map(|r| r.1.ref_ids("page_templates"))
            .unwrap_or_default()
            .into_iter()
            .map(|id| (id, k.obj(id).and_then(|m| m.str("name")).unwrap_or("").to_string()))
            .collect()
    }

    /// The page count Pages saved in the view state (page-layout documents have no flow to count).
    pub fn saved_page_count(&self) -> usize {
        let k = self.file();
        let hints: usize = k
            .first_of("TP.LayoutStateArchive")
            .map(|(_, l)| l.msgs("section_hints").map(|s| s.count("page_hints")).sum())
            .unwrap_or(0);
        hints.max(k.first_of("TP.LayoutStateArchive").and_then(|(_, l)| l.u64("last_page_count")).unwrap_or(1) as usize).max(1)
    }

    /// Plain text of the body (as stored: breaks and attachment characters included).
    pub fn body_text(&self) -> String {
        self.body_storage().map(|b| self.doc.storage_text(b)).unwrap_or_default()
    }

    /// The document's saved window state (zoom, ruler, sidebar…).
    pub fn ui_state(&self) -> UiState {
        let mut u = UiState::default();
        if let Some((_, m)) = self.file().first_of("TP.UIStateArchive") {
            u.zoom = m.f32("view_scale").unwrap_or(1.25);
            u.ruler = m.bool("ruler_visible").unwrap_or(false);
            u.word_count = m.bool("word_count_hud_visible").unwrap_or(false);
            u.thumbnails = m.bool("shows_page_navigator").unwrap_or(false);
            u.toc = m.bool("shows_toc_navigator").unwrap_or(false);
            u.inspector_hidden = m.bool("inspector_hidden").unwrap_or(false);
            u.two_up = m.enum_name("page_view_state") == Some("PageViewState_TwoUp");
        }
        u
    }

    /// Remember the window state in the file (saved with the document).
    pub fn set_ui_state(&mut self, u: &UiState) {
        let Some((id, _)) = self.file().first_of("TP.UIStateArchive") else { return };
        if let Some(m) = self.doc.file.obj_mut(id) {
            m.set_f32("view_scale", u.zoom);
            m.set_bool("ruler_visible", u.ruler);
            m.set_bool("word_count_hud_visible", u.word_count);
            m.set_bool("shows_page_navigator", u.thumbnails);
            m.set_bool("shows_toc_navigator", u.toc);
            m.set_bool("inspector_hidden", u.inspector_hidden);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiState {
    pub zoom: f32,
    pub ruler: bool,
    pub word_count: bool,
    pub thumbnails: bool,
    pub toc: bool,
    pub inspector_hidden: bool,
    pub two_up: bool,
}

impl Default for UiState {
    fn default() -> Self {
        UiState { zoom: 1.25, ruler: false, word_count: false, thumbnails: false, toc: false, inspector_hidden: false, two_up: false }
    }
}

fn section_of(k: &KeyFile, id: ObjId, start16: usize) -> Section {
    let m = k.obj(id);
    let b = |n: &str| m.and_then(|m| m.bool(n)).unwrap_or(false);
    Section {
        id,
        start16,
        first: m.and_then(|m| m.ref_id("first_section_template_page")),
        even: m.and_then(|m| m.ref_id("even_section_template_page")),
        odd: m.and_then(|m| m.ref_id("odd_section_template_page")),
        first_page_different: b("section_template_first_page_different"),
        even_odd_different: b("section_template_even_odd_pages_different"),
        first_page_hides_header_footer: b("section_template_first_page_hides_header_footer"),
        inherit_previous: m.and_then(|m| m.bool("inherit_previous_header_footer")).unwrap_or(true),
        page_number_kind: m.and_then(|m| m.u64("section_page_number_kind")).unwrap_or(0),
        page_number_start: m.and_then(|m| m.u64("section_page_number_start")).unwrap_or(1),
        background: m.and_then(|m| m.msg("background_fill")).map(kn_model::style::fill).filter(|f| !matches!(f, kn_model::Fill::None)),
    }
}

#[cfg(test)]
mod tests;
