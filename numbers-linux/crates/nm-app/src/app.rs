//! The window: toolbar, sheet tabs, the sheet canvas, the inspector and the cell bar, laid out
//! like Numbers 15.4 (research screenshots N-insp-*, N-category-by-group).

use std::path::{Path, PathBuf};

use egui::{Color32, RichText};
use nm_core::kn_format::ObjId;
use nm_core::kn_model::edit::History;
use nm_core::kn_model::{Document, Drawable, DrawableKind};
use nm_core::{Sheet, Workbook};

use crate::canvas::Canvas;

use kn_ui::theme;

/// The highlight colour (selections, the open sheet's tab): Keydeck's colour theme.
pub fn accent() -> Color32 {
    theme::ACCENT()
}

/// A light wash of the highlight colour (selected cells' fill).
pub fn accent_light() -> Color32 {
    let a = theme::ACCENT();
    let mix = |v: u8| (f32::from(v) * 0.18 + 255.0 * 0.82) as u8;
    Color32::from_rgb(mix(a.r()), mix(a.g()), mix(a.b()))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InspTab {
    Table,
    Cell,
    Text,
    Arrange,
    /// Organize: sort, filter, categories.
    Organize,
}

/// Selected cells of one table: anchor and cursor (inclusive corners).
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
    /// (row0, col0, row1, col1), inclusive.
    pub fn block(&self) -> (u32, u32, u32, u32) {
        (self.anchor.0.min(self.cursor.0), self.anchor.1.min(self.cursor.1), self.anchor.0.max(self.cursor.0), self.anchor.1.max(self.cursor.1))
    }
}

pub struct App {
    pub wb: Option<Workbook>,
    pub hist: History,
    pub sheet: usize,
    /// Bumped on every change to the document (re-reads drawables, re-renders tiles).
    pub version: u64,
    pub drawables: Vec<Drawable>,
    /// Cell comments of the sheet's tables: table -> (model row, column) -> (text, author).
    pub comments: std::collections::HashMap<ObjId, std::collections::HashMap<(u32, u32), (String, String)>>,
    /// Categorized tables: shown rows -> model rows.
    pub views: std::collections::HashMap<ObjId, nm_core::categories::View>,
    drawables_for: (u64, usize),
    pub canvas: Canvas,
    pub sel: Option<Sel>,
    pub inspector: bool,
    pub insp_tab: InspTab,
    pub message: Option<(String, f64)>,
    pub renaming: Option<(usize, String)>,
    snap: Option<(PathBuf, u32)>,
    pending_close: bool,
    /// NM_SCRIPT: synthetic input for automated checks, one batch of events per frame.
    script: std::collections::VecDeque<Vec<egui::Event>>,
    /// Started for an automated check (NM_SNAP): never ask before closing.
    automated: bool,
    /// Edit > Find: the query, the matches (sheet, table, shown row, column) and the current one.
    pub find: Option<(String, Vec<(usize, ObjId, u32, u32)>, usize)>,
    /// Find & Replace: the replacement text (None: the bar shows Find only).
    pub replace: Option<String>,
    /// A password-protected file waiting for its password: (path, hint, typed, wrong).
    password: Option<(PathBuf, String, String, bool)>,
    /// Add Comment: (table, model row, column, text, author).
    pub comment_dialog: Option<(ObjId, u32, u32, String, String)>,
    /// File > Set Password: (password, again, hint).
    set_password: Option<(String, String, String)>,
    /// The formula text last looked up: (version, table, model row, column) -> text.
    formula_cache: Option<((u64, ObjId, u32, u32), Option<String>)>,
    /// NM_PERF: time spent building each frame (ms).
    perf: Vec<f32>,
    /// The desktop's window buttons (left / right of the title bar).
    pub buttons: kn_ui::winframe::ButtonLayout,
    pub menu_bar_width: Option<f32>,
    pub title_press_at: f64,
    pub show_comments: bool,
    /// View > Show Rulers; View > Guides > Hide Guides.
    pub show_rulers: bool,
    /// Draw With Pen: the stroke being drawn (sheet points), its colour and width.
    pub pen: Option<Vec<egui::Pos2>>,
    pub pen_color: egui::Color32,
    pub pen_width: f32,
    /// Format > Advanced > Set as Default Text Box Appearance: the text box new ones copy
    /// (a copy of the document it was in, and its id there).
    pub default_textbox: Option<(nm_core::kn_format::KeyFile, ObjId)>,
    /// File > New's template chooser and Save as Template….
    pub templates: crate::templates::Templates,
    /// File > Advanced > Language & Region…: (language, region) being chosen.
    pub language_edit: Option<(String, String)>,
    /// File > Print…'s Print Setup window.
    pub print_setup_open: bool,
    /// Insert > Equation….
    pub equation: crate::equation::EquationDialog,
    /// Media: browsers, recording, web video, galleries, playback.
    pub media: crate::media::MediaBrowser,
    /// Insert > Browse Content Hub: openly licensed pictures from Wikimedia Commons.
    pub hub: kn_ui::contenthub::Hub,
    /// The image each gallery shows (its arrows / dots).
    pub gallery_pos: std::collections::HashMap<ObjId, usize>,
    /// Pivot tables made here: pivot table -> the table it summarizes and its fields.
    pub pivots: std::collections::HashMap<ObjId, crate::pivotui::PivotOf>,
    pub show_guides: bool,
    pub show_toolbar: bool,
    pub show_about: bool,
    /// Format > Add Link's popover (an object's link or linked text).
    pub link_edit: Option<crate::links::LinkEdit>,
    /// Insert > Formula > Stock Quote, and quotes being fetched.
    pub stocks: crate::stocks::StockQuote,
    /// The document name's popover in the toolbar: the name being typed and the folder.
    pub doc_title: Option<(String, Option<PathBuf>)>,
    /// An opened Excel / CSV file's name (Numbers names the new spreadsheet after it).
    pub untitled_name: Option<String>,
    /// Excel mode: the document is the .xlsx it was opened from, and Save writes it back.
    pub excel: Option<ExcelMode>,
    /// The toolbar's customizing state (View > Customize Toolbar…).
    pub tb: crate::toolbar::TbState,
    /// Editing a shape's points (Make Editable).
    pub points: crate::points::PointEdit,
    /// View > Show Colors: the colour window (its colour).
    pub show_colors: bool,
    pub colors_pick: egui::Color32,
    /// The Colors window's mode, the colour being edited and the saved colours (kn-ui's).
    pub color_state: kn_ui::colors::ColorPanelState,
    /// Edit > Spelling and Grammar's window.
    pub spelling: Option<crate::spelling::Spelling>,
    /// File > Revert To > Browse All Versions.
    pub show_versions: bool,
    /// Format > Font > Show Fonts (and its search box), Replace Fonts….
    pub show_fonts: bool,
    pub font_query: String,
    pub replace_fonts: Option<crate::fonts::ReplaceFonts>,
    /// View > Show Warnings: the open window's list.
    pub warnings: Option<Vec<String>>,
    /// Format > Chart > Edit Chart Data….
    pub chart_edit: Option<crate::chartedit::ChartEdit>,
    /// Edit Data References of a chart linked to cells.
    pub data_refs: Option<crate::datarefs::DataRefs>,
    pub show_shortcuts: bool,
    /// Copied objects (Edit > Copy with objects selected, Duplicate).
    pub clip: Option<nm_core::sheets::Clip>,
    /// The last copy was of objects (else of cells, on the system clipboard).
    pub obj_clip_active: bool,
    /// Rounded window corners (our own frame).
    corner: f32,
    /// Object inspector state (Keydeck's): the description being typed, the font list, the
    /// picture being replaced, the picture whose mask is edited, the Format tab.
    pub desc_buf: Option<(ObjId, String)>,
    pub font_names: Option<Vec<String>>,
    pub replace_target: Option<ObjId>,
    pub mask_edit: Option<ObjId>,
    pub obj_tab: usize,
    /// Format > Copy Style.
    pub copied_style: Option<crate::editing::CopiedStyle>,
    /// Copied table cells (values, formulas, formats, styles, merges).
    pub cell_clip: Option<crate::clip::CellClip>,
    /// Organize: each table's sort rules (column, ascending) and whether only selected rows sort,
    /// its filters and its categories (Values keeps the rules while the document is open).
    pub sorts: std::collections::HashMap<ObjId, (Vec<(u32, bool)>, bool)>,
    pub filters: std::collections::HashMap<ObjId, nm_core::organize::Filters>,
    pub cats: std::collections::HashMap<ObjId, crate::organize::Cats>,
    /// The group rows of each table categorized here (by view row).
    pub cat_groups: std::collections::HashMap<ObjId, std::collections::HashMap<usize, crate::organize::GroupRow>>,
    /// The next edit continues the last one when it has the same label (one undo step).
    pub merge_next_edit: bool,
    /// The mouse button is held this frame: edits repeated while dragging (a colour in the
    /// Colors window, a slider, a stepper) make one undo step, not one per frame.
    pointer_held: bool,
    /// NM_SCRIPT "menu <command>" steps waiting to run.
    script_cmds: Vec<String>,
    /// Keydeck's look (shared) and Values' own settings; Values > Settings… shows them.
    pub look: crate::look::Look,
    pub settings: crate::settings::Settings,
    pub show_settings: bool,
    /// The document's saved window state (see [`Self::load`]).
    pub saved_views: Option<nm_core::UiState>,
    /// Where each undo / redo step was made: the sheet and selection just before it (Numbers
    /// goes back there on undo, Mac N21), and last frame's, to record the next step with.
    undo_where: Vec<(usize, Option<Sel>)>,
    redo_where: Vec<(usize, Option<Sel>)>,
    prev_where: (usize, Option<Sel>),
    /// Each sheet's zoom and top-left as last shown this session (switching sheets keeps them).
    pub session_views: std::collections::HashMap<ObjId, (f32, egui::Pos2)>,
}

/// The sheet being shown (the ported inspector's "current slide").
pub struct CurSheet {
    pub slide: ObjId,
}

impl App {
    pub fn new(cc: &eframe::CreationContext, file: Option<PathBuf>) -> App {
        // Keydeck's look and settings (colour theme, appearance, glass)
        let look = crate::look::Look::load();
        look.apply(&cc.egui_ctx);
        // Ctrl+Plus / Minus zoom the sheet, not the whole interface (egui's own keyboard zoom off)
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let mut app = App {
            wb: None,
            hist: History::default(),
            sheet: 0,
            version: 1,
            drawables: Vec::new(),
            views: Default::default(),
            comments: Default::default(),
            drawables_for: (0, usize::MAX),
            canvas: Canvas::default(),
            sel: None,
            inspector: true,
            insp_tab: InspTab::Table,
            message: None,
            renaming: None,
            snap: std::env::var_os("NM_SNAP").map(|p| (PathBuf::from(p), 0)),
            pending_close: false,
            script: std::env::var("NM_SCRIPT").map(|s| parse_script(&s)).unwrap_or_default(),
            automated: std::env::var_os("NM_SNAP").is_some(),
            find: None,
            replace: None,
            formula_cache: None,
            password: None,
            set_password: None,
            comment_dialog: None,
            perf: Vec::new(),
            buttons: kn_ui::winframe::ButtonLayout::from_desktop(),
            menu_bar_width: None,
            title_press_at: -1.0,
            show_comments: true,
            show_rulers: false,
            pen: None,
            pen_color: egui::Color32::BLACK,
            pen_width: 2.0,
            default_textbox: None,
            templates: Default::default(),
            language_edit: None,
            print_setup_open: false,
            equation: Default::default(),
            media: Default::default(),
            hub: Default::default(),
            gallery_pos: Default::default(),
            pivots: Default::default(),
            show_guides: true,
            show_toolbar: true,
            show_about: false,
            link_edit: None,
            stocks: Default::default(),
            doc_title: None,
            untitled_name: None,
            excel: None,
            tb: Default::default(),
            points: Default::default(),
            show_colors: false,
            colors_pick: egui::Color32::from_rgb(0x33, 0xd1, 0x7a),
            color_state: Default::default(),
            spelling: None,
            show_versions: false,
            show_fonts: false,
            font_query: String::new(),
            replace_fonts: None,
            warnings: None,
            chart_edit: None,
            data_refs: None,
            show_shortcuts: false,
            clip: None,
            obj_clip_active: false,
            corner: 0.0,
            desc_buf: None,
            font_names: None,
            replace_target: None,
            mask_edit: None,
            obj_tab: 0,
            copied_style: None,
            merge_next_edit: false,
            pointer_held: false,
            cell_clip: None,
            sorts: Default::default(),
            filters: Default::default(),
            cats: Default::default(),
            cat_groups: Default::default(),
            script_cmds: Vec::new(),
            look,
            settings: crate::settings::Settings::load(),
            show_settings: false,
            saved_views: None,
            undo_where: Vec::new(),
            redo_where: Vec::new(),
            prev_where: (0, None),
            session_views: Default::default(),
        };
        match file {
            Some(p) => app.open(&p),
            None => {
                app.new_document();
                // a new window starts on the start screen (recent spreadsheets, templates)
                app.templates.chooser = !app.automated || std::env::var_os("NM_WELCOME").is_some();
            }
        }
        if let Some(n) = std::env::var("NM_SHEET").ok().and_then(|s| s.parse::<usize>().ok()) {
            app.sheet = n.saturating_sub(1);
        }
        if let Some(z) = std::env::var("NM_ZOOM").ok().and_then(|s| s.parse::<f32>().ok()) {
            app.canvas.zoom = z.clamp(0.25, 4.0);
        }
        if let Some(t) = std::env::var("NM_INSPECTOR").ok() {
            app.insp_tab = match t.as_str() {
                "cell" => InspTab::Cell,
                "text" => InspTab::Text,
                "arrange" => InspTab::Arrange,
                _ => InspTab::Table,
            };
        }
        app
    }

    pub fn doc(&self) -> Option<&Document> {
        self.wb.as_ref().map(|w| &w.doc)
    }

    pub fn notify(&mut self, s: impl Into<String>) {
        let s = s.into();
        if std::env::var_os("NM_DEBUG").is_some() {
            eprintln!("notify: {s}");
        }
        self.message = Some((s.into(), 0.0));
    }

    pub fn changed(&mut self) {
        self.version += 1;
        if let Some(w) = &mut self.wb {
            w.doc.dirty = true;
        }
    }

    /// File > Open Recent (most recent first, kept in ~/.config/numdeck/recent.txt).
    pub fn recent(&self) -> Vec<PathBuf> {
        recent_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| s.lines().map(PathBuf::from).filter(|p| p.exists()).collect()).unwrap_or_default()
    }

    pub fn remember(&self, p: &Path) {
        // automated checks leave the user's list alone
        if self.automated {
            return;
        }
        let Some(file) = recent_path() else { return };
        let mut list = self.recent();
        list.retain(|x| x != p);
        list.insert(0, p.to_path_buf());
        list.truncate(10);
        let _ = std::fs::create_dir_all(file.parent().unwrap());
        let _ = std::fs::write(file, list.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n"));
    }

    pub(crate) fn load(&mut self, wb: Workbook) {
        // the window as the document was saved: its open sheet, the inspector, each sheet's
        // zoom and scroll position (Numbers restores them, Mac N15)
        self.saved_views = nm_core::ui_state(wb.file());
        self.session_views.clear();
        let active = self.saved_views.as_ref().map(|u| u.active_sheet).unwrap_or(0);
        if let Some(i) = self.saved_views.as_ref().and_then(|u| u.inspector) {
            // NM_INSPECTOR (checks) always shows it
            // (the phone layout: documents open with the inspector closed)
            self.inspector = (i || std::env::var_os("NM_INSPECTOR").is_some()) && !kn_ui::form::was_compact();
        }
        // Numbers' own pivot tables: their fields, to show and refresh (Mac N29-C1)
        let apple_epoch = std::time::UNIX_EPOCH + std::time::Duration::from_secs(978_307_200);
        self.pivots = nm_core::pivot::native_pivots(wb.file())
            .into_iter()
            .map(|(pivot, source, spec, ts)| (pivot, crate::pivotui::PivotOf { source, spec, refreshed: ts.map(|t| apple_epoch + std::time::Duration::from_secs_f64(t.max(0.0))), native: true }))
            .collect();
        // (another document's rules are for its tables)
        self.sorts.clear();
        self.filters.clear();
        self.cats.clear();
        self.wb = Some(wb);
        // (another document: none of the tiles drawn so far are of it)
        self.canvas.tiles.clear();
        self.hist = History::default();
        self.sheet = active.min(self.sheets().len().saturating_sub(1));
        self.sel = None;
        self.version += 1;
        self.canvas.reset();
    }

    pub fn new_document(&mut self) {
        self.new_document_with(|_| {});
    }

    /// A new spreadsheet from Blank, changed by `shape` first (the start screen's templates).
    pub fn new_document_with(&mut self, shape: impl FnOnce(&mut Document)) {
        match nm_core::kn_format::KeyFile::from_zip(nm_core::BLANK) {
            Ok(k) => {
                let mut doc = Document::from_file(k);
                shape(&mut doc);
                self.load(Workbook { doc });
                self.untitled_name = None;
                self.excel = None;
                self.apply_rotations();
            }
            Err(e) => self.notify(format!("Couldn't make a new spreadsheet: {e}")),
        }
    }

    pub fn open(&mut self, p: &Path) {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        // a template opens as a new, untitled spreadsheet
        if ext == "nmbtemplate" {
            crate::templates::new_from(self, p);
            return;
        }
        // an Excel workbook stays one (Excel mode): from its Values copy if it has one
        let mut excel = None;
        let mut note = None;
        let opened = match ext.as_str() {
            "csv" | "tsv" | "txt" => nm_core::import::import_delimited(p),
            "xlsx" | "xlsm" => nm_core::xlsx::open_xlsx(p).map(|o| {
                excel = Some(ExcelMode { path: p.to_path_buf(), compat: o.compat, extras: o.extras.clone(), by_sheet: Default::default() });
                if o.changed_elsewhere {
                    note = Some("This workbook was changed in another app since Values saved it: it opened from the Excel data, and the Values-only extras it had are gone.".to_string());
                }
                o.workbook
            }),
            _ => Workbook::open(p),
        };
        match opened {
            Ok(wb) => {
                self.remember(p);
                let imported = wb.doc.path.is_none();
                self.load(wb);
                self.untitled_name = if imported { p.file_stem().map(|n| n.to_string_lossy().into_owned()) } else { None };
                self.excel = excel;
                self.apply_rotations();
                if let Some(n) = note {
                    self.notify(n);
                }
                let w = nm_core::import::take_warnings();
                if !w.is_empty() {
                    self.notify(w.join("; "));
                }
            }
            Err(nm_core::kn_format::Error::NeedsPassword(hint)) => self.password = Some((p.to_path_buf(), hint, String::new(), false)),
            Err(e) => self.notify(format!("Couldn't open {}: {e}", p.display())),
        }
    }

    pub fn open_dialog(&mut self) {
        if let Some(p) = file_dialog(|d| d.add_filter("Spreadsheets", &["numbers", "xlsx", "csv", "tsv", "txt"]).add_filter("Numbers spreadsheet", &["numbers"]).add_filter("Excel", &["xlsx", "xlsm"]).add_filter("CSV / TSV", &["csv", "tsv", "txt"]).pick_file()) {
            self.open(&p);
        }
    }

    pub fn save(&mut self, save_as: bool) {
        // Excel mode: the workbook is written back as Excel (with the Values copy in
        // compatibility mode); Save As can still make a Numbers spreadsheet of it
        if let (Some(x), false) = (&self.excel, save_as) {
            let (mut path, compat) = (x.path.clone(), x.compat);
            // a new workbook: where to put it
            if path.as_os_str().is_empty() {
                let Some(p) = file_dialog(|d| d.add_filter("Excel workbook", &["xlsx"]).set_file_name("Untitled.xlsx").save_file()) else { return };
                path = p;
            }
            self.save_excel(&path, compat);
            return;
        }
        if save_as {
            let stem = self.title().trim_end_matches(".numbers").trim_end_matches(".xlsx").to_string();
            let Some(p) = file_dialog(|d| d.add_filter("Numbers spreadsheet", &["numbers"]).add_filter("Excel workbook", &["xlsx"]).set_file_name(format!("{stem}.numbers")).save_file()) else { return };
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xlsx")) {
                let compat = self.excel.as_ref().is_some_and(|x| x.compat);
                self.save_excel(&p, compat);
                return;
            }
            self.save_numbers_to(&p);
            return;
        }
        let Some(wb) = &mut self.wb else { return };
        let path = match (&wb.doc.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match file_dialog(|d| d.add_filter("Numbers spreadsheet", &["numbers"]).set_file_name(format!("{}.numbers", self.untitled_name.as_deref().unwrap_or("Untitled"))).save_file()) {
                Some(p) => p,
                None => return,
            },
        };
        // the file as it was is kept (Revert To > Browse All Versions)
        if wb.doc.path.as_ref() == Some(&path) {
            crate::fileops::keep_version(&path);
        }
        match wb.doc.save_as(&path) {
            Ok(()) => {
                self.remember(&path);
                self.notify(format!("Saved {}", path.display()));
            }
            Err(e) => self.notify(format!("Couldn't save: {e}")),
        }
    }

    /// Save as a Numbers spreadsheet at `path` (leaving Excel mode: the document is a Numbers
    /// one from now on).
    pub fn save_numbers_to(&mut self, path: &Path) {
        crate::canvas::commit(self);
        let Some(wb) = &mut self.wb else { return };
        match wb.doc.save_as(path) {
            Ok(()) => {
                self.excel = None;
                self.untitled_name = None;
                self.remember(path);
                self.notify(format!("Saved {}", path.display()));
            }
            Err(e) => self.notify(format!("Couldn't save: {e}")),
        }
    }

    /// Write the workbook as Excel at `path` (Excel mode from now on); the file as it was is
    /// kept among the versions.
    pub fn save_excel(&mut self, path: &Path, compat: bool) {
        crate::canvas::commit(self);
        if path.exists() {
            crate::fileops::keep_version(path);
        }
        let Some(wb) = &mut self.wb else { return };
        // the extras under each sheet's name now (renamed sheets keep theirs)
        let by_sheet = self.excel.as_ref().map(|x| x.by_sheet.clone()).unwrap_or_default();
        let extras: nm_core::xlsx::Extras = wb.sheets().into_iter().filter_map(|s| Some((s.name.clone(), by_sheet.get(&s.id)?.clone()))).collect();
        match nm_core::xlsx::write_xlsx_full(wb, path, compat, &extras) {
            Ok(_) => {
                wb.doc.dirty = false;
                self.excel = Some(ExcelMode { path: path.to_path_buf(), compat, extras, by_sheet });
                self.untitled_name = path.file_stem().map(|n| n.to_string_lossy().into_owned());
                self.remember(path);
                self.notify(format!("Saved {}", path.display()));
            }
            Err(e) => self.notify(format!("Couldn't save the Excel workbook: {e}")),
        }
    }

    /// An Excel workbook's cell extras, to the tables they are in (each sheet's one table).
    pub fn apply_rotations(&mut self) {
        let mut by_table = std::collections::HashMap::new();
        if let (Some(x), Some(w)) = (&mut self.excel, &self.wb) {
            // matched to the sheets by name once, as the file named them
            if x.by_sheet.is_empty() {
                x.by_sheet = w.sheets().into_iter().filter_map(|s| Some((s.id, x.extras.get(&s.name)?.clone()))).collect();
            }
            for s in w.sheets() {
                if let (Some(m), Some(t)) = (x.by_sheet.get(&s.id), w.tables(&s).first()) {
                    by_table.insert(t.info, m.clone());
                }
            }
        }
        self.canvas.tiles.extras = by_table;
    }

    /// Plain Excel mode: only what an Excel workbook can hold can be added.
    pub fn excel_only(&self) -> bool {
        self.excel.as_ref().is_some_and(|x| !x.compat)
    }

    pub fn sheets(&self) -> Vec<Sheet> {
        self.wb.as_ref().map(|w| w.sheets()).unwrap_or_default()
    }

    /// The current sheet's drawables, rebuilt when the document or the sheet changed.
    pub(crate) fn refresh(&mut self) {
        if self.drawables_for == (self.version, self.sheet) {
            return;
        }
        let sheets = self.sheets();
        if self.sheet >= sheets.len() {
            self.sheet = sheets.len().saturating_sub(1);
        }
        (self.drawables, self.views) = match (&self.wb, sheets.get(self.sheet)) {
            (Some(w), Some(s)) => w.drawables_and_views(s),
            _ => Default::default(),
        };
        self.drawables_for = (self.version, self.sheet);
        // Numbers' pivot tables: shown with their totals, computed from their source
        for (pt, p) in self.pivots.iter().filter(|(_, p)| p.native) {
            let Some(src) = self.wb.as_ref().and_then(|w| w.table_data(p.source)) else { continue };
            let pv = nm_core::pivot::build(&src, &p.spec);
            if let Some(DrawableKind::Table(t)) = self.drawables.iter_mut().find(|d| d.id == *pt).map(|d| &mut d.kind) {
                nm_core::pivot::show_in(t, &pv, &p.spec);
            }
        }
        // rows that fit their content grow to their wrapped text (Numbers)
        if self.wb.is_some() {
            for d in &mut self.drawables {
                if let DrawableKind::Table(t) = &mut d.kind {
                    // the cell being typed in fits the text as typed (the row grows as you type)
                    if let Some(e) = self.canvas.edit.as_ref().filter(|e| e.table == d.id && !e.text.trim_start().starts_with('=')) {
                        if let Some(c) = t.cells.get_mut((e.row * t.cols + e.col) as usize) {
                            // a new empty last line counts as a line
                            *c = if e.text.ends_with('\n') { format!("{} ", e.text) } else { e.text.clone() };
                        }
                    }
                    let view = self.views.get(&d.id).map(|v| v.rows.as_slice());
                    self.canvas.tiles.fit_rows(t, view);
                }
            }
        }
        // galleries show the image picked with their arrows
        for d in &mut self.drawables {
            if let (DrawableKind::Image(im), Some(p)) = (&mut d.kind, self.gallery_pos.get(&d.id)) {
                im.gallery_shown = (*p).min(im.gallery.len().saturating_sub(1));
            }
        }
        // Organize > Categories made here: the table shown in groups
        self.cat_groups.clear();
        if let Some(w) = &self.wb {
            for d in &mut self.drawables {
                let Some(spec) = self.cats.get(&d.id).filter(|c| c.on && !c.levels.is_empty()) else { continue };
                if let (DrawableKind::Table(t), Some(fresh)) = (&mut d.kind, w.table_data(d.id)) {
                    *t = fresh;
                    let (view, groups) = crate::organize::categorize(t, spec);
                    self.views.insert(d.id, view);
                    self.cat_groups.insert(d.id, groups);
                }
            }
        }
        self.canvas.tiles.resolve_frames(&self.drawables);
        self.comments = match &self.wb {
            Some(w) => self
                .drawables
                .iter()
                .filter(|d| matches!(d.kind, DrawableKind::Table(_)))
                .map(|d| (d.id, nm_core::cell_comments(w.file(), d.id)))
                .filter(|(_, c)| !c.is_empty())
                .collect(),
            None => Default::default(),
        };
        // a selection in a table that is gone (undo, other sheet) is dropped; one past the end is clamped
        if let Some(sel) = &mut self.sel {
            match self.drawables.iter().find(|d| d.id == sel.table).map(|d| &d.kind) {
                Some(DrawableKind::Table(t)) if t.rows > 0 && t.cols > 0 => {
                    for p in [&mut sel.anchor, &mut sel.cursor] {
                        p.0 = p.0.min(t.rows - 1);
                        p.1 = p.1.min(t.cols - 1);
                    }
                }
                _ => self.sel = None,
            }
        }
    }

    pub fn table(&self, id: ObjId) -> Option<(&Drawable, &nm_core::kn_model::TableData)> {
        self.drawables.iter().find(|d| d.id == id).and_then(|d| match &d.kind {
            DrawableKind::Table(t) => Some((d, t)),
            _ => None,
        })
    }

    /// Tab moves between cells while a table has the keyboard (egui would move between widgets).
    fn take_tab(&mut self, raw: &mut egui::RawInput) {
        let dialog = self.password.is_some() || self.set_password.is_some() || self.comment_dialog.is_some() || self.find.is_some() || self.renaming.is_some();
        if (self.sel.is_none() && self.canvas.obj.is_none()) || self.canvas.editing.is_some() || dialog {
            return;
        }
        let mut tab = None;
        raw.events.retain(|e| match e {
            egui::Event::Key { key: egui::Key::Tab, pressed, modifiers, .. } => {
                if *pressed {
                    tab = Some(modifiers.shift);
                }
                false
            }
            _ => true,
        });
        if tab.is_some() {
            self.canvas.tab = tab;
        }
    }

    /// A cell's formula as text: Keydeck's reader, else nm-core's (other tables, sheets, any
    /// function); None for a plain value.
    pub fn formula_of(&mut self, t: ObjId, row: u32, col: u32) -> Option<String> {
        let key = (self.version, t, row, col);
        if let Some((k, v)) = &self.formula_cache {
            if *k == key {
                return v.clone();
            }
        }
        let v = self.doc().and_then(|d| d.formula_text(t, row, col).or_else(|| nm_core::formula::formula_text(&d.file, t, row, col)));
        self.formula_cache = Some((key, v.clone()));
        v
    }

    /// The model row of a shown row (None for a category's group row).
    pub fn mrow(&self, t: ObjId, v: u32) -> Option<u32> {
        match self.views.get(&t) {
            Some(view) => view.rows.get(v as usize).copied().flatten(),
            None => Some(v),
        }
    }

    /// The shown row of a model row.
    pub fn vrow(&self, t: ObjId, m: u32) -> u32 {
        self.views.get(&t).and_then(|v| v.view_row(m)).unwrap_or(m)
    }

    pub fn categorized(&self, t: ObjId) -> bool {
        self.views.contains_key(&t)
    }

    /// A shown block as blocks of model rows (one per run of consecutive model rows).
    /// A block of shown cells grown to take in every merged cell it touches (Numbers selects
    /// merges whole).
    pub fn with_merges(&self, t: ObjId, b: (u32, u32, u32, u32)) -> (u32, u32, u32, u32) {
        let Some((_, td)) = self.table(t) else { return b };
        // a whole row or column stays as it is (Numbers, Mac N16)
        if (b.0 == 0 && b.2 + 1 >= td.rows) || (b.1 == 0 && b.3 + 1 >= td.cols) {
            return b;
        }
        let (mut r0, mut c0, mut r1, mut c1) = b;
        loop {
            let before = (r0, c0, r1, c1);
            for m in &td.merges {
                let (mr1, mc1) = (m.row + m.rows - 1, m.col + m.cols - 1);
                if m.row <= r1 && mr1 >= r0 && m.col <= c1 && mc1 >= c0 {
                    r0 = r0.min(m.row);
                    c0 = c0.min(m.col);
                    r1 = r1.max(mr1);
                    c1 = c1.max(mc1);
                }
            }
            if (r0, c0, r1, c1) == before {
                return before;
            }
        }
    }

    /// The merge's top-left cell for a cell inside a merge (where its content lives).
    pub fn merge_origin(&self, t: ObjId, r: u32, c: u32) -> (u32, u32) {
        self.table(t).and_then(|(_, td)| td.merge_at(r, c)).map(|m| (m.row, m.col)).unwrap_or((r, c))
    }

    pub fn model_blocks(&self, t: ObjId, b: (u32, u32, u32, u32)) -> Vec<(u32, u32, u32, u32)> {
        let b = self.with_merges(t, b);
        let mut rows: Vec<u32> = (b.0..=b.2).filter_map(|v| self.mrow(t, v)).collect();
        rows.sort_unstable();
        let mut out: Vec<(u32, u32, u32, u32)> = Vec::new();
        for r in rows {
            match out.last_mut() {
                Some(last) if last.2 + 1 == r => last.2 = r,
                _ => out.push((r, b.1, r, b.3)),
            }
        }
        out
    }

    /// Run an undoable edit on the document.
    pub fn edit<R>(&mut self, label: &str, f: impl FnOnce(&mut Document, &mut History) -> R) -> Option<R> {
        let wb = self.wb.as_mut()?;
        if self.merge_next_edit || self.pointer_held {
            wb.doc.begin_merge(&mut self.hist, label);
        } else {
            wb.doc.begin(&mut self.hist, label);
        }
        let r = f(&mut wb.doc, &mut self.hist);
        // formulas reading other tables follow (Keydeck's recalculation is per table)
        nm_core::formula::recalc_cross(&mut wb.doc, &mut self.hist);
        // charts linked to cells follow them (Numbers)
        nm_core::chartlink::refresh(&mut wb.doc, &mut self.hist);
        // filters follow the edit (Numbers re-filters as values change)
        for (t, fl) in &self.filters {
            crate::organize::apply_filters(&mut wb.doc, &mut self.hist, *t, fl);
        }
        wb.doc.end(&mut self.hist);
        // text being edited in place: its typing carries on as its own undo step
        if self.canvas.editing.is_some() {
            wb.doc.begin(&mut self.hist, "Typing");
        }
        self.changed();
        Some(r)
    }

    pub fn undo(&mut self) {
        let at = self.undo_where.pop();
        if let Some(w) = &mut self.wb {
            if w.doc.undo(&mut self.hist) {
                self.redo_where.extend(at);
                self.go_where(at);
                self.changed();
            }
        }
    }

    pub fn redo(&mut self) {
        let at = self.redo_where.pop();
        if let Some(w) = &mut self.wb {
            if w.doc.redo(&mut self.hist) {
                self.undo_where.extend(at);
                self.go_where(at);
                self.changed();
            }
        }
    }

    pub fn title(&self) -> String {
        let Some(w) = &self.wb else { return "Values".into() };
        if let Some(x) = &self.excel {
            return x.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled.xlsx".into());
        }
        w.doc.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).or_else(|| self.untitled_name.clone()).unwrap_or_else(|| "Untitled".into())
    }

    // ------------------------------------------------------------------------------ chrome

    /// The zoom and top-left (sheet points) the open sheet was saved with.
    pub fn saved_view(&self) -> Option<(f32, egui::Pos2)> {
        let id = self.sheets().get(self.sheet)?.id;
        if let Some(v) = self.session_views.get(&id) {
            return Some(*v);
        }
        // (a zoom of 0 is none saved: Values' blank template has one, and imports start from
        // it; the default zoom applies)
        let v = self.saved_views.as_ref()?.sheets.get(&id).filter(|v| v.zoom >= 0.1)?;
        let (x, y) = if v.unscaled { v.scroll } else { v.scroll };
        Some((v.zoom.clamp(0.25, 4.0), egui::pos2(x, y)))
    }

    pub fn current_slide(&self) -> Option<CurSheet> {
        self.sheets().get(self.sheet).map(|s| CurSheet { slide: s.id })
    }

    /// Something on the sheet changed (re-read and re-render it).
    pub fn bump(&mut self, _sheet: ObjId) {
        self.changed();
    }

    /// Run a menu command (the ported inspector's buttons).
    pub fn command(&mut self, ctx: &egui::Context, name: &str) {
        crate::menus::run(self, ctx, name);
    }

    /// Image > Replace: a new picture for the replace target.
    pub fn pick_image(&mut self) {
        let Some(id) = self.replace_target.take() else { return };
        let Some(p) = file_dialog(|d| d.add_filter("Pictures", &["png", "jpg", "jpeg"]).pick_file()) else { return };
        let bytes = match std::fs::read(&p) {
            Ok(b) => b,
            Err(e) => return self.notify(format!("Couldn't read the picture: {e}")),
        };
        let Some((w, h)) = nm_core::kn_format::objects::image_size(&bytes) else { return self.notify("Only PNG and JPEG pictures can be used.") };
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "image.png".into());
        let res = self.edit("Replace Image", |d, hist| {
            d.structural(hist, &[id], |k| -> Result<(), nm_core::kn_format::Error> {
                let data = k.add_image_data(bytes, &name)?;
                k.set_image_data(id, data, w as f32, h as f32);
                Ok(())
            })
        });
        if let Some(Err(e)) = res {
            self.notify(format!("Couldn't replace the picture: {e}"));
        }
    }

    /// Hide Sheet: out of the tab bar; the next visible sheet opens.
    pub fn hide_sheet(&mut self, i: usize) {
        let sheets = self.sheets();
        let Some(id) = sheets.get(i).map(|s| s.id) else { return };
        if sheets.iter().filter(|s| !s.hidden).count() < 2 {
            return;
        }
        self.edit("Hide Sheet", |d, h| nm_core::sheets::set_sheet_hidden(d, h, id, true));
        let next = (0..sheets.len()).map(|k| (i + k) % sheets.len()).find(|k| *k != i && !sheets[*k].hidden).unwrap_or(0);
        self.sheet = usize::MAX;
        self.go_to_sheet(next);
    }

    /// Format / Organize in the toolbar: show that inspector, or hide it when it's showing.
    pub fn toggle_inspector(&mut self, tab: InspTab) {
        let organize = tab == InspTab::Organize;
        let showing = self.inspector && (self.insp_tab == InspTab::Organize) == organize;
        if showing {
            self.inspector = false;
        } else {
            self.inspector = true;
            if organize || self.insp_tab == InspTab::Organize {
                self.insp_tab = tab;
            }
        }
    }

    /// Insert > Comment: the comment dialog for the selected cell.
    pub fn add_comment(&mut self) {
        crate::canvas::commit(self);
        let Some(sel) = self.sel else { return };
        let (r, c) = sel.cursor;
        if let Some(mr) = self.mrow(sel.table, r) {
            let text = self.comments.get(&sel.table).and_then(|m| m.get(&(mr, c))).map(|x| x.0.clone()).unwrap_or_default();
            let author = if self.settings.author.is_empty() { std::env::var("USER").unwrap_or_else(|_| "Values".into()) } else { self.settings.author.clone() };
            self.comment_dialog = Some((sel.table, mr, c, text, author));
        }
    }

    pub fn open_set_password(&mut self) {
        self.set_password = Some(Default::default());
    }

    pub fn go_to_sheet(&mut self, i: usize) {
        let n = self.sheets().len();
        if i < n && i != self.sheet {
            crate::canvas::commit(self);
            self.sheet = i;
            self.sel = None;
            self.canvas.obj = None;
            self.canvas.reset();
        }
    }

    pub fn duplicate_sheet(&mut self, i: usize) {
        crate::canvas::commit(self);
        match self.edit("Duplicate Sheet", |d, h| nm_core::sheets::duplicate_sheet(d, h, i)) {
            Some(Ok(_)) => {
                self.sheet = i + 1;
                self.sel = None;
                self.canvas.reset();
            }
            Some(Err(e)) => self.notify(format!("Couldn't duplicate the sheet: {e}")),
            None => {}
        }
    }

    /// Edit > Copy with an object selected (tables, text boxes, shapes, pictures, charts).
    pub fn copy_objects(&mut self) {
        let Some(id) = self.canvas.obj else { return };
        let Some(doc) = self.doc() else { return };
        match nm_core::sheets::copy_objects(doc, &[id]) {
            Some(c) => {
                self.clip = Some(c);
                self.obj_clip_active = true;
            }
            None => self.notify("This object can't be copied yet."),
        }
    }

    /// Paste the copied objects onto this sheet, `offset` points down and right.
    pub fn paste_objects(&mut self, offset: f32) {
        let Some(clip) = self.clip.clone() else { return };
        let Some(sheet) = self.sheets().get(self.sheet).map(|s| s.id) else { return };
        crate::canvas::commit(self);
        match self.edit("Paste", |d, h| nm_core::sheets::paste_objects(d, h, sheet, &clip, offset)) {
            Some(Ok(ids)) => {
                self.sel = None;
                self.canvas.obj = ids.last().copied();
            }
            Some(Err(e)) => self.notify(format!("Couldn't paste: {e}")),
            None => {}
        }
    }

    /// Edit > Paste of copied objects as Numbers places them (Mac N15): with an object
    /// selected 10 pt down and right of where they were, else in the middle of the view.
    pub fn paste_here(&mut self) {
        if self.canvas.obj.is_some() {
            self.paste_objects(10.0);
            return;
        }
        let before = self.canvas.obj;
        self.paste_objects(0.0);
        let (Some(id), Some(c)) = (self.canvas.obj.filter(|o| Some(*o) != before), self.canvas.view_center()) else { return };
        self.refresh();
        if let Some(g) = self.drawables.iter().find(|d| d.id == id).map(crate::tiles::frame) {
            let (dx, dy) = (c.x - (g.x + g.w / 2.0), c.y - (g.y + g.h / 2.0));
            self.edit("Paste", |d, h| d.move_by(h, id, dx, dy));
        }
    }

    /// Option-drag: a copy of object `id` moved by (dx, dy); the original stays (Numbers).
    pub fn duplicate_moved(&mut self, id: ObjId, dx: f32, dy: f32) {
        let Some(doc) = self.doc() else { return };
        let Some(clip) = nm_core::sheets::copy_objects(doc, &[id]) else { return };
        let Some(sheet) = self.sheets().get(self.sheet).map(|s| s.id) else { return };
        match self.edit("Duplicate", |d, h| {
            let ids = nm_core::sheets::paste_objects(d, h, sheet, &clip, 0.0)?;
            for n in &ids {
                d.move_by(h, *n, dx, dy);
            }
            Ok::<_, nm_core::kn_format::Error>(ids)
        }) {
            Some(Ok(ids)) => {
                self.canvas.obj = ids.last().copied();
                self.sel = None;
            }
            Some(Err(e)) => self.notify(format!("Couldn't duplicate: {e}")),
            None => {}
        }
    }

    /// Delete the selected object.
    /// The selected objects: `canvas.obj` and, after Shift-clicks, the others.
    pub fn selected_objs(&self) -> Vec<ObjId> {
        let Some(first) = self.canvas.obj else { return Vec::new() };
        if self.canvas.multi.first() != Some(&first) {
            return vec![first];
        }
        self.canvas.multi.iter().copied().filter(|i| self.drawables.iter().any(|d| d.id == *i)).collect()
    }

    pub fn delete_objects(&mut self) {
        let ids = self.selected_objs();
        if ids.is_empty() {
            return;
        }
        let Some(sheet) = self.sheets().get(self.sheet).map(|s| s.id) else { return };
        if self.drawables.iter().any(|d| ids.contains(&d.id) && d.locked) {
            self.notify("The object is locked (Arrange > Unlock).");
            return;
        }
        let id = ids[0];
        self.edit("Delete", |d, h| nm_core::sheets::detach_drawables(d, h, sheet, &ids));
        self.canvas.obj = None;
        self.canvas.multi.clear();
        if self.sel.is_some_and(|s| s.table == id) {
            self.sel = None;
        }
    }

    /// Edit > Duplicate Selection (⌘D): a copy of the selected object 10 pt down and right (Numbers).
    pub fn duplicate_selection(&mut self) {
        if self.canvas.obj.is_none() {
            // a table whose cells are selected duplicates as a whole
            match self.sel {
                Some(s) => self.canvas.obj = Some(s.table),
                None => return,
            }
        }
        let (saved, active) = (self.clip.take(), self.obj_clip_active);
        self.copy_objects();
        if self.clip.is_some() {
            self.paste_objects(10.0);
        }
        self.clip = saved;
        self.obj_clip_active = active;
    }

    /// Find Next / Find Previous (⌘G / ⇧⌘G) with the last search.
    pub fn find_step(&mut self, next: bool) {
        let Some((q, res, cur)) = self.find.clone() else {
            self.find = Some((String::new(), Vec::new(), 0));
            return;
        };
        let n = res.len();
        if n == 0 || q.is_empty() {
            return;
        }
        let i = if next { (cur + 1) % n } else { (cur + n - 1) % n };
        self.show_match(i);
    }

    fn sheet_tabs(&mut self, ui: &mut egui::Ui) {
        let sheets = self.sheets();
        ui.horizontal_centered(|ui| {
            // (the gap to the separator is 6 + the item spacing twice + its own 4: the same here)
            ui.add_space(6.0 + ui.spacing().item_spacing.x * 2.0 + 4.0 - ui.spacing().item_spacing.x);
            // "+" is a menu in Numbers: New Sheet, Hide "<sheet>", Unhide ▸ (Mac N2). The plus
            // is drawn, centred in its square (the font's "+" sat low and to the left)
            let (plus, _) = egui::containers::menu::MenuButton::from_button(egui::Button::new("").min_size(egui::vec2(28.0, 28.0)).frame(false)).ui(ui, |ui| {
                if ui.button("New Sheet").clicked() {
                    self.add_sheet();
                }
                let visible = sheets.iter().filter(|s| !s.hidden).count();
                if let Some(cur) = sheets.get(self.sheet) {
                    if ui.add_enabled(visible > 1, egui::Button::new(format!("Hide \u{201c}{}\u{201d}", cur.name))).clicked() {
                        self.hide_sheet(self.sheet);
                    }
                }
                let hidden: Vec<(usize, String)> = sheets.iter().enumerate().filter(|(_, s)| s.hidden).map(|(i, s)| (i, s.name.clone())).collect();
                ui.add_enabled_ui(!hidden.is_empty(), |ui| {
                    ui.menu_button("Unhide", |ui| {
                        for (i, name) in &hidden {
                            if ui.button(name).clicked() {
                                let id = sheets[*i].id;
                                self.edit("Unhide Sheet", |d, h| nm_core::sheets::set_sheet_hidden(d, h, id, false));
                                self.go_to_sheet(*i);
                            }
                        }
                    });
                });
            });
            {
                let c = plus.rect.center();
                let st = egui::Stroke::new(1.6, if plus.hovered() { theme::TEXT() } else { theme::TEXT_DIM() });
                ui.painter().line_segment([c - egui::vec2(7.0, 0.0), c + egui::vec2(7.0, 0.0)], st);
                ui.painter().line_segment([c - egui::vec2(0.0, 7.0), c + egui::vec2(0.0, 7.0)], st);
            }
            ui.add_space(6.0);
            ui.add(egui::Separator::default().vertical().spacing(8.0));
            ui.add_space(8.0);
            // tabs that don't fit scroll sideways (wheel or drag), the open one kept in view
            // ‹ › at the bar's right end page the tabs (Numbers, Mac N15)
            let key = egui::Id::unique("sheet-tab-scroll");
            // (a page glides over ~250 ms, ease-in-out: Mac N21) from, to, started at
            let glide: Option<(f32, f32, f64)> = ui.ctx().data(|d| d.get_temp(key));
            let room = (ui.available_width() - 60.0).max(80.0);
            let mut area = egui::ScrollArea::horizontal().id_salt("sheet-tabs").max_width(room).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
            if let Some((from, to, t0)) = glide {
                let t = ((ui.input(|i| i.time) - t0) as f32 / 0.25).min(1.0);
                area = area.horizontal_scroll_offset((from + (to - from) * egui::emath::easing::cubic_in_out(t)).max(0.0));
                if t >= 1.0 {
                    ui.ctx().data_mut(|d| d.remove_temp::<(f32, f32, f64)>(key));
                } else {
                    ui.ctx().request_repaint();
                }
            }
            let out = area.show(ui, |ui| {
            ui.horizontal_centered(|ui| {
            let mut tab_rects: Vec<egui::Rect> = Vec::new();
            let mut dropped: Option<(usize, Option<egui::Pos2>)> = None;
            // tab dragging: (sheet, pointer x inside the tab) since the drag began; last frame's
            // tabs (sheet, rect) to work out the slot from; a dropped tab settling in
            let drag_key = egui::Id::unique("tab-drag");
            let rects_key = egui::Id::unique("tab-rects");
            let settle_key = egui::Id::unique("tab-settle");
            let held: Option<(usize, f32)> = ui.ctx().data(|d| d.get_temp(drag_key));
            let prev_rects: Vec<(usize, egui::Rect)> = ui.ctx().data(|d| d.get_temp(rects_key)).unwrap_or_default();
            let settle: Option<(usize, f32, f64)> = ui.ctx().data(|d| d.get_temp(settle_key));
            let pointer_x = ui.ctx().pointer_latest_pos().map(|p| p.x);
            // the dragged tab's slot among the others: how many of them its middle has passed
            let drag: Option<(usize, f32, usize)> = held.zip(pointer_x).map(|((di, grab), px)| {
                let w = prev_rects.iter().find(|(k, _)| *k == di).map(|(_, r)| r.width()).unwrap_or(117.0);
                let mid = px - grab + w / 2.0;
                (di, grab, prev_rects.iter().filter(|(k, r)| *k != di && r.center().x < mid).count())
            });
            let mut tab_sheets: Vec<usize> = Vec::new();
            // Format > Sheet > Tab Color (Mac N23 A1: the open tab fills with it)
            let tab_colors: Vec<Option<Color32>> = sheets
                .iter()
                .map(|s| self.wb.as_ref().and_then(|w| nm_core::sheets::tab_color(w.file(), s.id)).map(|c| Color32::from_rgba_unmultiplied((c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, (c.a * 255.0) as u8)))
                .collect();
            let mut drag_start: Option<(usize, f32)> = None;
            let mut dragged_now: Option<(usize, String, egui::Vec2)> = None;
            for (i, s) in sheets.iter().enumerate() {
                if s.hidden {
                    continue;
                }
                if let Some((n, text)) = &mut self.renaming {
                    if *n == i {
                        let fid = egui::Id::unique(("sheet-rename", *n));
                        let r = ui.add(egui::TextEdit::singleline(text).desired_width(140.0).id(fid));
                        // focus once, when the field opens (asking every frame kept Return from
                        // ever finishing the edit)
                        let opened: bool = ui.data(|d| d.get_temp(fid)).unwrap_or(false);
                        if !opened {
                            r.request_focus();
                            ui.data_mut(|d| d.insert_temp(fid, true));
                        }
                        // Return or clicking away renames; Escape keeps the old name
                        let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                        if enter || esc || (opened && r.lost_focus()) || (opened && !r.has_focus() && ui.input(|i| i.pointer.any_pressed())) {
                            let (n, text) = self.renaming.take().unwrap();
                            ui.data_mut(|d| d.remove::<bool>(fid));
                            ui.memory_mut(|m| m.surrender_focus(fid));
                            let t = text.trim().to_string();
                            if !esc && !t.is_empty() && t != sheets[n].name {
                                let id = sheets[n].id;
                                self.edit("Rename Sheet", |d, h| nm_core::sheets::rename_sheet(d, h, id, &t));
                            }
                        }
                        continue;
                    }
                }
                // Numbers: the open sheet is a green capsule, the others just a grey label (N2)
                let on = i == self.sheet;
                // long names are cut short in the middle (Numbers: "MASTER CALEND…6 Student Version")
                let n: Vec<char> = s.name.chars().collect();
                let shown = if n.len() > 30 { format!("{}…{}", n[..13].iter().collect::<String>(), n[n.len() - 15..].iter().collect::<String>()) } else { s.name.clone() };
                // drawn by hand so tabs can slide while one is dragged (Mac N21: the dragged tab
                // becomes the green pill and follows the pointer, the others slide aside over
                // ~175 ms as it passes their middles, and it settles into its slot in ~100 ms)
                let (font, colour) = if on { (egui::FontId::new(13.0, egui::FontFamily::Name("bold".into())), Color32::WHITE) } else { (egui::FontId::proportional(13.0), theme::TEXT_DIM()) };
                let galley = ui.painter().layout_no_wrap(shown.clone(), font.clone(), colour);
                let size = egui::vec2((galley.size().x + 20.0).max(if on { 117.0 } else { 90.0 }), 27.0);
                let (slot, r) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
                let vi = tab_rects.len();
                tab_rects.push(slot);
                tab_sheets.push(i);
                let mut at = slot;
                if let Some((di, grab, slot_v)) = drag {
                    if di == i {
                        // painted at the pointer after the loop
                        at = egui::Rect::NOTHING;
                    } else {
                        let vd = prev_rects.iter().position(|(k, _)| *k == di).unwrap_or(0);
                        let oj = if vi < vd { vi } else { vi - 1 };
                        let wd = prev_rects.get(vd).map(|(_, r)| r.width()).unwrap_or(117.0) + 4.0 + ui.spacing().item_spacing.x;
                        let shift = if vi > vd && oj < slot_v { -wd } else if vi < vd && oj >= slot_v { wd } else { 0.0 };
                        let x = ui.ctx().animate_value_with_time(egui::Id::unique(("tab-shift", i)), shift, 0.175);
                        at = slot.translate(egui::vec2(x, 0.0));
                        let _ = grab;
                    }
                } else {
                    let _ = ui.ctx().animate_value_with_time(egui::Id::unique(("tab-shift", i)), 0.0, 0.0);
                }
                // just dropped here: glide in from where it was let go
                if let Some((si, from_x, t0)) = settle
                    && si == i
                {
                    let t = ((ui.input(|i| i.time) - t0) as f32 / 0.1).min(1.0);
                    at = at.translate(egui::vec2((from_x - slot.min.x) * (1.0 - egui::emath::easing::cubic_out(t)), 0.0));
                    if t < 1.0 {
                        ui.ctx().request_repaint();
                    }
                }
                if at.is_positive() {
                    let tc = tab_colors.get(i).copied().flatten();
                    if on {
                        ui.painter().rect_filled(at, 14.0, tc.unwrap_or_else(accent));
                    } else if let Some(c) = tc {
                        // (a coloured tab that isn't open: its colour, faint; to check with N24)
                        ui.painter().rect_filled(at, 14.0, c.gamma_multiply(if r.hovered() { 0.5 } else { 0.32 }));
                    } else if r.hovered() {
                        ui.painter().rect_filled(at, 14.0, ui.visuals().widgets.hovered.weak_bg_fill);
                    }
                    ui.painter().galley(at.center() - galley.size() / 2.0, galley.clone(), colour);
                }
                if on && ui.ctx().data(|d| d.get_temp::<usize>(egui::Id::unique("tab-shown"))) != Some(i) {
                    // (only as far as needed: a tab already in view stays put, the strip does not jump)
                    r.scroll_to_me(None);
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("tab-shown"), i));
                }
                // (where the button went down: the drag is only seen once the pointer has moved)
                if r.drag_started()
                    && let Some(p) = ui.input(|i| i.pointer.press_origin()).or(r.interact_pointer_pos())
                {
                    drag_start = Some((i, p.x - slot.min.x));
                }
                if r.drag_stopped() {
                    dropped = Some((i, r.interact_pointer_pos().or(ui.ctx().pointer_latest_pos())));
                }
                if r.dragged() {
                    dragged_now = Some((i, shown.clone(), size));
                }
                if r.clicked() {
                    self.sheet = i;
                    self.sel = None;
                    self.canvas.reset();
                }
                if r.double_clicked() {
                    self.renaming = Some((i, s.name.clone()));
                }
                r.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        self.renaming = Some((i, s.name.clone()));
                    }
                    if ui.button("Duplicate").clicked() {
                        crate::canvas::commit(self);
                        match self.edit("Duplicate Sheet", |d, h| nm_core::sheets::duplicate_sheet(d, h, i)) {
                            Some(Ok(_)) => {
                                self.sheet = i + 1;
                                self.sel = None;
                                self.canvas.reset();
                            }
                            Some(Err(e)) => self.notify(format!("Couldn't duplicate the sheet: {e}")),
                            None => {}
                        }
                    }
                    if ui.add_enabled(i > 0, egui::Button::new("Move Left")).clicked() {
                        self.edit("Move Sheet", |d, h| nm_core::sheets::move_sheet(d, h, i, i - 1));
                        self.sheet = i - 1;
                    }
                    if ui.add_enabled(i + 1 < sheets.len(), egui::Button::new("Move Right")).clicked() {
                        self.edit("Move Sheet", |d, h| nm_core::sheets::move_sheet(d, h, i, i + 1));
                        self.sheet = i + 1;
                    }
                    ui.separator();
                    if ui.add_enabled(sheets.len() > 1, egui::Button::new("Delete")).clicked() {
                        self.edit("Delete Sheet", |d, h| nm_core::sheets::delete_sheet(d, h, i));
                        self.sel = None;
                        self.sheet = self.sheet.min(sheets.len() - 2);
                        self.canvas.reset();
                    }
                });
                ui.add_space(4.0);
            }
            ui.ctx().data_mut(|d| d.insert_temp(rects_key, tab_sheets.iter().copied().zip(tab_rects.iter().copied()).collect::<Vec<_>>()));
            if let Some(s0) = drag_start {
                ui.ctx().data_mut(|d| d.insert_temp(drag_key, s0));
            }
            // the dragged tab: the green pill, following the pointer along the strip
            if let (Some((_, name, size)), Some((_, grab, _)), Some(px)) = (dragged_now, drag, pointer_x) {
                let cy = tab_rects.first().map(|r| r.center().y).unwrap_or(ui.max_rect().center().y);
                let at = egui::Rect::from_min_size(egui::pos2(px - grab, cy - size.y / 2.0), size);
                ui.painter().rect_filled(at, 14.0, accent());
                let g = ui.painter().layout_no_wrap(name, egui::FontId::new(13.0, egui::FontFamily::Name("bold".into())), Color32::WHITE);
                ui.painter().galley(at.center() - g.size() / 2.0, g, Color32::WHITE);
                ui.ctx().request_repaint();
            }
            // let go: the tab moves to the slot it was over, becomes the open sheet, and settles in
            if let Some((from, _)) = dropped {
                ui.ctx().data_mut(|d| d.remove_temp::<(usize, f32)>(drag_key));
                if let Some((_, grab, slot_v)) = drag.filter(|d| d.0 == from) {
                    let others: Vec<usize> = tab_sheets.iter().copied().filter(|k| *k != from).collect();
                    // (move_sheet's target is the index once the tab is out of the list)
                    let target = match others.get(slot_v) {
                        Some(&o) => if o > from { o - 1 } else { o },
                        None => others.last().map(|&o| if o > from { o } else { o + 1 }).unwrap_or(from),
                    };
                    if target != from {
                        self.edit("Move Sheet", |d, h| nm_core::sheets::move_sheet(d, h, from, target));
                    }
                    if self.sheet != target || target != from {
                        self.sheet = target;
                        self.sel = None;
                        self.canvas.reset();
                    }
                    let now = ui.input(|i| i.time);
                    ui.ctx().data_mut(|d| d.insert_temp(settle_key, (target, pointer_x.unwrap_or(0.0) - grab, now)));
                }
            }
            });
            });
            let (off, content, view) = (out.state.clamped_offset().x, out.content_size.x, out.inner_rect.width());
            if content > view + 1.0 {
                let page = view * 0.8;
                if ui.add_enabled(off > 0.5, egui::Button::new(RichText::new("‹").size(18.0)).frame(false).min_size(egui::vec2(25.0, 27.0))).on_hover_text("Back").clicked() {
                    let now = ui.input(|i| i.time);
                    ui.ctx().data_mut(|d| d.insert_temp(key, (off, (off - page).max(0.0), now)));
                }
                if ui.add_enabled(off + view < content - 0.5, egui::Button::new(RichText::new("›").size(18.0)).frame(false).min_size(egui::vec2(25.0, 27.0))).on_hover_text("Forward").clicked() {
                    let now = ui.input(|i| i.time);
                    ui.ctx().data_mut(|d| d.insert_temp(key, (off, (off + page).min(content - view), now)));
                }
            }
        });
    }

    /// The "+" button / Insert > Sheet: a new sheet right after the open one (Numbers, Mac
    /// N21), named like Numbers ("Sheet 2"…).
    /// A new sheet named after `base` ("Pivot Table", "Pivot Table 2"…) with no table, shown.
    pub fn add_sheet_empty(&mut self, base: &str) {
        let sheets = self.sheets();
        let Some(model) = sheets.get(self.sheet).map(|s| s.id) else { return };
        let names: Vec<String> = sheets.iter().map(|s| s.name.clone()).collect();
        let name = (1..).map(|n| if n == 1 { base.to_string() } else { format!("{base} {n}") }).find(|n| !names.contains(n)).unwrap();
        let at = sheets.len();
        if let Some(Ok(_)) = self.edit("Add Sheet", |d, h| nm_core::sheets::add_sheet(d, h, model, &name, at)) {
            self.sheet = at;
            self.sel = None;
            self.canvas.reset();
        }
    }

    pub fn add_sheet(&mut self) {
        let sheets = self.sheets();
        let Some(model) = sheets.get(self.sheet).map(|s| s.id) else { return };
        let names: Vec<String> = sheets.iter().map(|s| s.name.clone()).collect();
        // (the first free "Sheet n": Numbers names it "Sheet 1" when that is free, Mac N21)
        let name = (1..).map(|n| format!("Sheet {n}")).find(|n| !names.contains(n)).unwrap();
        let at = (self.sheet + 1).min(sheets.len());
        let donor = nm_core::sheets::new_sheet_table_donor();
        let made = self.edit("Add Sheet", |d, h| {
            let s = nm_core::sheets::add_sheet(d, h, model, &name, at)?;
            // a new sheet comes with a table, as in Numbers
            if let Ok((k, info)) = &donor {
                nm_core::sheets::insert_table(d, h, s, k, *info, 0.0, 0.0)?;
            }
            Ok::<_, nm_core::kn_format::Error>(s)
        });
        match made {
            Some(Ok(_)) => {
                self.sheet = at;
                self.sel = None;
                self.canvas.reset();
            }
            Some(Err(e)) => self.notify(format!("Couldn't add a sheet: {e}")),
            None => {}
        }
    }

    /// Matches of `q` (case-insensitive, in what the cells show) on every sheet.
    pub fn search(&self, q: &str) -> Vec<(usize, ObjId, u32, u32)> {
        let Some(wb) = &self.wb else { return Vec::new() };
        let q = q.to_lowercase();
        let mut out = Vec::new();
        if q.is_empty() {
            return out;
        }
        for (si, s) in wb.sheets().iter().enumerate() {
            for d in wb.drawables_and_views(s).0 {
                let DrawableKind::Table(t) = &d.kind else { continue };
                for r in 0..t.rows {
                    if t.row_heights.get(r as usize).is_some_and(|h| *h <= 0.0) {
                        continue;
                    }
                    for c in 0..t.cols {
                        if t.cells[(r * t.cols + c) as usize].to_lowercase().contains(&q) {
                            out.push((si, d.id, r, c));
                        }
                    }
                }
            }
        }
        out
    }

    /// View > Show Colors (Apple's Colors window, kn-ui's), with Fill Selection for the selected
    /// cells or objects; and the window a colour well's wheel button opens.
    fn colors_window(&mut self, ctx: &egui::Context) {
        // saved colours: from the settings at first, back into them when they change
        if self.color_state.saved.is_empty() && !self.settings.saved_colors.is_empty() {
            self.color_state.saved_from_string(&self.settings.saved_colors.clone());
        }
        let before = self.color_state.saved.clone();
        kn_ui::colors::colors_window(ctx, &mut self.color_state);
        if self.show_colors {
            let mut open = true;
            let mut apply = false;
            let mut c = self.colors_pick;
            let any = self.sel.is_some() || self.canvas.obj.is_some();
            // (the suite's Colors window: its frame, margins and eyedropper, kn-ui)
            kn_ui::colors::colors_window_with(ctx, "view-colors", &mut self.color_state, &mut c, &mut open, |ui| {
                if ui.add_enabled(any, egui::Button::new("Fill Selection").min_size(egui::vec2(232.0, 26.0))).clicked() {
                    apply = true;
                }
            });
            self.colors_pick = c;
            self.show_colors = open;
            if apply {
                let col = nm_core::kn_model::Color { r: f32::from(c.r()) / 255.0, g: f32::from(c.g()) / 255.0, b: f32::from(c.b()) / 255.0, a: f32::from(c.a()) / 255.0 };
                let spec = nm_core::kn_model::format::FillSpec::Color(col);
                if let Some(sel) = self.sel {
                    crate::canvas::commit(self);
                    let t = sel.table;
                    let blocks = self.model_blocks(t, sel.block());
                    self.edit("Fill", |d, h| blocks.iter().try_for_each(|b| d.set_cells_fill(h, t, *b, Some(&spec)).map(|_| ())));
                } else {
                    let ids = self.selected_objs();
                    self.edit("Fill", |d, h| ids.iter().for_each(|i| {
                        d.set_fill(h, *i, &spec);
                    }));
                }
            }
        }
        if self.color_state.saved != before {
            self.settings.saved_colors = self.color_state.saved_to_string();
            self.settings.save();
        }
    }

    /// Undo / redo: back to the sheet and cells the step was made on, scrolled into view (the
    /// view jumps, Mac N21).
    fn go_where(&mut self, at: Option<(usize, Option<Sel>)>) {
        let Some((si, sel)) = at else { return };
        if si != self.sheet && si < self.sheets().len() {
            self.sheet = si;
            self.canvas.reset();
            self.refresh();
        }
        if let Some(s) = sel {
            self.sel = Some(s);
            self.reveal_cell(s.table, s.cursor.0, s.cursor.1);
        }
    }

    /// Scroll cell (r, c) of table `t` into view.
    pub fn reveal_cell(&mut self, t: ObjId, r: u32, c: u32) {
        if let Some((d, td)) = self.table(t) {
            let g = td.frame(&d.geom);
            let x: f32 = g.x + td.col_widths.iter().take(c as usize).sum::<f32>();
            let y: f32 = g.y + td.row_heights.iter().take(r as usize).sum::<f32>();
            let size = egui::vec2(td.col_widths.get(c as usize).copied().unwrap_or(98.0), td.row_heights.get(r as usize).copied().unwrap_or(20.0));
            self.canvas.reveal = Some(egui::Rect::from_min_size(egui::pos2(x, y), size));
        }
    }

    /// Select match `i` of the find results, switching sheet and scrolling to it.
    fn show_match(&mut self, i: usize) {
        let Some((_, results, cur)) = &mut self.find else { return };
        let Some(&(si, t, r, c)) = results.get(i) else { return };
        *cur = i;
        if si != self.sheet {
            self.sheet = si;
            self.canvas.reset();
            self.refresh();
        }
        self.sel = Some(Sel::cell(t, r, c));
        self.reveal_cell(t, r, c);
    }

    fn find_bar(&mut self, ui: &mut egui::Ui) {
        let Some((mut q, _, cur)) = self.find.clone() else { return };
        let mut go = None;
        let mut replace = self.replace.clone();
        let mut do_replace: Option<bool> = None;
        ui.horizontal_centered(|ui| {
            // Find / Find & Replace (Numbers' pop-up at the bar's left)
            let mode = if replace.is_some() { "Find & Replace" } else { "Find" };
            egui::ComboBox::from_id_salt("find-mode").width(120.0).selected_text(mode).show_ui(ui, |ui| {
                if ui.selectable_label(replace.is_none(), "Find").clicked() {
                    replace = None;
                }
                if ui.selectable_label(replace.is_some(), "Find & Replace").clicked() && replace.is_none() {
                    replace = Some(String::new());
                }
            });
            let r = ui.add(egui::TextEdit::singleline(&mut q).desired_width(260.0).id(egui::Id::unique("find-field")));
            if ui.memory(|m| m.focused().is_none()) {
                r.request_focus();
            }
            let n = self.find.as_ref().map(|f| f.1.len()).unwrap_or(0);
            if r.changed() {
                let res = self.search(&q);
                let any = !res.is_empty();
                self.find = Some((q.clone(), res, 0));
                if any {
                    go = Some(0);
                }
            }
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let back = ui.input(|i| i.modifiers.shift);
            if ui.add_enabled(n > 0, egui::Button::new("Previous")).clicked() || enter && back && n > 0 {
                go = Some((cur + n - 1) % n);
            }
            if ui.add_enabled(n > 0, egui::Button::new("Next")).clicked() || enter && !back && n > 0 {
                go = Some((cur + 1) % n);
            }
            ui.label(if q.is_empty() { String::new() } else if n == 0 { "No matches".into() } else { format!("{} of {n}", cur.min(n - 1) + 1) });
            if let Some(w) = replace.as_mut() {
                ui.separator();
                ui.label("Replace:");
                ui.add(egui::TextEdit::singleline(w).desired_width(200.0).id(egui::Id::unique("replace-field")));
                if ui.add_enabled(n > 0, egui::Button::new("Replace All")).clicked() {
                    do_replace = Some(true);
                }
                if ui.add_enabled(n > 0, egui::Button::new("Replace")).clicked() {
                    do_replace = Some(false);
                }
            }
            if enter {
                r.request_focus();
            }
            if ui.button("Done").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.find = None;
            }
        });
        if self.find.is_some() {
            self.replace = replace;
        }
        if let Some(i) = go {
            self.show_match(i);
        }
        if let Some(all) = do_replace {
            self.replace_matches(all);
        }
    }

    /// Replace (the current match) or Replace All: the matched text in plain cells (not
    /// formulas) becomes the replacement, as one undo step; the search runs again after.
    pub fn replace_matches(&mut self, all: bool) {
        let (Some((q, res, cur)), Some(with)) = (self.find.clone(), self.replace.clone()) else { return };
        if q.is_empty() || res.is_empty() {
            return;
        }
        crate::canvas::commit(self);
        let targets: Vec<(usize, ObjId, u32, u32)> = if all { res.clone() } else { res.get(cur).copied().into_iter().collect() };
        let Some(wb) = &self.wb else { return };
        let sheets = wb.sheets();
        let mut edits: Vec<(ObjId, u32, u32, String)> = Vec::new();
        let mut per_sheet: std::collections::HashMap<usize, (Vec<nm_core::kn_model::Drawable>, std::collections::HashMap<ObjId, nm_core::categories::View>)> = Default::default();
        for (si, t, r, c) in targets {
            let Some(s) = sheets.get(si) else { continue };
            let (ds, views) = per_sheet.entry(si).or_insert_with(|| wb.drawables_and_views(s));
            let Some(DrawableKind::Table(td)) = ds.iter().find(|d| d.id == t).map(|d| &d.kind) else { continue };
            let mr = match views.get(&t) {
                Some(v) => match v.rows.get(r as usize).copied().flatten() {
                    Some(m) => m,
                    None => continue,
                },
                None => r,
            };
            if wb.doc.formula_text(t, mr, c).is_some() || nm_core::formula::formula_text(&wb.doc.file, t, mr, c).is_some() {
                continue;
            }
            let Some(old) = td.cells.get((r * td.cols + c) as usize) else { continue };
            edits.push((t, mr, c, replace_ci(old, &q, &with)));
        }
        let n = edits.len();
        if n == 0 {
            self.notify("Nothing to replace (formulas are left as they are)");
            return;
        }
        let res = self.edit(if all { "Replace All" } else { "Replace" }, |d, h| {
            edits.iter().try_for_each(|(t, r, c, text)| d.set_cell(h, *t, *r, *c, &nm_core::kn_model::tablecells::CellInput::parse(text)))
        });
        if let Some(Err(e)) = res {
            self.notify(format!("Couldn't replace: {e}"));
        } else if all {
            self.notify(format!("Replaced {n} {}", if n == 1 { "match" } else { "matches" }));
        }
        let found = self.search(&q);
        let next = if all { 0 } else { cur.min(found.len().saturating_sub(1)) };
        let any = !found.is_empty();
        self.find = Some((q, found, next));
        if any && !all {
            self.show_match(next);
        }
    }

    /// Toolbar Text: a text box to the right of what's on the sheet, ready to type in.
    pub fn insert_text_box(&mut self) {
        crate::canvas::commit(self);
        let Some(sheet) = self.sheets().get(self.sheet).map(|s| s.id) else { return };
        let x = self.drawables.iter().map(crate::tiles::frame).fold(0f32, |x, g| x.max(g.x + g.w + 40.0));
        // the default appearance set with Format > Advanced, else Numbers' own text box
        let donor = match self.default_textbox.clone().map(Ok).unwrap_or_else(nm_core::sheets::text_box_donor) {
            Ok(d) => d,
            Err(e) => return self.notify(format!("Couldn't insert a text box: {e}")),
        };
        match self.edit("Insert Text Box", |d, h| {
            let id = nm_core::sheets::insert_object(d, h, sheet, &donor.0, donor.1, x, 40.0)?;
            nm_core::sheets::set_text_box_path(d, h, id);
            if let Some(st) = d.storage_of(id) {
                d.set_storage_text(h, st, "Text");
            }
            Ok::<_, nm_core::kn_format::Error>(id)
        }) {
            Some(Ok(id)) => {
                self.sel = None;
                self.canvas.obj = Some(id);
                // typing replaces the placeholder (all of it selected)
                crate::textedit::begin(self, id, None);
            }
            Some(Err(e)) => self.notify(format!("Couldn't insert a text box: {e}")),
            None => {}
        }
    }

    /// A free place for a new object (about 100 pt square): the first spot in view, reading
    /// left to right, top to bottom, that touches nothing (Numbers puts new objects where they
    /// can be seen); else right of everything on the sheet.
    /// Where a new object goes: a free place in view.
    pub fn free_spot_pub(&self) -> (f32, f32) {
        self.free_spot()
    }

    fn free_spot(&self) -> (f32, f32) {
        self.room_in_view(100.0, 100.0).unwrap_or_else(|| {
            let x = self.drawables.iter().map(crate::tiles::frame).fold(0f32, |x, g| x.max(g.x + g.w + 40.0));
            (x, 40.0)
        })
    }

    /// Room in view for a `w` x `h` object.
    fn room_in_view(&self, w: f32, h: f32) -> Option<(f32, f32)> {
        let taken: Vec<egui::Rect> = self.drawables.iter().map(crate::tiles::frame).map(|g| egui::Rect::from_min_size(egui::pos2(g.x, g.y), egui::vec2(g.w, g.h)).expand(20.0)).collect();
        if let Some(v) = self.canvas.visible() {
            // (the view may start above / left of the sheet's edge: nothing goes there)
            let v = v.shrink(20.0);
            let v = egui::Rect::from_min_max(v.min.max(egui::pos2(20.0, 20.0)), v.max);
            let mut y = v.min.y;
            while y + h <= v.max.y {
                let mut x = v.min.x;
                while x + w <= v.max.x {
                    let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
                    if !taken.iter().any(|t| t.intersects(r)) {
                        return Some((x.round(), y.round()));
                    }
                    x += 20.0;
                }
                y += 20.0;
            }
        }
        None
    }

    /// Toolbar Chart: a chart of the selected cells (a column per series, rows as categories).
    /// Insert > Chart: donor `kind` (CHART_TYPES), then given type `ty` when that's another one.
    pub fn insert_chart(&mut self, kind: usize, ty: Option<&str>) {
        crate::canvas::commit(self);
        let Some(sheet) = self.sheets().get(self.sheet).map(|s| s.id) else { return };
        // the selected cells, which the chart stays linked to (Numbers)
        let from = self.sel.and_then(|sel| {
            let (_, t) = self.table(sel.table)?;
            let b = sel.block();
            let g = nm_core::charts::grid_from_cells(t, b);
            (!g.row_names.is_empty()).then(|| (g, sel.table, nm_core::charts::link_cells(t, b)))
        });
        // (a table shown by categories lists rows out of their stored order: not linked)
        let link = from.as_ref().filter(|f| !self.views.contains_key(&f.1)).map(|f| (f.1, f.2.clone()));
        let grid = match from.map(|f| f.0) {
            Some(g) => g,
            // nothing (or no numbers) selected: Numbers' sample data, to edit with Edit Chart Data
            _ => nm_core::kn_model::chartdata::ChartGrid {
                row_names: vec!["Region 1".into(), "Region 2".into()],
                col_names: ["2007", "2008", "2009", "2010"].iter().map(|s| s.to_string()).collect(),
                values: vec![vec![17.0, 26.0, 53.0, 96.0], vec![55.0, 43.0, 70.0, 58.0]],
            },
        };
        // where there's room in view for the chart and its legend above it (Numbers puts a new
        // chart in the visible part of the sheet)
        const LEGEND: f32 = 60.0;
        let (w, h) = nm_core::charts::chart_size(kind).unwrap_or((400.0, 300.0));
        let (x, y) = match self.room_in_view(w, h + LEGEND) {
            Some((x, y)) => (x, y + LEGEND),
            // no room in view: the middle of the view (Numbers overlaps then)
            None => match self.canvas.visible() {
                Some(v) => ((v.center().x - w / 2.0).max(20.0).round(), (v.center().y - h / 2.0).max(20.0 + LEGEND).round()),
                None => self.free_spot(),
            },
        };
        match self.edit("Insert Chart", |doc, h| {
            let id = nm_core::charts::insert_chart(doc, h, sheet, kind, &grid, x, y)?;
            if let Some(ty) = ty {
                doc.set_chart_type(h, id, ty);
            }
            // linked to the cells it shows, else not linked (the donor's link is to its own table)
            match &link {
                Some((t, (series, names, cats))) => {
                    nm_core::chartlink::set_link(doc, h, id, *t, series, names, cats, true);
                }
                None => {
                    nm_core::chartlink::clear_link(doc, h, id);
                }
            }
            Ok::<_, nm_core::kn_format::Error>(id)
        }) {
            Some(Ok(id)) => {
                self.sel = None;
                self.canvas.obj = Some(id);
                self.insp_tab = InspTab::Arrange;
            }
            Some(Err(e)) => self.notify(format!("Couldn't insert the chart: {e}")),
            None => {}
        }
    }

    /// Toolbar Shape: shape `i` of the popover, as Numbers inserts it.
    pub fn insert_shape(&mut self, i: usize) {
        self.insert_shape_then(i, |_, _, _| {});
    }

    /// Insert shape `i`, then `then` on it in the same step (a preset outline, arrowheads).
    pub fn insert_shape_then(&mut self, i: usize, then: impl FnOnce(&mut Document, &mut History, ObjId)) {
        crate::canvas::commit(self);
        let Some(sheet) = self.sheets().get(self.sheet).map(|s| s.id) else { return };
        let (x, y) = self.free_spot();
        let donor = match nm_core::sheets::shape_donor(i) {
            Ok(d) => d,
            Err(e) => return self.notify(format!("Couldn't insert a shape: {e}")),
        };
        match self.edit("Insert Shape", |d, h| {
            let id = nm_core::sheets::insert_object(d, h, sheet, &donor.0, donor.1, x, y)?;
            then(d, h, id);
            Ok::<_, nm_core::kn_format::Error>(id)
        }) {
            Some(Ok(id)) => {
                self.sel = None;
                self.canvas.obj = Some(id);
                self.insp_tab = InspTab::Arrange;
            }
            Some(Err(e)) => self.notify(format!("Couldn't insert a shape: {e}")),
            None => {}
        }
    }

    /// Insert > Image…: a PNG or JPEG file.
    pub fn insert_image_dialog(&mut self) {
        let Some(p) = file_dialog(|d| d.add_filter("Pictures", &["png", "jpg", "jpeg"]).pick_file()) else { return };
        self.insert_image_file(&p);
    }

    pub fn insert_image_file(&mut self, p: &Path) {
        crate::canvas::commit(self);
        let Some(sheet) = self.sheets().get(self.sheet).map(|s| s.id) else { return };
        let bytes = match std::fs::read(p) {
            Ok(b) => b,
            Err(e) => return self.notify(format!("Couldn't read {}: {e}", p.display())),
        };
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "image.png".into());
        let (x, y) = self.free_spot();
        match self.edit("Insert Image", |d, h| nm_core::sheets::insert_image(d, h, sheet, bytes, &name, x, y)) {
            Some(Ok(id)) => {
                self.sel = None;
                self.canvas.obj = Some(id);
                self.insp_tab = InspTab::Arrange;
            }
            Some(Err(e)) => self.notify(format!("Couldn't insert the picture: {e}")),
            None => {}
        }
    }

    /// Toolbar Table: a new table below everything on the sheet.
    pub fn insert_table(&mut self, style: usize) {
        crate::canvas::commit(self);
        let Some(sheet) = self.sheets().get(self.sheet).map(|s| s.id) else { return };
        let (x, y) = self.drawables.iter().map(crate::tiles::frame).fold((0f32, 0f32), |(x, y), g| (x.min(g.x), y.max(g.y + g.h + 40.0)));
        let donor = match nm_core::sheets::table_style_donor(style) {
            Ok(d) => d,
            Err(e) => return self.notify(format!("Couldn't insert a table: {e}")),
        };
        match self.edit("Insert Table", |d, h| nm_core::sheets::insert_table(d, h, sheet, &donor.0, donor.1, x, y)) {
            Some(Ok(t)) => {
                self.sel = Some(Sel::cell(t, 0, 0));
                self.canvas.obj = None;
            }
            Some(Err(e)) => self.notify(format!("Couldn't insert a table: {e}")),
            None => {}
        }
    }

    /// Numbers' bottom bar: what the selected cell holds (the formula, or the value).
    fn cell_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(12.0);
            if let Some((msg, _)) = &self.message {
                ui.label(RichText::new(msg).color(theme::TEXT_DIM()));
                return;
            }
            let Some(sel) = self.sel else { return };
            // several cells: Numbers' quick calculations of the numbers among them
            let (r0, c0, r1, c1) = sel.block();
            if (r0, c0) != (r1, c1) {
                let nums: Vec<f64> = self
                    .table(sel.table)
                    .map(|(_, t)| {
                        (r0..=r1)
                            .flat_map(|r| (c0..=c1).map(move |c| (r, c)))
                            .filter(|(r, _)| t.row_heights.get(*r as usize).is_none_or(|h| *h > 0.0))
                            .filter_map(|(r, c)| {
                                let s = t.cells.get((r * t.cols + c) as usize)?;
                                let clean: String = s.chars().filter(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | 'E' | 'e')).collect();
                                if clean.is_empty() || s.chars().any(|ch| ch.is_alphabetic() && ch != 'E' && ch != 'e') { None } else { clean.parse::<f64>().ok().map(|v| if s.trim_end().ends_with('%') { v / 100.0 } else { v }) }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let cells = (r1 - r0 + 1) * (c1 - c0 + 1);
                if nums.is_empty() {
                    ui.label(RichText::new(format!("{cells} cells")).color(theme::TEXT_DIM()));
                    return;
                }
                let sum: f64 = nums.iter().sum();
                let fmt = |v: f64| nm_core::kn_model::table::fmt_num((v * 1e4).round() / 1e4);
                for (name, v) in [
                    ("SUM", sum),
                    ("AVG", sum / nums.len() as f64),
                    ("MIN", nums.iter().copied().fold(f64::MAX, f64::min)),
                    ("MAX", nums.iter().copied().fold(f64::MIN, f64::max)),
                    ("COUNT", nums.len() as f64),
                ] {
                    ui.label(RichText::new(name).color(theme::TEXT_DIM()));
                    ui.label(fmt(v));
                    ui.add_space(14.0);
                }
                return;
            }
            let (vr, c) = sel.cursor;
            let value = self.table(sel.table).and_then(|(_, t)| t.cells.get((vr * t.cols + c) as usize).cloned()).unwrap_or_default();
            let Some(r) = self.mrow(sel.table, vr) else {
                ui.label(RichText::new("Category").color(theme::TEXT_DIM()));
                ui.separator();
                ui.add(egui::Label::new(one_line(value.trim_start_matches("▼ "))).truncate());
                return;
            };
            let formula = self.formula_of(sel.table, r, c);
            if let Some((text, author)) = self.comments.get(&sel.table).and_then(|m| m.get(&(r, c))).cloned() {
                ui.label(RichText::new("Comment").color(Color32::from_rgb(0xb0, 0x8a, 0x00)));
                ui.add(egui::Label::new(one_line(&if author.is_empty() { text } else { format!("{text} — {author}") })).truncate());
                ui.separator();
            }
            let (kind, shown) = match formula {
                Some(f) => ("Formula", format!("{f}      {value}")),
                None if value.is_empty() => ("Empty", String::new()),
                None => match self.wb.as_ref().and_then(|w| w.doc.cell_values(sel.table).get(&(r, c)).cloned()) {
                    Some(nm_core::kn_model::table::CellValue::Date(_)) => ("Date", value),
                    Some(nm_core::kn_model::table::CellValue::Duration(_)) => ("Duration", value),
                    Some(nm_core::kn_model::table::CellValue::Number(_)) => ("Number", value),
                    Some(nm_core::kn_model::table::CellValue::Bool(_)) => ("Checkbox", value),
                    _ if value.trim().parse::<f64>().is_ok() => ("Number", value),
                    _ => ("Text", value),
                },
            };
            ui.label(RichText::new(kind).color(theme::TEXT_DIM()));
            ui.separator();
            ui.label(RichText::new(format!("{}{}", crate::canvas::col_name(c), r + 1)).color(theme::TEXT_DIM()));
            ui.add_space(8.0);
            ui.add(egui::Label::new(one_line(&shown)).truncate());
        });
    }

    pub fn export_pdf_dialog(&mut self) {
        let name = self.title().trim_end_matches(".numbers").to_string();
        let Some(path) = file_dialog(|d| d.add_filter("PDF", &["pdf"]).set_file_name(format!("{name}.pdf")).save_file()) else { return };
        let Some(wb) = &self.wb else { return };
        match crate::pdf::export_with(wb, &path, self.canvas.tiles.extras.clone()) {
            Ok(n) => self.notify(format!("Exported {n} pages to {}", path.display())),
            Err(e) => self.notify(format!("Export failed: {e}")),
        }
    }

    /// File > Export To > Excel…: the spreadsheet as it is now (nm-core's writer).
    pub fn export_xlsx_dialog(&mut self) {
        let stem = self.doc().and_then(|d| d.path.as_ref()).and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
        let Some(out) = file_dialog(|d| d.add_filter("Excel", &["xlsx"]).set_file_name(format!("{stem}.xlsx")).save_file()) else { return };
        self.export_xlsx_to(&out);
    }

    pub fn export_xlsx_to(&mut self, out: &Path) {
        crate::canvas::commit(self);
        let Some(wb) = self.wb.as_ref() else { return };
        match nm_core::xlsx::write_xlsx(wb, out) {
            Ok(n) => self.notify(format!("Exported {n} worksheet{} to {}", if n == 1 { "" } else { "s" }, out.display())),
            Err(e) => self.notify(format!("Export failed: {e}")),
        }
    }

    /// File > Export To > TSV…: a tab-separated file per table, into a folder.
    pub fn export_tsv_dialog(&mut self) {
        let Some(dir) = file_dialog(|d| d.pick_folder()) else { return };
        crate::canvas::commit(self);
        let Some(wb) = self.wb.as_ref() else { return };
        match export_delimited(wb, &dir, '\t') {
            Ok(n) => self.notify(format!("Wrote {n} TSV files to {}", dir.display())),
            Err(e) => self.notify(format!("Export failed: {e}")),
        }
    }

    pub fn export_csv_dialog(&mut self) {
        let Some(path) = self.doc().and_then(|d| d.path.clone()) else {
            self.notify("Save the spreadsheet first.");
            return;
        };
        if let Some(dir) = file_dialog(|d| d.pick_folder()) {
            match export_csv(&path, &dir) {
                Ok(n) => self.notify(format!("Wrote {n} CSV files to {}", dir.display())),
                Err(e) => self.notify(format!("Export failed: {e}")),
            }
        }
    }

    /// NM_SNAP: once the tiles are drawn, capture the window and quit.
    fn snapshot(&mut self, ctx: &egui::Context, settled: bool) {
        let Some((path, frames)) = &mut self.snap else { return };
        *frames += 1;
        let n = *frames;
        let path = path.clone();
        if n == 3 {
            // NM_CELL=row,col[,row2,col2] selects in the sheet's first table; NM_EDIT starts typing
            if let Some(v) = std::env::var("NM_CELL").ok() {
                let p: Vec<u32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                let first = self.drawables.iter().find(|d| matches!(d.kind, DrawableKind::Table(_))).map(|d| d.id);
                if let (Some(t), [r, c, rest @ ..]) = (first, p.as_slice()) {
                    let mut s = Sel::cell(t, *r, *c);
                    if let [r2, c2] = rest {
                        s.cursor = (*r2, *c2);
                    }
                    self.sel = Some(s);
                    self.insp_tab = if std::env::var("NM_INSPECTOR").is_ok() { self.insp_tab } else { InspTab::Cell };
                }
            }
            // NM_CMD=insert-table,add-sheet,undo: menu commands, in order
            for c in std::env::var("NM_CMD").unwrap_or_default().split(',').map(str::trim) {
                match c {
                    "insert-table" => self.insert_table(0),
                    "insert-shape" => self.insert_shape(3),
                    "insert-chart" => self.insert_chart(0, None),
                    "add-sheet" => self.add_sheet(),
                    "insert-text" => self.insert_text_box(),
                    "undo" => self.undo(),
                    "redo" => self.redo(),
                    "content-hub" => self.hub.show_hub(),
                    "" => {}
                    other => eprintln!("NM_CMD: unknown command {other}"),
                }
            }
            if let (Some(text), Some(sel)) = (std::env::var("NM_EDIT").ok(), self.sel) {
                self.canvas.edit = Some(crate::canvas::CellEdit { table: sel.table, row: sel.cursor.0, col: sel.cursor.1, text, focus: true, typed: true, click: None });
            }
        }
        for c in std::mem::take(&mut self.script_cmds) {
            // "image path": insert a picture, movie or sound (checks: the file dialog can't be
            // scripted)
            if let Some(path) = c.strip_prefix("\u{4}") {
                crate::media::insert_path(self, PathBuf::from(path.trim()));
                continue;
            }
            // "cell r c [r2 c2]": select in the sheet's first table (checks), else a menu command
            if let Some(rest) = c.strip_prefix("\u{3}") {
                let p: Vec<u32> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                let first = self.drawables.iter().find(|d| matches!(d.kind, DrawableKind::Table(_))).map(|d| d.id);
                if let (Some(t), [r, c, more @ ..]) = (first, p.as_slice()) {
                    crate::canvas::commit(self);
                    let mut sel = Sel::cell(t, *r, *c);
                    if let [r2, c2] = more {
                        sel.cursor = (*r2, *c2);
                    }
                    self.sel = Some(sel);
                    self.canvas.obj = None;
                }
                continue;
            }
            crate::menus::run(self, ctx, &c);
        }
        if n > 20 && settled && self.script.is_empty() && !self.stocks.busy() && !self.hub.busy() {
            // NM_SAVE=path: the document as the script left it
            if let Some(p) = std::env::var_os("NM_SAVE") {
                crate::canvas::commit(self);
                if let Some(w) = &mut self.wb {
                    match w.doc.save_as(std::path::Path::new(&p)) {
                        Ok(_) => eprintln!("saved to {}", std::path::Path::new(&p).display()),
                        Err(e) => eprintln!("save failed: {e}"),
                    }
                }
            }
            if !self.perf.is_empty() {
                // skip the first frames (startup, first tiles)
                let v: Vec<f32> = self.perf.iter().skip(15).copied().collect();
                let mut sorted = v.clone();
                sorted.sort_by(f32::total_cmp);
                let pick = |q: f32| sorted.get(((sorted.len() as f32 - 1.0) * q) as usize).copied().unwrap_or(0.0);
                eprintln!("perf: {} frames, mean {:.2} ms, p50 {:.2}, p95 {:.2}, max {:.2}", v.len(), v.iter().sum::<f32>() / v.len().max(1) as f32, pick(0.5), pick(0.95), pick(1.0));
                eprintln!("blank tiles seen: {} (zooming), {} (any time)", crate::tiles::BLANKS.with(|b| b.get()), crate::tiles::MISSING.with(|b| b.get()));
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
        } else if n > 900 {
            eprintln!("snapshot timed out");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}


impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // fingers: one scrolls the sheet, a hold is a right click (the iPad's way)
        kn_ui::touch::raw_input(ctx, raw);
        // NM_SCRIPT starts once the window has settled (after NM_CELL / NM_EDIT at frame 3)
        if self.snap.as_ref().is_some_and(|(_, n)| *n >= 12) {
            if let Some(evs) = self.script.pop_front() {
                for e in evs {
                    match e {
                        egui::Event::Paste(t) if t == "\u{1}" => raw.events.push(egui::Event::Paste(self.cell_clip.as_ref().map(|c| c.tsv.clone()).unwrap_or_default())),
                        egui::Event::Text(t) if t.starts_with("\u{2}") => self.script_cmds.push(t[1..].to_string()),
                        e => raw.events.push(e),
                    }
                }
            }
        }
        self.take_tab(raw);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // transparent around the window: its shadow is drawn there
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let t0 = std::time::Instant::now();
        kn_ui::glass::native_blur(frame, kn_ui::glass::translucent_panes());
        let ctx = ui.ctx().clone();
        kn_ui::begin_frame(&ctx);
        kn_ui::scale::apply(&ctx, self.settings.ui_scale);
        kn_ui::form::set_pref(self.settings.ui_layout);
        if self.canvas.editing.is_some() || self.canvas.edit.is_some() {
            kn_ui::typing(&ctx);
        }
        self.pointer_held = ctx.input(|i| i.pointer.any_down() || i.pointer.any_released());
        fit_window_to_screen(&ctx);
        // documents opened from Finder, Open With or the Dock come as an Apple event (patched
        // winit queues them), not on the command line (Keydeck's)
        #[cfg(target_os = "macos")]
        {
            for path in winit::platform::macos::take_opened_files() {
                self.open(&path);
            }
            // (the event may come while the app is idle: look again shortly)
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        // dropped files (on Android, also files picked in the system picker): spreadsheets
        // open, pictures, movies and sounds go on the sheet
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        for p in dropped {
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            use kn_ui::media::{AUDIO_EXT, IMAGE_EXT, MOVIE_EXT};
            if ["numbers", "xlsx", "xlsm", "csv", "tsv", "txt", "nmbtemplate"].contains(&ext.as_str()) {
                self.templates.chooser = false;
                self.open(&p);
            } else if IMAGE_EXT.iter().chain(MOVIE_EXT).chain(AUDIO_EXT).any(|e| *e == ext) {
                crate::media::insert_path(self, p);
            }
        }
        // our own frame (Keydeck's): shadow, rounded corners and outline where the desktop
        // leaves frames to apps
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
        if std::env::var_os("NM_PERF").is_some() {
            self.perf.push(t0.elapsed().as_secs_f32() * 1000.0);
        }
    }
}

impl App {
    fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.refresh();
        if let Some((_, t)) = &mut self.message {
            *t += ctx.input(|i| i.stable_dt) as f64;
            if *t > 4.0 {
                self.message = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.doc().is_some_and(|d| d.dirty) && !self.automated && !self.pending_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending_close = true;
        }
        if let Some((path, hint, typed, wrong)) = &mut self.password {
            let mut done = None;
            egui::Window::new("Password").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(&ctx, |ui| {
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
                Some(true) => match Document::open_with_password(path.as_path(), typed, hint) {
                    Ok(doc) => {
                        self.password = None;
                        self.load(Workbook { doc });
                    }
                    Err(_) => *wrong = true,
                },
                Some(false) => self.password = None,
                None => {}
            }
        }
        if let Some((t, r, c, text, author)) = &mut self.comment_dialog {
            let mut done = None;
            egui::Window::new("Add Comment").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(&ctx, |ui| {
                ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(300.0).hint_text("Comment"));
                ui.horizontal(|ui| {
                    ui.label("Author:");
                    ui.text_edit_singleline(author);
                });
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        done = Some(false);
                    }
                    if ui.add_enabled(!text.trim().is_empty(), egui::Button::new("Add")).clicked() {
                        done = Some(true);
                    }
                });
            });
            match done {
                Some(true) => {
                    let (t, r, c, text, author) = (*t, *r, *c, text.trim().to_string(), author.trim().to_string());
                    self.comment_dialog = None;
                    if let Some(Err(e)) = self.edit("Add Comment", |d, h| nm_core::comments::add_cell_comment(d, h, t, r, c, &text, &author)) {
                        self.notify(format!("Couldn't add the comment: {e}"));
                    }
                }
                Some(false) => self.comment_dialog = None,
                None => {}
            }
        }
        if let Some((pw, again, hint)) = &mut self.set_password {
            let mut done = None;
            let has = self.wb.as_ref().is_some_and(|w| w.doc.password.is_some());
            egui::Window::new("Set Password").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(&ctx, |ui| {
                egui::Grid::new("pw").num_columns(2).show(ui, |ui| {
                    ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(pw).password(true));
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
                    if ui.add_enabled(ok, egui::Button::new("Set Password")).clicked() {
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
        if self.pending_close {
            egui::Window::new("Unsaved changes").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(&ctx, |ui| {
                ui.label("Do you want to save the changes to this spreadsheet?");
                ui.horizontal(|ui| {
                    if ui.button("Don't Save").clicked() {
                        if let Some(w) = &mut self.wb {
                            w.doc.dirty = false;
                        }
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_close = false;
                    }
                    if ui.button("Save").clicked() {
                        self.save(false);
                        self.pending_close = false;
                        if self.doc().is_some_and(|d| !d.dirty) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                });
            });
        }
        let window: egui::Rect = ui.max_rect();
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("nm-window-rect"), window));
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
        // the title bar: window buttons and menus (or the menus alone under the system's)
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
        // Settings > Classic look: the toolbar on a strip of its own; otherwise its buttons
        // float over the panels below the menus (macOS 26, as in Keydeck)
        if classic && self.show_toolbar {
            egui::Panel::top("toolbar").exact_size(tb_h).frame(egui::Frame::new().fill(crate::look::header_bg()).inner_margin(egui::Margin::symmetric(10, 4))).show(ui, |ui| {
                let bar = ui.max_rect();
                crate::toolbar::toolbar(self, ui, bar);
            });
        }
        let float_h = if classic { 0.0 } else { tb_h };
        self.templates.top = top_y + float_h;
        let mut right_w = 0.0;
        let mut insp_top = top_y;
        // the inspector slides in and out, pushing the sheet (Mac N21: ~233–250 ms, ease-in-out)
        // (the phone layout: the inspector is a sheet from the bottom, closed to begin with)
        let compact = kn_ui::form::compact(&ctx);
        if kn_ui::form::switched(&ctx) == Some(true) {
            self.inspector = false;
        }
        if compact && self.inspector {
            kn_ui::form::sheet(ui, "inspector-sheet", theme::PANEL_BG(), |ui| crate::inspector::inspector(self, ui));
        }
        let t = kn_ui::theme::slide(&ctx, egui::Id::unique("inspector-slide"), self.inspector && !compact);
        if t > 0.0 {
            const W: f32 = 300.0;
            let r = egui::Panel::right("inspector")
                // Keydeck's inspector width (its panels are laid out for it)
                .exact_size(W * t)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(if kn_ui::glass::translucent_panes() { Color32::TRANSPARENT } else { theme::PANEL_BG() }).corner_radius(corners([false, false, false, true])))
                .show(ui, |ui| {
                    if kn_ui::glass::translucent_panes() {
                        // (rounded like the window's bottom-right corner)
                        kn_ui::glass::pane_background_rounded(ui, theme::PANEL_BG(), corners([false, false, false, true]));
                    }
                    // laid out full width from the pane's left edge as it moves: the part
                    // past the window's edge is cut off
                    let m = ui.max_rect();
                    let full = egui::Rect::from_min_size(m.min, egui::vec2(W, m.height())).shrink2(egui::vec2(12.0, 0.0));
                    let full = egui::Rect::from_min_max(egui::pos2(full.min.x, full.min.y + 12.0 + float_h), egui::pos2(full.max.x, full.max.y - 12.0));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(full), |ui| crate::inspector::inspector(self, ui));
                })
                .response
                .rect;
            right_w = r.width();
            // the divider runs the panel's whole height, under the floating toolbar too (Keydeck)
            insp_top = r.min.y;
        }
        if float_h > 0.0 {
            egui::Panel::top("toolbar-room").exact_size(float_h).frame(egui::Frame::new().fill(crate::look::header_bg())).show(ui, |_| {});
        }
        // (its line is drawn below, stopping at the inspector: egui's ran on into it)
        let tabs_y = egui::Panel::top("sheettabs").exact_size(42.0).show_separator_line(false).frame(egui::Frame::new().fill(crate::look::tabs_bg())).show(ui, |ui| self.sheet_tabs(ui)).response.rect.max.y;
        if self.find.is_some() {
            egui::Panel::top("findbar").exact_size(40.0).frame(egui::Frame::new().fill(theme::PANEL_BG()).inner_margin(egui::Margin::symmetric(12, 6))).show(ui, |ui| self.find_bar(ui));
        }
        egui::Panel::bottom("cellbar")
            .exact_size(32.0)
            .frame(egui::Frame::new().fill(theme::PANEL_BG()).corner_radius(corners([false, false, true, !self.inspector])).stroke(egui::Stroke::new(1.0, theme::SEPARATOR())))
            .show(ui, |ui| self.cell_bar(ui));
        let mut settled = true;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::WHITE)).show(ui, |ui| {
            settled = crate::canvas::canvas(self, ui);
        });
        // the floating toolbar over the panels' top, and the inspector's divider
        let line = egui::Stroke::new(1.0, theme::SEPARATOR());
        ui.painter().hline(window.min.x..=window.max.x - right_w, tabs_y, line);
        if right_w > 0.0 {
            ui.painter().vline(window.max.x - right_w, insp_top..=window.max.y, line);
        }
        if float_h > 0.0 {
            let strip = egui::Rect::from_min_max(egui::pos2(window.min.x, top_y), egui::pos2(window.max.x, top_y + float_h)).shrink2(egui::vec2(10.0, 4.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(strip).id_salt("floating-toolbar"));
            crate::toolbar::toolbar(self, &mut child, strip);
        }
        // new undo steps were made this frame: on the sheet / cells of the frame before
        let n = self.hist.undo_len();
        if n > self.undo_where.len() {
            self.undo_where.resize(n, self.prev_where);
            if !self.hist.can_redo() {
                self.redo_where.clear();
            }
        } else {
            self.undo_where.truncate(n);
        }
        self.prev_where = (self.sheet, self.sel);
        crate::links::window(self, &ctx);
        crate::stocks::window(self, &ctx);
        crate::toolbar::sheet(self, &ctx);
        crate::toolbar::drag_overlay(self, &ctx);
        self.colors_window(&ctx);
        crate::spelling::window(self, &ctx);
        crate::fileops::versions_window(self, &ctx);
        crate::fonts::fonts_window(self, &ctx);
        crate::fonts::replace_window(self, &ctx);
        crate::fonts::warnings_window(self, &ctx);
        crate::templates::ui(self, &ctx);
        crate::templates::language_window(self, &ctx);
        crate::fileops::print_setup(self, &ctx);
        crate::equation::ui(self, &ctx);
        crate::media::ui(self, &ctx);
        if self.hub.needs_shapes() {
            (self.hub.basic, self.hub.mine) = crate::chrome::hub_shapes();
        }
        if let Some(p) = self.hub.show(&ctx) {
            crate::chrome::insert_from_hub(self, p);
        }
        crate::media::web_and_jobs(self, &ctx);
        crate::chartedit::ui(self, &ctx);
        if self.show_about {
            egui::Window::new("About Values").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).open(&mut self.show_about).show(&ctx, |ui| {
                ui.label(theme::bold("Values"));
                ui.label("Reads and writes Numbers (.numbers) spreadsheets.");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("Looks and settings follow Keydeck.");
                ui.label(RichText::new("Stock quotes come from Yahoo Finance's public endpoint and may stop working.").small().weak());
            });
        }
        crate::menus::shortcuts_window(self, &ctx);
        crate::settings::window(self, &ctx);
        if self.renaming.is_none() && self.comment_dialog.is_none() && self.password.is_none() && self.set_password.is_none() {
            crate::menus::shortcuts(self, &ctx);
        }
        if self.renaming.is_none() {
            crate::canvas::shortcuts(self, &ctx);
        }
        // window title: the document's name, with "Edited" while it has unsaved changes
        let title = format!("{}{} — Values", self.title(), if self.doc().is_some_and(|d| d.dirty) { " (Edited)" } else { "" });
        if ctx.data(|d| d.get_temp::<String>(egui::Id::unique("window-title"))) != Some(title.clone()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique("window-title"), title));
        }
        self.snapshot(&ctx, settled);
    }
}

/// One CSV file per table, named "<sheet> - <table>.csv" (Numbers' own export does the same).
pub fn export_csv(src: &Path, dir: &Path) -> Result<usize, String> {
    let wb = Workbook::open(src).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut n = 0;
    for s in wb.sheets() {
        for t in wb.tables(&s) {
            let Some(td) = wb.table_data(t.info) else { continue };
            let mut out = String::new();
            for r in 0..td.rows {
                let row: Vec<String> = (0..td.cols).map(|c| csv_field(&td.cells[(r * td.cols + c) as usize])).collect();
                out.push_str(&row.join(","));
                out.push('\n');
            }
            let name = format!("{} - {}.csv", s.name, t.name).replace('/', "-");
            std::fs::write(dir.join(name), out).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

/// A file per table of `wb`, fields separated by `sep` ("Sheet - Table.tsv" / ".csv").
pub fn export_delimited(wb: &Workbook, dir: &Path, sep: char) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let ext = if sep == '\t' { "tsv" } else { "csv" };
    let field = |s: &str| if s.contains([sep, '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
    let mut n = 0;
    for s in wb.sheets() {
        for t in wb.tables(&s) {
            let Some(td) = wb.table_data(t.info) else { continue };
            let mut out = String::new();
            for r in 0..td.rows {
                let row: Vec<String> = (0..td.cols).map(|c| field(&td.cells[(r * td.cols + c) as usize])).collect();
                out.push_str(&row.join(&sep.to_string()));
                out.push('\n');
            }
            let name = format!("{} - {}.{ext}", s.name, t.name).replace('/', "-");
            std::fs::write(dir.join(name), out).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// A native file dialog; none while NM_SCRIPT drives the app (a dialog would wait for a person).
pub(crate) fn file_dialog<T>(f: impl FnOnce(kn_ui::files::FileDialog) -> Option<T>) -> Option<T> {
    if std::env::var_os("NM_SCRIPT").is_some() {
        return None;
    }
    f(kn_ui::files::FileDialog::new())
}

/// NM_SCRIPT="click 400 300; dclick 400 300; drag 100 100 300 100; type hello; key Enter; key ctrl+Z; wait"
/// (screen points). Each step becomes a few frames of events.
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
            // a Shift-click
            ("sclick", [x, y]) => {
                let p = Pos2::new(*x, *y);
                let ev = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::SHIFT };
                out.push_back(vec![Event::PointerMoved(p), Event::ModifiersChanged(Modifiers::SHIFT)]);
                out.push_back(vec![ev(true)]);
                out.push_back(vec![ev(false)]);
                out.push_back(vec![Event::ModifiersChanged(Modifiers::NONE)]);
                out.push_back(vec![]);
            }
            ("click", [x, y]) | ("dclick", [x, y]) => {
                let p = Pos2::new(*x, *y);
                out.push_back(vec![Event::PointerMoved(p)]);
                for _ in 0..if cmd == "dclick" { 2 } else { 1 } {
                    out.push_back(vec![button(p, true)]);
                    out.push_back(vec![button(p, false)]);
                }
                out.push_back(vec![]);
            }
            // a drag left held (a snapshot mid-drag): "hold x1 y1 x2 y2"
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
            // ("\\s" is a space: steps are trimmed)
            ("type", _) => {
                out.push_back(vec![Event::Text(rest.replace("\\s", " "))]);
                out.push_back(vec![]);
            }
            ("key", _) => {
                let mut modifiers = Modifiers::NONE;
                let mut name = rest;
                if let Some(k) = name.strip_prefix("ctrl+") {
                    modifiers = Modifiers::COMMAND;
                    name = k;
                }
                if let Some(k) = name.strip_prefix("alt+") {
                    modifiers.alt = true;
                    name = k;
                }
                if let Some(k) = name.strip_prefix("shift+") {
                    modifiers.shift = true;
                    name = k;
                }
                if let Some(key) = Key::from_name(name) {
                    let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
                    // the modifiers are held around the key (egui tracks them from these events)
                    out.push_back(vec![Event::ModifiersChanged(modifiers), ev(true)]);
                    out.push_back(vec![ev(false), Event::ModifiersChanged(Modifiers::NONE)]);
                    out.push_back(vec![]);
                }
            }
            ("scroll", [x, y, dx, dy, n]) => {
                out.push_back(vec![Event::PointerMoved(Pos2::new(*x, *y))]);
                for _ in 0..*n as usize {
                    out.push_back(vec![Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(*dx, *dy),
                        phase: egui::TouchPhase::Move,
                        modifiers: Modifiers::NONE,
                        source: egui::MouseWheelSource::Wheel,
                    }]);
                }
            }
            // a trackpad pinch at x y: n frames, each zooming by factor f
            ("pinch", [x, y, f, n]) => {
                out.push_back(vec![Event::PointerMoved(Pos2::new(*x, *y))]);
                for _ in 0..*n as usize {
                    out.push_back(vec![Event::Zoom(*f)]);
                }
            }
            // our own clipboard pasted (synthetic keys don't reach the system clipboard)
            ("menu", _) => out.push_back(vec![Event::Text(format!("\u{2}{rest}"))]),
            ("cell", _) => out.push_back(vec![Event::Text(format!("\u{2}\u{3}{rest}"))]),
            ("image", _) => out.push_back(vec![Event::Text(format!("\u{2}\u{4}{rest}"))]),
            ("paste", _) => out.push_back(vec![Event::Paste("\u{1}".into())]),
            ("wait", _) => {
                for _ in 0..10 {
                    out.push_back(vec![]);
                }
            }
            _ => eprintln!("NM_SCRIPT: can't read step {step:?}"),
        }
    }
    out
}

fn recent_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("numdeck").join("recent.txt"))
}

/// A cell's value on one line for the bottom bar: line breaks shown as ⏎.
fn one_line(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\n', " ⏎ ")
}

/// `text` with every `q` (any case) replaced by `with`.
fn replace_ci(text: &str, q: &str, with: &str) -> String {
    let lower = text.to_lowercase();
    let ql = q.to_lowercase();
    // (lower-casing can change lengths; then only an exact-case replace is safe)
    if lower.len() != text.len() || ql.is_empty() {
        return text.replace(q, with);
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(k) = lower[i..].find(&ql) {
        out.push_str(&text[i..i + k]);
        out.push_str(with);
        i += k + ql.len();
    }
    out.push_str(&text[i..]);
    out
}

#[cfg(test)]
mod replace_tests {
    #[test]
    fn replaces_any_case() {
        assert_eq!(super::replace_ci("Hello hello HELLO", "hello", "bye"), "bye bye bye");
        assert_eq!(super::replace_ci("abc", "x", "y"), "abc");
        assert_eq!(super::replace_ci("Straße", "ß", "ss"), "Strasse");
    }
}

/// A document kept as an Excel workbook (File > Save writes the .xlsx).
#[derive(Clone, Debug)]
pub struct ExcelMode {
    pub path: PathBuf,
    /// What the cells have that Numbers can't hold (rotation, diagonal lines, links), by sheet
    /// name as the file had them: kept, drawn and written back.
    pub extras: nm_core::xlsx::Extras,
    /// The same by sheet (matched by name once, when the file opened), so a sheet renamed or
    /// moved keeps them when it's saved.
    pub by_sheet: std::collections::HashMap<ObjId, std::collections::HashMap<(u32, u32), nm_core::xlstyle::Extra>>,
    /// Compatibility mode: every Values feature, the Values copy saved inside the .xlsx.
    pub compat: bool,
}

/// Once: the first window no larger than the screen allows (92 % × 86 %), and on macOS and
/// Windows centred a little below the middle, clear of the menu bar (Keydeck's: the Mac opened
/// Values partly off the screen, N25).
fn fit_window_to_screen(ctx: &egui::Context) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.load(std::sync::atomic::Ordering::Relaxed) || std::env::var_os("NM_SNAP").is_some() {
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
