//! The window: toolbar, the page canvas, the sidebar (page thumbnails / table of contents),
//! the ruler, the Format / Document inspector, laid out like Pages 15.4 (research
//! ui-geometry/UI-GEOMETRY.txt) with the suite's look (Keydeck / Values, orange accent).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Color32, RichText};
use kn_ui::theme;
use pg_core::flow::{self, Layout};
use pg_core::kn_format::ObjId;
use pg_core::kn_model::edit::History;
use pg_core::kn_model::{Document, Drawable};
use pg_core::Pages;

use crate::canvas::Canvas;

/// Which inspector tab group the Format inspector shows for table cells (Values' tabs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InspTab {
    Table,
    Cell,
    Text,
    Arrange,
}

/// The toolbar's Format | Document switch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InspMode {
    Format,
    Document,
}

/// View > Page Thumbnails / Table of Contents / Document Only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sidebar {
    None,
    Thumbnails,
    Toc,
}

/// Selected cells of one table: anchor and cursor (inclusive corners).
/// Word mode (as Values' Excel mode): the .docx it saves to, and whether the whole Letters
/// document travels inside it.
#[derive(Clone, Debug)]
pub struct WordMode {
    pub path: PathBuf,
    pub hybrid: bool,
}

impl WordMode {
    pub fn label(&self) -> &'static str {
        if self.hybrid { "Word + Letters" } else { "Word" }
    }
}

/// What plain Word mode can't keep (greyed there; Word + Letters keeps everything).
pub fn word_blocks(name: &str) -> bool {
    matches!(
        name,
        "Add Image Gallery…" | "Add Web Video…" | "Record Audio" | "Choose Music…" | "Line > Straight Connection Line" | "Line > Curved Connection Line" | "Line > Right Angle Connection Line" | "Conditional Highlighting…"
    ) || name.starts_with("Chart > Interactive")
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sel {
    pub table: ObjId,
    pub anchor: (u32, u32),
    pub cursor: (u32, u32),
}

impl Sel {
    pub fn cell(table: ObjId, r: u32, c: u32) -> Sel {
        Sel { table, anchor: (r, c), cursor: (r, c) }
    }
    pub fn block(&self) -> (u32, u32, u32, u32) {
        (self.anchor.0.min(self.cursor.0), self.anchor.1.min(self.cursor.1), self.anchor.0.max(self.cursor.0), self.anchor.1.max(self.cursor.1))
    }
}

/// Where the text caret is.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Owner {
    /// The document body.
    Body,
    /// A text box or shape's own text.
    Drawable(ObjId),
    /// A header / footer field shown on `page`.
    HeaderFooter { page: usize },
    /// A footnote shown on `page`.
    Footnote { page: usize },
}

/// The text caret and selection: stored byte offsets in `storage`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cursor {
    pub storage: ObjId,
    pub owner: Owner,
    pub anchor: usize,
    pub caret: usize,
    /// The x the caret keeps while moving up and down (page points).
    pub goal_x: Option<f32>,
}

impl Cursor {
    pub fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.caret), self.anchor.max(self.caret))
    }
    pub fn is_empty(&self) -> bool {
        self.anchor == self.caret
    }
}

/// The current page for the ported inspector (it calls it a slide).
pub struct CurSheet {
    pub slide: ObjId,
}

pub struct App {
    pub wb: Option<Pages>,
    pub hist: History,
    /// Bumped on every change to the document (layout and objects re-read).
    pub version: u64,
    pub layout: Option<Layout>,
    pub layout_for: (u64, u64),
    pub flow_cache: flow::Cache,
    /// Every object on every page as the renderer sees it (page-local frames).
    pub drawables: Vec<Drawable>,
    /// Section layout objects (wallpaper): one copy each; shown on every page whose layout
    /// lists them, not selectable unless Arrange > Section Layouts > Make Layout Objects Selectable.
    pub layout_drawables: Vec<Drawable>,
    pub layout_selectable: bool,
    /// Insert > Browse Content Hub: openly licensed pictures from Wikimedia Commons.
    pub hub: kn_ui::contenthub::Hub,
    /// The floating toolbar's band this frame (the canvas draws what is under it).
    pub toolbar_band: Option<egui::Rect>,
    /// What the canvas made for the band: (blurred view, where, tinted).
    pub toolbar_band_paint: Option<(egui::TextureHandle, egui::Rect, bool)>,
    /// The page each object is on.
    /// The page an object starts on (a table continued over pages: its first).
    pub obj_page: HashMap<ObjId, usize>,
    /// The page each of `drawables` is drawn on (a table continued over pages has a part on
    /// each, sharing its id).
    pub drawable_page: Vec<usize>,
    pub canvas: Canvas,
    pub cursor: Option<Cursor>,
    /// Formatting picked with no text selected: applied to what is typed next.
    pub typing_props: Vec<crate::text::CharChange>,
    pub sel: Option<Sel>,
    pub inspector: bool,
    pub insp_mode: InspMode,
    pub insp_tab: InspTab,
    /// Text inspector: 0 Style, 1 Layout, 2 More. Document inspector: 0 Document, 1 Section, 2 Bookmarks.
    pub text_tab: usize,
    pub doc_tab: usize,
    pub obj_tab: usize,
    pub sidebar: Sidebar,
    pub ruler: bool,
    pub word_count: bool,
    /// 0 words, 1 characters, 2 characters without spaces, 3 paragraphs, 4 pages.
    pub word_count_mode: u8,
    pub invisibles: bool,
    pub screen_view: bool,
    /// A text field (dialog, inspector) had keyboard focus last frame: Tab is its own.
    pub widget_focus: bool,
    pub comments_pane: bool,
    pub show_comments: bool,
    pub show_toolbar: bool,
    pub show_layout: bool,
    /// Page-layout documents: pages shown even if empty (Add Page this session).
    pub min_pages: usize,
    pub message: Option<(String, f64)>,
    /// Edit > Find: the query, matches (stored byte ranges in the body) and the current one.
    pub find: Option<(String, Vec<(usize, usize)>, usize)>,
    pub replace: Option<String>,
    pub find_case: bool,
    pub find_words: bool,
    snap: Option<(PathBuf, u32)>,
    pub tabs: crate::tabs::Tabs,
    /// The document is a Word file (Save writes it back): plain, or with the Letters copy
    /// inside (Word + Letters).
    pub word: Option<WordMode>,
    /// A mouse button is held this frame (a slider or colour being dragged): edits with the same
    /// label then merge into one undo step instead of one per frame.
    pub pointer_down: bool,
    script: std::collections::VecDeque<Vec<egui::Event>>,
    script_cmds: Vec<String>,
    pub automated: bool,
    password: Option<(PathBuf, String, String, bool)>,
    pub set_password: Option<(String, String, String)>,
    perf: Vec<f32>,
    pub buttons: kn_ui::winframe::ButtonLayout,
    pub menu_bar_width: Option<f32>,
    pub title_press_at: f64,
    pub show_about: bool,
    /// Format > Add Link: (kind, text typed); applies to the selected text or object.
    pub link_edit: Option<(String, String)>,
    pub doc_title: Option<(String, Option<PathBuf>)>,
    pub untitled_name: Option<String>,
    pub tb: crate::toolbar::TbState,
    pub points: crate::points::PointEdit,
    pub show_colors: bool,
    pub colors_pick: Color32,
    pub color_state: kn_ui::colors::ColorPanelState,
    pub spelling: Option<crate::spelling::Spelling>,
    pub show_versions: bool,
    pub show_fonts: bool,
    pub font_query: String,
    pub replace_fonts: Option<crate::fonts::ReplaceFonts>,
    pub chart_edit: Option<crate::chartedit::ChartEdit>,
    pub show_shortcuts: bool,
    /// Copied objects (Edit > Copy with objects selected).
    pub obj_clip: Option<pg_core::kn_model::insert::Clip>,
    pub obj_clip_active: bool,
    /// Copied text with its formatting (within Letters).
    pub text_clip: Option<crate::text::TextClip>,
    corner: f32,
    pub desc_buf: Option<(ObjId, String)>,
    pub font_names: Option<Vec<String>>,
    pub replace_target: Option<ObjId>,
    pub mask_edit: Option<ObjId>,
    pub copied_style: Option<crate::editing::CopiedStyle>,
    pub look: crate::look::Look,
    pub settings: crate::settings::Settings,
    pub show_settings: bool,
    /// View > Go To > Page…: the page number typed.
    pub go_to: Option<String>,
    /// File > New: the template chooser.
    pub chooser: bool,
    /// Where the toolbar ends (the start screen goes below it).
    pub welcome_top: f32,
    /// Comment being written / edited: (highlight or None for new, text).
    pub comment_edit: Option<(Option<ObjId>, String)>,
    /// Format > Advanced > Create Style / rename: the name typed.
    pub new_style: Option<String>,
    /// Insert > Equation…: the LaTeX typed.
    pub equation: Option<String>,
    /// The equation picture being edited (double-clicked): replaced by the new one.
    pub equation_target: Option<ObjId>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext, files: Vec<PathBuf>) -> App {
        let look = crate::look::Look::load();
        look.apply(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| {
            o.zoom_with_keyboard = false;
            // a wheel notch goes as far as in GTK apps (about the viewport^(2/3)), not egui's 40
            // (the page canvas eases notches itself, at the same distance)
            o.input_options.line_scroll_speed = 100.0;
        });
        let settings = crate::settings::Settings::load();
        let mut app = App {
            wb: None,
            hist: History::default(),
            version: 1,
            layout: None,
            layout_for: (0, 0),
            flow_cache: flow::Cache::default(),
            drawables: Vec::new(),
            layout_drawables: Vec::new(),
            layout_selectable: false,
            hub: Default::default(),
            toolbar_band: None,
            toolbar_band_paint: None,
            obj_page: HashMap::new(),
            drawable_page: Vec::new(),
            canvas: Canvas::default(),
            cursor: None,
            typing_props: Vec::new(),
            sel: None,
            inspector: true,
            insp_mode: InspMode::Format,
            insp_tab: InspTab::Text,
            text_tab: 0,
            doc_tab: 0,
            obj_tab: 0,
            sidebar: Sidebar::None,
            ruler: false,
            word_count: false,
            word_count_mode: 0,
            invisibles: false,
            screen_view: false,
            widget_focus: false,
            comments_pane: false,
            show_comments: true,
            show_toolbar: true,
            show_layout: false,
            min_pages: 1,
            message: None,
            find: None,
            replace: None,
            find_case: false,
            find_words: false,
            snap: std::env::var_os("CS_SNAP").map(|p| (PathBuf::from(p), 0)),
            tabs: Default::default(),
            word: None,
            pointer_down: false,
            script: std::env::var("CS_SCRIPT").map(|s| parse_script(&s)).unwrap_or_default(),
            script_cmds: Vec::new(),
            automated: std::env::var_os("CS_SNAP").is_some(),
            password: None,
            set_password: None,
            perf: Vec::new(),
            buttons: kn_ui::winframe::ButtonLayout::from_desktop(),
            menu_bar_width: None,
            title_press_at: -1.0,
            show_about: false,
            link_edit: None,
            doc_title: None,
            untitled_name: None,
            tb: Default::default(),
            points: Default::default(),
            show_colors: false,
            colors_pick: Color32::from_rgb(0xf7, 0xa2, 0x3e),
            color_state: Default::default(),
            spelling: None,
            show_versions: false,
            show_fonts: false,
            font_query: String::new(),
            replace_fonts: None,
            chart_edit: None,
            show_shortcuts: false,
            obj_clip: None,
            obj_clip_active: false,
            text_clip: None,
            corner: 0.0,
            desc_buf: None,
            font_names: None,
            replace_target: None,
            mask_edit: None,
            copied_style: None,
            look,
            settings,
            show_settings: false,
            go_to: None,
            chooser: false,
            welcome_top: 0.0,
            comment_edit: None,
            new_style: None,
            equation: None,
            equation_target: None,
        };
        app.canvas.zoom = app.settings.default_zoom;
        match files.split_first() {
            Some((p, rest)) => {
                app.open(p);
                for f in rest {
                    crate::tabs::open_in_tab(&mut app, f);
                }
            }
            None => {
                app.new_document(false);
                // a new window starts with the template chooser (Settings > For New Documents)
                app.chooser = (app.settings.show_chooser && !app.automated) || std::env::var_os("CS_WELCOME").is_some();
            }
        }
        if let Some(z) = std::env::var("CS_ZOOM").ok().and_then(|s| s.parse::<f32>().ok()) {
            app.canvas.zoom = z.clamp(0.1, 4.0);
        }
        app
    }

    pub fn doc(&self) -> Option<&Document> {
        self.wb.as_ref().map(|w| &w.doc)
    }

    pub fn notify(&mut self, s: impl Into<String>) {
        let s = s.into();
        if std::env::var_os("CS_DEBUG").is_some() {
            eprintln!("notify: {s}");
        }
        self.message = Some((s, 0.0));
    }

    pub fn changed(&mut self) {
        self.version += 1;
        if let Some(w) = &mut self.wb {
            w.doc.dirty = true;
        }
    }

    /// Something on a page changed (the ported inspector's call).
    pub fn bump(&mut self, _page: ObjId) {
        self.changed();
    }

    /// Run a menu command (the ported inspector's buttons).
    pub fn command(&mut self, ctx: &egui::Context, name: &str) {
        crate::menus::run(self, ctx, name);
    }

    pub fn current_slide(&self) -> Option<CurSheet> {
        self.root_id().map(|slide| CurSheet { slide })
    }

    pub fn root_id(&self) -> Option<ObjId> {
        self.wb.as_ref().and_then(|w| w.root_id())
    }

    /// File > Open Recent (most recent first, kept in ~/.config/letters/recent.txt). Read
    /// once; whether each file still exists is checked off the UI thread, since the files may
    /// sit on a network mount (a cloud drive) where a check can take a while the first time.
    pub fn recent(&self) -> Vec<PathBuf> {
        let mut g = RECENT.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            let list: Vec<PathBuf> = recent_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| s.lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect()).unwrap_or_default();
            *g = Some(list.clone());
            std::thread::spawn(move || {
                let gone: Vec<PathBuf> = list.into_iter().filter(|p| !p.exists()).collect();
                if !gone.is_empty() {
                    if let Some(l) = RECENT.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                        l.retain(|p| !gone.contains(p));
                    }
                }
            });
        }
        g.clone().unwrap_or_default()
    }

    pub fn remember(&self, p: &Path) {
        if self.automated {
            return;
        }
        let Some(file) = recent_path() else { return };
        let mut list = self.recent();
        list.retain(|x| x != p);
        list.insert(0, p.to_path_buf());
        list.truncate(10);
        *RECENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(list.clone());
        let _ = std::fs::create_dir_all(file.parent().unwrap());
        let _ = std::fs::write(file, list.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n"));
    }

    pub fn load(&mut self, p: Pages) {
        let u = p.ui_state();
        self.ruler = u.ruler;
        self.word_count = u.word_count;
        self.sidebar = if u.toc { Sidebar::Toc } else if u.thumbnails && std::env::var_os("CS_SIDEBAR").is_some() { Sidebar::Thumbnails } else { self.sidebar };
        // (the phone layout: documents open with the inspector and sidebar closed)
        self.inspector = (!u.inspector_hidden || self.automated) && !kn_ui::form::was_compact();
        if kn_ui::form::was_compact() {
            self.sidebar = Sidebar::None;
        }
        self.canvas.two_up = u.two_up;
        self.wb = Some(p);
        self.word = None;
        self.hist = History::default();
        self.cursor = None;
        self.sel = None;
        self.canvas.obj = None;
        self.canvas.objs.clear();
        self.min_pages = 1;
        self.version += 1;
        self.layout = None;
        self.canvas.reset();
        // the caret at the start of the body (Pages opens with it there)
        if let Some(b) = self.wb.as_ref().and_then(|w| w.body_storage()).filter(|_| self.wb.as_ref().is_some_and(|w| w.is_word_processing())) {
            self.cursor = Some(Cursor { storage: b, owner: Owner::Body, anchor: 0, caret: 0, goal_x: None });
        }
    }

    pub fn new_document(&mut self, layout: bool) {
        let p = Pages::new_blank(layout);
        self.load(p);
        self.untitled_name = None;
        if !layout {
            self.apply_default_font();
        }
    }

    /// Settings > Default Font: new Basic documents' Body style uses it.
    fn apply_default_font(&mut self) {
        let (font, size) = (self.settings.default_font.clone(), self.settings.default_font_size);
        if font.is_empty() {
            return;
        }
        let Some(w) = &mut self.wb else { return };
        let Some(body_style) = pg_core::edit::para_style_named(&w.doc, "Body") else { return };
        if let Some(m) = w.doc.file.obj_mut(body_style) {
            let cp = m.msg_mut_or_create("char_properties");
            let ps = kn_render_ps_name(&font);
            cp.set_str("font_name", &ps);
            cp.set_f32("font_size", size);
        }
        w.doc.dirty = false;
    }

    pub fn open(&mut self, p: &Path) {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        // a Word file opens in Word mode: it stays a .docx
        if ext == "docx" {
            match pg_core::wordmode::open_docx(p) {
                Ok(o) => {
                    self.remember(p);
                    self.load(o.pages);
                    self.untitled_name = None;
                    self.word = Some(WordMode { path: p.to_path_buf(), hybrid: o.hybrid });
                    if o.changed_elsewhere {
                        self.notify("This document was changed in another app since Letters saved it: it opened from the Word content, and the Letters-only extras it had are gone.");
                    }
                }
                Err(e) => self.notify(format!("Couldn't open {}: {e}", p.display())),
            }
            return;
        }
        let opened = match ext.as_str() {
            "txt" | "md" | "markdown" | "rtf" | "docx" | "html" | "htm" => pg_core::import::import_file(p),
            _ => Pages::open(p),
        };
        match opened {
            Ok(doc) => {
                self.remember(p);
                let imported = doc.doc.path.is_none();
                self.load(doc);
                self.untitled_name = if imported { p.file_stem().map(|n| n.to_string_lossy().into_owned()) } else { None };
            }
            Err(pg_core::kn_format::Error::NeedsPassword(hint)) => self.password = Some((p.to_path_buf(), hint, String::new(), false)),
            Err(e) => self.notify(format!("Couldn't open {}: {e}", p.display())),
        }
    }

    /// A password is being asked for (the document opens after it).
    pub fn password_pending(&self) -> bool {
        self.password.is_some()
    }

    pub fn open_dialog(&mut self) {
        if let Some(p) = file_dialog(|d| {
            d.add_filter("Documents", &["pages", "docx", "rtf", "txt", "md", "html"])
                .add_filter("Pages document", &["pages"])
                .add_filter("Word document", &["docx"])
                .add_filter("Rich Text", &["rtf"])
                .add_filter("Plain Text / Markdown", &["txt", "md"])
                .pick_files()
        }) {
            // each in its own tab
            for f in p {
                crate::tabs::open_in_tab(self, &f);
            }
        }
    }

    pub fn title(&self) -> String {
        let Some(w) = &self.wb else { return "Letters".into() };
        if let Some(wm) = &self.word {
            return wm.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        }
        match &w.doc.path {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            None => self.untitled_name.clone().unwrap_or_else(|| "Untitled".into()),
        }
    }

    pub fn save(&mut self, save_as: bool) {
        // Word mode: Save writes the .docx back (Save As can still make a Pages document)
        if let (Some(wm), false) = (self.word.clone(), save_as) {
            self.save_word(&wm.path, wm.hybrid);
            return;
        }
        let Some(w) = &mut self.wb else { return };
        let path = match (&w.doc.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let stem = self.untitled_name.clone().or_else(|| self.word.as_ref().and_then(|wm| wm.path.file_stem()).map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "Untitled".into());
                match file_dialog(|d| d.add_filter("Pages document", &["pages"]).add_filter("Word document", &["docx"]).set_file_name(format!("{stem}.pages")).save_file()) {
                    Some(p) => p,
                    None => return,
                }
            }
        };
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("docx")) {
            // a Pages document saved as Word keeps everything inside unless told otherwise
            let hybrid = self.word.as_ref().map(|w| w.hybrid).unwrap_or(true);
            self.save_word(&path, hybrid);
            return;
        }
        self.word = None;
        self.save_to(&path);
    }

    /// Write the document as a Word file and keep working on it in Word mode.
    pub fn save_word(&mut self, path: &Path, hybrid: bool) {
        crate::text::finish_typing(self);
        crate::tables::commit(self);
        if path.exists() {
            crate::fileops::keep_version(path);
        }
        let Some(w) = &mut self.wb else { return };
        match pg_core::wordmode::save_docx(w, path, hybrid) {
            Ok(()) => {
                w.doc.path = None;
                w.word_layout = true;
                self.word = Some(WordMode { path: path.to_path_buf(), hybrid });
                self.untitled_name = None;
                self.remember(path);
                self.notify(format!("Saved {}", path.display()));
            }
            Err(e) => self.notify(format!("Couldn't save: {e}")),
        }
    }

    pub fn save_to(&mut self, path: &Path) {
        crate::text::finish_typing(self);
        let ui = self.ui_state();
        let Some(w) = &mut self.wb else { return };
        if w.doc.path.as_deref() == Some(path) {
            crate::fileops::keep_version(path);
        }
        w.set_ui_state(&ui);
        match w.save_as(path) {
            Ok(()) => {
                self.untitled_name = None;
                self.remember(path);
            }
            Err(e) => self.notify(format!("Couldn't save: {e}")),
        }
    }

    fn ui_state(&self) -> pg_core::UiState {
        pg_core::UiState {
            zoom: self.canvas.zoom,
            ruler: self.ruler,
            word_count: self.word_count,
            thumbnails: self.sidebar == Sidebar::Thumbnails,
            toc: self.sidebar == Sidebar::Toc,
            inspector_hidden: !self.inspector,
            two_up: self.canvas.two_up,
        }
    }

    /// Run an undoable edit on the document (one undo step).
    pub fn edit<R>(&mut self, label: &str, f: impl FnOnce(&mut Document, &mut History) -> R) -> Option<R> {
        crate::text::finish_typing(self);
        let wb = self.wb.as_mut()?;
        if self.pointer_down {
            wb.doc.begin_merge(&mut self.hist, label);
        } else {
            wb.doc.begin(&mut self.hist, label);
        }
        let r = f(&mut wb.doc, &mut self.hist);
        wb.doc.end(&mut self.hist);
        self.changed();
        Some(r)
    }

    /// Like [`App::edit`] with the whole Pages document.
    pub fn edit_pages<R>(&mut self, label: &str, f: impl FnOnce(&mut Pages, &mut History) -> R) -> Option<R> {
        crate::text::finish_typing(self);
        let wb = self.wb.as_mut()?;
        if self.pointer_down {
            wb.doc.begin_merge(&mut self.hist, label);
        } else {
            wb.doc.begin(&mut self.hist, label);
        }
        let r = f(wb, &mut self.hist);
        wb.doc.end(&mut self.hist);
        self.changed();
        Some(r)
    }

    pub fn undo(&mut self) {
        crate::text::finish_typing(self);
        if let Some(w) = &mut self.wb {
            if w.doc.undo(&mut self.hist) {
                self.changed();
                self.clamp_cursor();
            }
        }
    }

    pub fn redo(&mut self) {
        if let Some(w) = &mut self.wb {
            if w.doc.redo(&mut self.hist) {
                self.changed();
                self.clamp_cursor();
            }
        }
    }

    /// After undo / redo or other big changes: keep the caret inside its text.
    pub fn clamp_cursor(&mut self) {
        let Some(c) = self.cursor else { return };
        let Some(w) = &self.wb else { return };
        if w.file().obj(c.storage).is_none() {
            self.cursor = None;
            return;
        }
        let t = w.doc.storage_text(c.storage);
        let fix = |mut b: usize| {
            b = b.min(t.len());
            while !t.is_char_boundary(b) {
                b -= 1;
            }
            b
        };
        self.cursor = Some(Cursor { anchor: fix(c.anchor), caret: fix(c.caret), ..c });
    }

    /// Re-lay out the document and re-read its objects when it changed (or the view did).
    pub fn refresh(&mut self) {
        // (a Word document is laid out with Word's metrics: switching kinds lays it out again)
        let mut view_key = self.canvas.layout_key(self.screen_view) ^ u64::from(self.wb.as_ref().is_some_and(|w| w.word_layout)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        // an object being dragged: the text wraps around where it is now (Pages reflows live);
        // the move itself is saved when it's dropped (not while editing a picture's mask)
        let moving = self.canvas.moving().filter(|(ids, _)| !(ids.len() == 1 && self.mask_edit == Some(ids[0])));
        if let Some((ids, d)) = &moving {
            use std::hash::{Hash, Hasher};
            let mut hs = std::collections::hash_map::DefaultHasher::new();
            (ids, (d.x * 4.0).round() as i64, (d.y * 4.0).round() as i64).hash(&mut hs);
            view_key ^= hs.finish() | 1;
        }
        if self.layout.is_some() && self.layout_for == (self.version, view_key) {
            return;
        }
        let Some(w) = &self.wb else { return };
        let moving = moving.map(|(ids, d)| ids.into_iter().map(|id| (id, d.x, d.y)).collect()).unwrap_or_default();
        let opts = flow::Options { show_deletions: true, show_markup: self.show_comments, min_pages: self.min_pages, screen_width: self.screen_view.then(|| (self.canvas.size.x / self.canvas.zoom).max(200.0)), moving, ..flow::Options::default() };
        let t = std::time::Instant::now();
        let mut l = flow::layout(w, &mut self.canvas.tiles.renderer.fonts, &mut self.flow_cache, &opts);
        if std::env::var_os("CS_PERF").is_some() {
            eprintln!("layout: {} pages in {:.1} ms", l.pages.len(), t.elapsed().as_secs_f32() * 1000.0);
        }
        // objects of every page
        self.drawables.clear();
        self.drawable_page.clear();
        // a selected layout object keeps the page it was picked on
        let picked: Vec<(ObjId, usize)> = self.selected_objs().into_iter().filter_map(|i| self.layout_drawables.iter().any(|d| d.id == i).then(|| self.obj_page.get(&i).map(|p| (i, *p))).flatten()).collect();
        self.obj_page.clear();
        self.layout_drawables.clear();
        for p in &mut l.pages {
            for d in flow::page_drawables(w, p) {
                if p.objects.iter().any(|o| o.id == d.id && o.layout) {
                    if !self.layout_drawables.iter().any(|x| x.id == d.id) {
                        self.layout_drawables.push(d);
                    }
                    continue;
                }
                self.obj_page.entry(d.id).or_insert(p.index);
                self.drawables.push(d);
                self.drawable_page.push(p.index);
            }
        }
        crate::mediainsp::apply_gallery_pos(&mut self.drawables);
        for (i, pg) in picked {
            if let Some(d) = self.layout_drawables.iter().find(|d| d.id == i).cloned() {
                self.drawables.push(d);
                self.drawable_page.push(pg);
                self.obj_page.insert(i, pg);
            }
        }
        self.layout = Some(l);
        self.layout_for = (self.version, view_key);
        // selected objects that are gone
        let ids: Vec<ObjId> = self.drawables.iter().map(|d| d.id).collect();
        self.canvas.objs.retain(|o| ids.contains(o));
        if self.canvas.obj.is_some_and(|o| !ids.contains(&o)) {
            self.canvas.obj = None;
        }
        if self.canvas.editing.is_some_and(|o| !ids.contains(&o)) {
            self.canvas.editing = None;
        }
        self.clamp_cursor();
    }

    pub fn selected_objs(&self) -> Vec<ObjId> {
        if !self.canvas.objs.is_empty() {
            return self.canvas.objs.clone();
        }
        self.canvas.obj.into_iter().collect()
    }

    /// The table drawable and data with id `id`.
    /// The objects drawn on page `pi` (each part of a table continued over pages on its own).
    pub fn drawables_on(&self, pi: usize) -> impl Iterator<Item = &Drawable> {
        self.drawables.iter().zip(&self.drawable_page).filter(move |(_, p)| **p == pi).map(|(d, _)| d)
    }

    /// Table `id`'s part under page point `pt` (a table continued over pages), else its first.
    pub fn table_at(&self, id: ObjId, pt: egui::Pos2) -> Option<(&Drawable, &pg_core::kn_model::TableData)> {
        let parts: Vec<(&Drawable, &pg_core::kn_model::TableData)> = self
            .drawables
            .iter()
            .filter(|d| d.id == id)
            .filter_map(|d| match &d.kind {
                pg_core::kn_model::DrawableKind::Table(t) => Some((d, t)),
                _ => None,
            })
            .collect();
        parts.iter().find(|(d, _)| pt.y >= d.geom.y - 0.5 && pt.y <= d.geom.y + d.geom.h + 0.5 && pt.x >= d.geom.x - 0.5 && pt.x <= d.geom.x + d.geom.w + 0.5).or(parts.first()).copied()
    }

    pub fn table(&self, id: ObjId) -> Option<(&Drawable, &pg_core::kn_model::TableData)> {
        self.drawables.iter().find(|d| d.id == id).and_then(|d| match &d.kind {
            pg_core::kn_model::DrawableKind::Table(t) => Some((d, t)),
            _ => None,
        })
    }

    /// Values' model-row mapping (no categories in Pages: shown rows are model rows).
    pub fn mrow(&self, _t: ObjId, v: u32) -> Option<u32> {
        Some(v)
    }

    pub fn with_merges(&self, t: ObjId, b: (u32, u32, u32, u32)) -> (u32, u32, u32, u32) {
        let Some((_, td)) = self.table(t) else { return b };
        let (mut r0, mut c0, mut r1, mut c1) = b;
        loop {
            let mut grown = false;
            for m in &td.merges {
                let (mr0, mc0, mr1, mc1) = (m.row, m.col, m.row + m.rows - 1, m.col + m.cols - 1);
                let overlaps = mr0 <= r1 && mr1 >= r0 && mc0 <= c1 && mc1 >= c0;
                if overlaps && (mr0 < r0 || mc0 < c0 || mr1 > r1 || mc1 > c1) {
                    r0 = r0.min(mr0);
                    c0 = c0.min(mc0);
                    r1 = r1.max(mr1);
                    c1 = c1.max(mc1);
                    grown = true;
                }
            }
            if !grown {
                return (r0, c0, r1, c1);
            }
        }
    }

    pub fn model_blocks(&self, _t: ObjId, b: (u32, u32, u32, u32)) -> Vec<(u32, u32, u32, u32)> {
        vec![b]
    }

    pub fn toggle_inspector(&mut self, mode: InspMode) {
        if self.inspector && self.insp_mode == mode {
            self.inspector = false;
        } else {
            self.inspector = true;
            self.insp_mode = mode;
        }
    }

    /// Image > Replace: a new picture for the replace target.
    pub fn pick_image(&mut self) {
        let Some(id) = self.replace_target.take() else { return };
        let Some(p) = file_dialog(|d| d.add_filter("Pictures", &["png", "jpg", "jpeg"]).pick_file()) else { return };
        let bytes = match std::fs::read(&p) {
            Ok(b) => b,
            Err(e) => return self.notify(format!("Couldn't read the picture: {e}")),
        };
        let Some((w, h)) = pg_core::kn_format::objects::image_size(&bytes) else { return self.notify("Only PNG and JPEG pictures can be used.") };
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "image.png".into());
        let res = self.edit("Replace Image", |d, hist| {
            d.structural(hist, &[id], |k| -> Result<(), pg_core::kn_format::Error> {
                let data = k.add_image_data(bytes, &name)?;
                k.set_image_data(id, data, w as f32, h as f32);
                Ok(())
            })
        });
        if let Some(Err(e)) = res {
            self.notify(format!("Couldn't replace the picture: {e}"));
        }
    }

    pub fn open_set_password(&mut self) {
        self.set_password = Some((String::new(), String::new(), String::new()));
    }

    pub fn page_count(&self) -> usize {
        self.layout.as_ref().map(|l| l.page_count()).unwrap_or(1)
    }

    /// The page the caret (or the selected object) is on.
    pub fn current_page(&self) -> usize {
        if let Some(o) = self.canvas.obj.and_then(|o| self.obj_page.get(&o)) {
            return *o;
        }
        match (self.cursor, &self.layout) {
            (Some(Cursor { owner: Owner::Body, caret, .. }), Some(l)) => l.body.as_ref().map(|b| l.page_of(b.to_shown(caret))).unwrap_or(0),
            (Some(Cursor { owner: Owner::HeaderFooter { page } | Owner::Footnote { page }, .. }), _) => page,
            _ => self.canvas.page_in_view.min(self.page_count().saturating_sub(1)),
        }
    }

    fn snapshot(&mut self, ctx: &egui::Context, settled: bool) {
        let Some((path, frames)) = &mut self.snap else { return };
        *frames += 1;
        let n = *frames;
        let path = path.clone();
        if n == 3 {
            if let Some(p) = std::env::var("CS_PAGE").ok().and_then(|s| s.parse::<usize>().ok()) {
                self.canvas.reveal_page(p.saturating_sub(1));
            }
            for c in std::env::var("CS_CMD").unwrap_or_default().split(',').map(str::trim).filter(|c| !c.is_empty()) {
                crate::menus::run(self, ctx, c);
            }
        }
        for c in std::mem::take(&mut self.script_cmds) {
            if let Some(path) = c.strip_prefix('\u{4}') {
                crate::media::insert_path(self, Path::new(path.trim()));
                continue;
            }
            crate::menus::run(self, ctx, &c);
        }
        if n > 20 && settled && self.script.is_empty() && !self.hub.busy() {
            if let Some(p) = std::env::var_os("CS_SAVE") {
                self.save_to(Path::new(&p));
                eprintln!("saved to {}", Path::new(&p).display());
            }
            if !self.perf.is_empty() {
                let v: Vec<f32> = self.perf.iter().skip(15).copied().collect();
                let mut sorted = v.clone();
                sorted.sort_by(f32::total_cmp);
                let pick = |q: f32| sorted.get(((sorted.len() as f32 - 1.0) * q) as usize).copied().unwrap_or(0.0);
                eprintln!("perf: {} frames, mean {:.2} ms, p50 {:.2}, p95 {:.2}, max {:.2}", v.len(), v.iter().sum::<f32>() / v.len().max(1) as f32, pick(0.5), pick(0.95), pick(1.0));
            }
            if std::env::var_os("CS_LINES").is_some() {
                crate::canvas::dump_lines(self);
            }
            if let Some(w) = &self.wb {
                if std::env::var_os("CS_DUMP").is_some() {
                    eprintln!("body: {:?}", w.body_text());
                    eprintln!("pages: {}", self.page_count());
                    eprintln!("undo steps: {} ({:?})", self.hist.undo_len(), self.hist.undo_label());
                }
            }
            self.snap = None;
            let ctx2 = ctx.clone();
            let cb = egui::ScreenshotCallback::new(move |img: std::sync::Arc<egui::ColorImage>| {
                let [w, h] = img.size;
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                    let _ = buf.save(&path);
                    eprintln!("snapshot saved to {}", path.display());
                }
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
        } else if n > 1200 {
            eprintln!("snapshot timed out");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

fn kn_render_ps_name(family: &str) -> String {
    family.replace(' ', "")
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // fingers: one scrolls the page, a hold is a right click (the iPad's way)
        kn_ui::touch::raw_input(ctx, raw);
        if self.automated {
            // a scripted run takes only its own input (the real keyboard and pointer, if the
            // window has them, would type into the document being tested)
            raw.events.retain(|e| !matches!(e, egui::Event::Text(_) | egui::Event::Key { .. } | egui::Event::Paste(_) | egui::Event::Ime(_) | egui::Event::PointerButton { .. } | egui::Event::PointerMoved(_) | egui::Event::MouseWheel { .. }));
        }
        if self.snap.as_ref().is_some_and(|(_, n)| *n >= 12) {
            if let Some(evs) = self.script.pop_front() {
                for e in evs {
                    match e {
                        egui::Event::Text(t) if t.starts_with('\u{2}') => self.script_cmds.push(t[1..].to_string()),
                        e => raw.events.push(e),
                    }
                }
            }
        }
        crate::text::take_keys(self, raw);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let t0 = std::time::Instant::now();
        kn_ui::glass::native_blur(frame, kn_ui::glass::translucent_panes());
        let ctx = ui.ctx().clone();
        kn_ui::begin_frame(&ctx);
        kn_ui::scale::apply(&ctx, self.settings.ui_scale);
        kn_ui::form::set_pref(self.settings.ui_layout);
        if self.cursor.is_some() && !self.chooser {
            kn_ui::typing(&ctx);
        }
        fit_window_to_screen(&ctx);
        #[cfg(target_os = "macos")]
        {
            for path in winit::platform::macos::take_opened_files() {
                crate::tabs::open_in_tab(self, &path);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        let m = if kn_ui::winframe::system_title_bar() { 0.0 } else { kn_ui::winframe::margin() };
        let inner = ui.max_rect().shrink(m);
        self.corner = if m > 0.0 { kn_ui::winframe::RADIUS } else { 0.0 };
        let dark = !theme::is_light();
        if m > 0.0 {
            let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
            kn_ui::winframe::paint_shadow(&ctx, inner, self.corner, focused, dark);
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| self.frame(ui));
        if m > 0.0 {
            kn_ui::winframe::paint_outline(&ctx, inner, self.corner, dark);
        }
        self.widget_focus = ctx.memory(|m| m.focused().is_some());
        if std::env::var_os("CS_PERF").is_some() {
            self.perf.push(t0.elapsed().as_secs_f32() * 1000.0);
        }
    }
}

impl App {
    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some((path, hint, typed, wrong)) = &mut self.password {
            let mut done = None;
            egui::Window::new("Password").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
                ui.label(format!("\u{201c}{}\u{201d} is password protected.", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
                if !hint.is_empty() {
                    ui.label(format!("Hint: {hint}"));
                }
                if *wrong {
                    ui.colored_label(Color32::from_rgb(0xc0, 0x30, 0x30), "That password isn't right.");
                }
                let r = ui.add(egui::TextEdit::singleline(typed).password(true).desired_width(240.0));
                r.request_focus();
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        done = Some(false);
                    }
                    if ui.button("Open").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        done = Some(true);
                    }
                });
            });
            match done {
                Some(true) => match Pages::open_with_password(path.as_path(), typed, hint) {
                    Ok(doc) => {
                        self.password = None;
                        self.load(doc);
                    }
                    Err(_) => *wrong = true,
                },
                Some(false) => self.password = None,
                None => {}
            }
        }
        if let Some((pw, again, hint)) = &mut self.set_password {
            let mut done = None;
            let has = self.wb.as_ref().is_some_and(|w| w.doc.password.is_some());
            egui::Window::new("Set Password").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
                egui::Grid::new("pw").num_columns(2).show(ui, |ui| {
                    ui.label("Password:");
                    let r = ui.add(egui::TextEdit::singleline(pw).password(true));
                    // the first field takes the typing when the dialog opens
                    if ui.memory(|m| m.focused().is_none()) {
                        r.request_focus();
                    }
                    ui.end_row();
                    ui.label("Verify:");
                    ui.add(egui::TextEdit::singleline(again).password(true));
                    ui.end_row();
                    ui.label("Hint:");
                    ui.text_edit_singleline(hint);
                    ui.end_row();
                });
                let ok = !pw.is_empty() && pw == again;
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        done = Some(None);
                    }
                    if has && ui.button("Remove Password").clicked() {
                        done = Some(Some(None));
                    }
                    if ui.add_enabled(ok, egui::Button::new("Set Password")).clicked() || (ok && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))) {
                        done = Some(Some(Some((pw.clone(), hint.clone()))));
                    }
                });
            });
            if let Some(d) = done {
                self.set_password = None;
                if let (Some(choice), Some(w)) = (d, &mut self.wb) {
                    w.doc.password = choice;
                    w.doc.dirty = true;
                    self.notify("The password applies from the next save.");
                }
            }
        }
        crate::tabs::closing_ui(self, ctx);
        crate::tabs::all_tabs_ui(self, ctx);
        if let Some(text) = &mut self.go_to {
            let mut done = None;
            let n = self.layout.as_ref().map(|l| l.page_count()).unwrap_or(1);
            egui::Window::new("Go to Page").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
                ui.label(format!("Page (1–{n}):"));
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(120.0));
                r.request_focus();
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        done = Some(None);
                    }
                    if ui.button("Go").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        done = Some(text.trim().parse::<usize>().ok());
                    }
                });
            });
            if let Some(d) = done {
                self.go_to = None;
                if let Some(p) = d {
                    crate::menus::go_to_page(self, p.clamp(1, n) - 1);
                }
            }
        }
        if self.show_about {
            egui::Window::new("About Letters").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).open(&mut self.show_about).show(ctx, |ui| {
                ui.label(theme::bold("Letters"));
                ui.label("A word processor and page-layout app that reads and writes Pages (.pages) documents.");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("Looks and settings follow Keydeck and Values.");
            });
        }
    }

    fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.pointer_down = ctx.input(|i| i.pointer.any_down());
        self.refresh();
        if let Some((_, t)) = &mut self.message {
            *t += ctx.input(|i| i.stable_dt) as f64;
            if *t > 4.0 {
                self.message = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        // the window's close button: every tab closes, changed ones asking first
        if ctx.input(|i| i.viewport().close_requested()) && crate::tabs::on_close_requested(self) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        self.dialogs(&ctx);
        let window: egui::Rect = ui.max_rect();
        let integrated = !kn_ui::winframe::system_title_bar();
        if integrated {
            crate::chrome::window_edges(&ctx, window);
        }
        let corner = self.corner;
        let corners = |c: [bool; 4]| {
            let r = corner.round() as u8;
            let k = |b: bool| if b { r } else { 0 };
            egui::CornerRadius { nw: k(c[0]), ne: k(c[1]), sw: k(c[2]), se: k(c[3]) }
        };
        let top_y = egui::Panel::top("menubar")
            .frame(egui::Frame::new().fill(crate::look::header_bg()).corner_radius(corners([true, true, false, false])).inner_margin(egui::Margin::symmetric(6, 2)))
            .show(ui, |ui| {
                if integrated {
                    crate::chrome::title_bar(self, ui);
                } else {
                    crate::menus::menu_bar(self, ui);
                }
            })
            .response
            .rect
            .max
            .y;
        let classic = kn_ui::glass::classic();
        let tb_h = if self.show_toolbar { crate::toolbar::height(self) } else { 0.0 };
        if classic && self.show_toolbar {
            egui::Panel::top("toolbar").exact_size(tb_h).frame(egui::Frame::new().fill(crate::look::header_bg()).inner_margin(egui::Margin::symmetric(10, 4))).show(ui, |ui| {
                let bar = ui.max_rect();
                crate::toolbar::toolbar(self, ui, bar);
            });
        }
        let float_h = if classic { 0.0 } else { tb_h };
        self.welcome_top = top_y + if classic { 0.0 } else { tb_h };
        let mut right_w = 0.0;
        let mut insp_top = top_y;
        // the inspector slides in and out (Pages: 272 pt; the suite's panels are laid out for 300)
        // (the phone layout: the inspector is a sheet from the bottom; it and the sidebar are
        // closed to begin with)
        let compact = kn_ui::form::compact(&ctx);
        if kn_ui::form::switched(&ctx) == Some(true) {
            self.inspector = false;
            self.sidebar = Sidebar::None;
        }
        if compact && self.inspector {
            kn_ui::form::sheet(ui, "inspector-sheet", theme::PANEL_BG(), |ui| crate::inspector::inspector(self, ui));
        }
        let t = kn_ui::theme::slide(&ctx, egui::Id::unique("inspector-slide"), self.inspector && !compact);
        if t > 0.0 {
            const W: f32 = 300.0;
            let r = egui::Panel::right("inspector")
                .exact_size(W * t)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(if kn_ui::glass::translucent_panes() { Color32::TRANSPARENT } else { theme::PANEL_BG() }).corner_radius(corners([false, false, false, true])))
                .show(ui, |ui| {
                    if kn_ui::glass::translucent_panes() {
                        kn_ui::glass::pane_background_rounded(ui, theme::PANEL_BG(), corners([false, false, false, true]));
                    }
                    let m = ui.max_rect();
                    let full = egui::Rect::from_min_size(m.min, egui::vec2(W, m.height())).shrink2(egui::vec2(12.0, 0.0));
                    let full = egui::Rect::from_min_max(egui::pos2(full.min.x, full.min.y + 12.0 + float_h), egui::pos2(full.max.x, full.max.y - 12.0));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(full), |ui| crate::inspector::inspector(self, ui));
                })
                .response
                .rect;
            right_w = r.width();
            insp_top = r.min.y;
        }
        self.toolbar_band = None;
        self.toolbar_band_paint = None;
        if float_h > 0.0 {
            let r = egui::Panel::top("toolbar-room").exact_size(float_h).frame(egui::Frame::new().fill(crate::look::header_bg())).show(ui, |_| {}).response.rect;
            self.toolbar_band = Some(r);
        }
        let tabs_h = crate::tabs::bar_height(self);
        if tabs_h > 0.0 {
            egui::Panel::top("doc-tabs").exact_size(tabs_h).frame(egui::Frame::new().fill(crate::look::header_bg())).show(ui, |ui| crate::tabs::bar(self, ui));
        }
        // the sidebar: page thumbnails or the table of contents (160 pt)
        let st = kn_ui::theme::slide(&ctx, egui::Id::unique("sidebar-slide"), self.sidebar != Sidebar::None);
        let mut left_w = 0.0;
        if st > 0.0 {
            let r = egui::Panel::left("sidebar")
                .exact_size(180.0 * st)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(theme::SIDEBAR_BG()).corner_radius(corners([false, false, true, false])))
                .show(ui, |ui| crate::sidebar::sidebar(self, ui))
                .response
                .rect;
            left_w = r.width();
        }
        if self.find.is_some() {
            egui::Panel::top("findbar").exact_size(40.0).frame(egui::Frame::new().fill(theme::PANEL_BG()).inner_margin(egui::Margin::symmetric(12, 6))).show(ui, |ui| crate::find::find_bar(self, ui));
        }
        if self.has_annotations_bar() {
            egui::Panel::top("annotations").exact_size(30.0).frame(egui::Frame::new().fill(theme::PANEL_BG()).inner_margin(egui::Margin::symmetric(10, 3))).show(ui, |ui| crate::notes::annotation_bar(self, ui));
        }
        if self.ruler && !self.screen_view {
            egui::Panel::top("ruler").exact_size(26.0).frame(egui::Frame::new().fill(theme::PANEL_BG())).show(ui, |ui| crate::ruler::ruler(self, ui));
        }
        if self.comments_pane {
            egui::Panel::right("comments-pane").exact_size(260.0).resizable(false).frame(egui::Frame::new().fill(theme::SIDEBAR_BG()).inner_margin(10)).show(ui, |ui| crate::notes::comments_pane(self, ui));
        }
        let mut settled = true;
        // its bottom corners follow the window's rounding where no sidebar / inspector covers them
        let canvas_corners = corners([false, false, left_w <= 0.0, right_w <= 0.0 && !self.comments_pane]);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(crate::look::canvas_bg()).corner_radius(canvas_corners)).show(ui, |ui| {
            settled = crate::canvas::canvas(self, ui);
        });
        let line = egui::Stroke::new(1.0, theme::SEPARATOR());
        if right_w > 0.0 {
            ui.painter().vline(window.max.x - right_w, insp_top..=window.max.y, line);
        }
        if left_w > 0.0 {
            ui.painter().vline(window.min.x + left_w, top_y + float_h..=window.max.y, line);
        }
        // under the floating toolbar: the pages scrolled up under it, blurred (and tinted)
        if let Some((tex, r, tinted)) = self.toolbar_band_paint.take() {
            let p = ui.painter().with_clip_rect(r);
            if tinted {
                p.image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                p.rect_filled(r, 0.0, kn_ui::glass::ToolbarBackground::tint());
            } else if let Some(band) = self.toolbar_band {
                // full strength under the buttons, easing out over the page below
                let band = egui::Rect::from_min_max(egui::pos2(r.min.x, band.min.y), egui::pos2(r.max.x, band.max.y));
                kn_ui::glass::paint_feathered(&p, None, tex.id(), r, band);
            }
        }
        if float_h > 0.0 {
            let strip = egui::Rect::from_min_max(egui::pos2(window.min.x, top_y), egui::pos2(window.max.x, top_y + float_h)).shrink2(egui::vec2(10.0, 4.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(strip).id_salt("floating-toolbar"));
            crate::toolbar::toolbar(self, &mut child, strip);
        }
        crate::dialogs::windows(self, &ctx);
        crate::toolbar::sheet(self, &ctx);
        crate::toolbar::drag_overlay(self, &ctx);
        crate::spelling::window(self, &ctx);
        crate::fileops::versions_window(self, &ctx);
        crate::fonts::fonts_window(self, &ctx);
        crate::fonts::replace_window(self, &ctx);
        crate::chartedit::ui(self, &ctx);
        crate::media::ui(self, &ctx);
        if self.hub.needs_shapes() {
            (self.hub.basic, self.hub.mine) = crate::chrome::hub_shapes();
        }
        if let Some(p) = self.hub.show(&ctx) {
            crate::chrome::insert_from_hub(self, &ctx, p);
        }
        crate::condwin::ui(self, &ctx);
        crate::menus::shortcuts_window(self, &ctx);
        crate::settings::window(self, &ctx);
        if !crate::dialogs::modal_open(self) {
            crate::menus::shortcuts(self, &ctx);
        }
        let title = format!("{}{} — Letters", self.title(), if self.doc().is_some_and(|d| d.dirty) { " (Edited)" } else { "" });
        if ctx.data(|d| d.get_temp::<String>(egui::Id::unique("window-title"))) != Some(title.clone()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique("window-title"), title));
        }
        self.snapshot(&ctx, settled);
    }

    /// The comments & changes bar under the toolbar: with comments shown and any in the
    /// document, or Track Changes on.
    pub fn has_annotations_bar(&self) -> bool {
        let Some(w) = &self.wb else { return false };
        let Some(b) = w.body_storage() else { return false };
        w.tracking() || (self.show_comments && w.file().obj(b).and_then(|s| s.msg("table_highlight")).is_some_and(|t| t.msgs("entries").any(|e| e.ref_id("object").is_some())))
    }

}

pub(crate) fn file_dialog<T>(f: impl FnOnce(kn_ui::files::FileDialog) -> Option<T>) -> Option<T> {
    if std::env::var_os("CS_SCRIPT").is_some() || std::env::var_os("CS_SNAP").is_some() {
        return None;
    }
    f(kn_ui::files::FileDialog::new())
}

/// CS_SCRIPT="click 400 300; dclick 400 300; drag 100 100 300 100; type hello; key Enter; key ctrl+Z; wait"
/// (window points). Each step becomes a few frames of events.
fn parse_script(s: &str) -> std::collections::VecDeque<Vec<egui::Event>> {
    use egui::{Event, Key, Modifiers, PointerButton, Pos2};
    let mut out = std::collections::VecDeque::new();
    let button = |pos: Pos2, pressed: bool| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    for step in s.split(';').map(str::trim).filter(|x| !x.is_empty()) {
        let mut it = step.splitn(2, ' ');
        let cmd = it.next().unwrap_or("");
        let rest = it.next().unwrap_or("").trim();
        let nums: Vec<f32> = rest.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        match (cmd, nums.as_slice()) {
            ("sclick", [x, y]) => {
                let p = Pos2::new(*x, *y);
                let ev = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::SHIFT };
                out.push_back(vec![Event::PointerMoved(p), Event::ModifiersChanged(Modifiers::SHIFT)]);
                out.push_back(vec![ev(true)]);
                out.push_back(vec![ev(false)]);
                out.push_back(vec![Event::ModifiersChanged(Modifiers::NONE)]);
                out.push_back(vec![]);
            }
            ("rclick", [x, y]) => {
                let p = Pos2::new(*x, *y);
                out.push_back(vec![Event::PointerMoved(p)]);
                out.push_back(vec![Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE }]);
                out.push_back(vec![Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE }]);
                out.push_back(vec![]);
            }
            ("click", [x, y]) | ("dclick", [x, y]) | ("tclick", [x, y]) => {
                let p = Pos2::new(*x, *y);
                out.push_back(vec![Event::PointerMoved(p)]);
                let n = match cmd {
                    "dclick" => 2,
                    "tclick" => 3,
                    _ => 1,
                };
                for _ in 0..n {
                    out.push_back(vec![button(p, true)]);
                    out.push_back(vec![button(p, false)]);
                }
                out.push_back(vec![]);
            }
            ("hold", [x1, y1, x2, y2]) => {
                let (a, b) = (Pos2::new(*x1, *y1), Pos2::new(*x2, *y2));
                out.push_back(vec![Event::PointerMoved(a)]);
                out.push_back(vec![button(a, true)]);
                for i in 1..=8 {
                    out.push_back(vec![Event::PointerMoved(a + (b - a) * (i as f32 / 8.0))]);
                }
            }
            ("drag", [x1, y1, x2, y2]) => {
                let (a, b) = (Pos2::new(*x1, *y1), Pos2::new(*x2, *y2));
                out.push_back(vec![Event::PointerMoved(a)]);
                out.push_back(vec![button(a, true)]);
                for i in 1..=8 {
                    out.push_back(vec![Event::PointerMoved(a + (b - a) * (i as f32 / 8.0))]);
                }
                out.push_back(vec![button(b, false)]);
                out.push_back(vec![]);
            }
            ("cdrag", [x1, y1, x2, y2]) => {
                // a drag with Ctrl held (rotate a handle)
                let (a, b) = (Pos2::new(*x1, *y1), Pos2::new(*x2, *y2));
                let m = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
                let ev = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: m };
                out.push_back(vec![Event::PointerMoved(a), Event::ModifiersChanged(m)]);
                out.push_back(vec![ev(a, true)]);
                for i in 1..=8 {
                    out.push_back(vec![Event::PointerMoved(a + (b - a) * (i as f32 / 8.0))]);
                }
                out.push_back(vec![ev(b, false)]);
                out.push_back(vec![Event::ModifiersChanged(Modifiers::NONE)]);
            }
            ("move", [x, y]) => {
                out.push_back(vec![Event::PointerMoved(Pos2::new(*x, *y))]);
                out.push_back(vec![]);
            }
            ("type", _) => {
                out.push_back(vec![Event::Text(rest.replace("\\s", " "))]);
                out.push_back(vec![]);
            }
            ("key", _) => {
                let mut modifiers = Modifiers::NONE;
                let mut name = rest;
                loop {
                    if let Some(k) = name.strip_prefix("ctrl+") {
                        modifiers.ctrl = true;
                        modifiers.command = true;
                        name = k;
                    } else if let Some(k) = name.strip_prefix("alt+") {
                        modifiers.alt = true;
                        name = k;
                    } else if let Some(k) = name.strip_prefix("shift+") {
                        modifiers.shift = true;
                        name = k;
                    } else {
                        break;
                    }
                }
                if let Some(key) = Key::from_name(name) {
                    let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
                    out.push_back(vec![Event::ModifiersChanged(modifiers), ev(true)]);
                    out.push_back(vec![ev(false), Event::ModifiersChanged(Modifiers::NONE)]);
                    out.push_back(vec![]);
                }
            }
            ("scroll", [x, y, dx, dy, n]) => {
                out.push_back(vec![Event::PointerMoved(Pos2::new(*x, *y))]);
                for _ in 0..*n as usize {
                    out.push_back(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(*dx, *dy), phase: egui::TouchPhase::Move, modifiers: Modifiers::NONE, source: egui::MouseWheelSource::Wheel }]);
                }
            }
            ("pinch", [x, y, f, n]) => {
                out.push_back(vec![Event::PointerMoved(Pos2::new(*x, *y))]);
                for _ in 0..*n as usize {
                    out.push_back(vec![Event::Zoom(*f)]);
                }
            }
            ("menu", _) => out.push_back(vec![Event::Text(format!("\u{2}{rest}"))]),
            ("image", _) => out.push_back(vec![Event::Text(format!("\u{2}\u{4}{rest}"))]),
            ("wait", _) => {
                for _ in 0..10 {
                    out.push_back(vec![]);
                }
            }
            _ => eprintln!("CS_SCRIPT: can't read step {step:?}"),
        }
    }
    out
}

static RECENT: std::sync::Mutex<Option<Vec<PathBuf>>> = std::sync::Mutex::new(None);

fn recent_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("letters").join("recent.txt"))
}

/// Once: the first window no larger than the screen allows (Keydeck's).
fn fit_window_to_screen(ctx: &egui::Context) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.load(std::sync::atomic::Ordering::Relaxed) || std::env::var_os("CS_SNAP").is_some() {
        return;
    }
    let (monitor, outer) = ctx.input(|i| (i.viewport().monitor_size, i.viewport().outer_rect));
    let (Some(m), Some(o)) = (monitor, outer) else { return };
    DONE.store(true, std::sync::atomic::Ordering::Relaxed);
    let (max_w, max_h) = (m.x * 0.92, m.y * 0.86);
    let size = egui::vec2(o.width().min(max_w), o.height().min(max_h));
    if size != o.size() {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size - (o.size() - ctx.content_rect().size()).max(egui::Vec2::ZERO)));
    }
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        let pos = egui::pos2(((m.x - size.x) / 2.0).max(0.0), ((m.y - size.y) / 2.0 + m.y * 0.02).max(25.0));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    }
}

#[allow(dead_code)]
fn unused(_r: RichText) {}
