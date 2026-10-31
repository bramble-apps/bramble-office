//! Application state, window layout and command dispatch.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use egui::{Align, Color32, Layout, RichText};
use kn_format::ObjId;
use kn_model::edit::History;
use kn_model::{Document, SlideInfo, SlideScene};
use kn_render::Renderer;

use crate::textures::Textures;
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    Navigator,
    SlideOnly,
    LightTable,
    Outline,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InspectorKind {
    Format,
    Animate,
    Document,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Zoom {
    Fit,
    Percent(f32),
}

/// Background jobs whose results the UI polls each frame (never blocks the UI thread).
enum Job {
    Open(PathBuf, Receiver<Result<Document, String>>, Instant, std::sync::Arc<kn_format::progress::Progress>),
    PickOpen(Receiver<Option<PathBuf>>),
    PickSave(Receiver<Option<PathBuf>>),
    PickImage(Receiver<Option<(String, std::io::Result<Vec<u8>>)>>),
    /// A picture, movie or sound being read / probed for insertion.
    Media(Receiver<Result<crate::media::Picked, String>>),
}

pub struct App {
    pub doc: Option<Document>,
    pub slides: Vec<SlideInfo>,
    pub cur: usize,
    /// Extra selected slides in the navigator (multi-select), always includes `cur`.
    pub slide_sel: Vec<usize>,
    /// The slide navigator was clicked last (Select All / Delete then act on slides, as in Keynote).
    pub nav_focus: bool,
    /// Copied slides: a snapshot of the document, the (slide, node) pairs, and its identity.
    pub slide_clip: Option<(std::sync::Arc<kn_format::KeyFile>, Vec<(ObjId, ObjId)>, String)>,
    pub sel: Vec<ObjId>,
    pub hist: History,
    pub renderer: Renderer,
    pub tex: Textures,
    revs: HashMap<ObjId, u64>,
    rev_counter: u64,
    scenes: HashMap<(ObjId, u64), SlideScene>,
    /// Image galleries: the image each one shows (index into its items).
    pub gallery_pos: HashMap<ObjId, usize>,
    pub view: ViewMode,
    pub inspector: InspectorKind,
    pub show_inspector: bool,
    /// View > Hide Toolbar.
    pub show_toolbar: bool,
    /// Frames left in the cursor refresh after the pointer enters the window (see `cursor_refresh`).
    cursor_nudge: u8,
    /// Width the menus took in the title bar last frame.
    menu_bar_width: Option<f32>,
    /// .pptx files that are PowerPoint + Keydeck (the Keynote document kept inside); other
    /// .pptx files are plain PowerPoint.
    pub(crate) pptx_hybrid: std::collections::HashSet<PathBuf>,
    pointer_was_inside: bool,
    /// View > Show Object List.
    pub show_object_list: bool,
    pub object_list_query: String,
    pub show_notes: bool,
    pub show_rulers: bool,
    pub show_comments: bool,
    /// Editing slide layouts: the slide index to return to.
    pub editing_layouts: Option<usize>,
    /// Comment popup: (drawable, text being edited).
    pub open_comment: Option<(ObjId, String)>,
    pub zoom: Zoom,
    pub play: Option<crate::play::Play>,
    pub canvas: crate::canvas::CanvasState,
    pub insp: crate::inspector::InspectorState,
    pub toast: Option<(String, Instant)>,
    /// Help > Update Video Support running in the background.
    pub video_update: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    jobs: Vec<Job>,
    pub notes_buf: Option<(ObjId, String)>,
    pub show_about: bool,
    pub export: crate::export::ExportState,
    pub chart_edit: Option<crate::chartedit::ChartEdit>,
    pub settings: crate::settings::Settings,
    pub show_settings: bool,
    pub show_goto: bool,
    pub link_dialog: crate::links::LinkDialog,
    goto_buf: String,
    pub show_theme_chooser: bool,
    pub fileops: crate::filemenu::FileOps,
    pub replace_fonts: Option<crate::replacefonts::ReplaceFonts>,
    /// Format > Copy Animation.
    pub copied_animation: Option<Vec<kn_model::buildedit::CopiedBuild>>,
    pub show_help: bool,
    pub find: crate::find::FindState,
    /// Navigator has scrolled to show this slide index.
    pub nav_scrolled_to: Option<usize>,
    /// Slide being dragged in the navigator.
    pub nav_drag: Option<usize>,
    /// Where that drag started (x): dragging right or left indents or outdents the slide.
    pub nav_drag_x: f32,
    /// Text being edited in the Outline view, by storage.
    pub outline_bufs: HashMap<ObjId, String>,
    /// KN_SNAP=<png>: capture the window after a few frames and quit (automated checks).
    snap: Option<(PathBuf, u32)>,
    /// Objects copied with Copy/Cut (in-app clipboard).
    pub clip: Option<kn_model::insert::Clip>,
    /// Attributes picked up by Format ▸ Copy Style.
    pub copied_style: Option<crate::editing::CopiedStyle>,
    /// Font names for the Text inspector (computed once per document).
    pub font_names: Option<Vec<String>>,
    /// Open presentations (Window > tabs). The active one's state lives in the fields above;
    /// its slot here is None.
    pub tabs: Vec<Option<DocTab>>,
    pub active_tab: usize,
    /// Tabs waiting to close; the first one with unsaved changes is asked about.
    closing: Vec<usize>,
    /// Quit (or close the window) once `closing` is done.
    quitting: bool,
    pub spell_win: crate::spellwin::SpellWindow,
    pub webmedia: crate::webmedia::WebMedia,
    /// Insert > Browse Content Hub: openly licensed pictures from Wikimedia Commons.
    pub hub: kn_ui::contenthub::Hub,
    pub cond_win: crate::condwin::CondWindow,
    /// Play > Clear Recording… asks first.
    pub confirm_clear_recording: bool,
    /// View > Show Live Video Preview: the camera shown in live video objects while editing.
    pub live_preview: bool,
    pub live_cam: Option<crate::media::Player>,
    /// Background thumbnail rendering for the navigator.
    pub thumb_worker: Option<crate::textures::ThumbWorker>,
    thumb_worker_pending: bool,
    /// Touchpad momentum scrolling (Linux delivers none): velocity and whether coasting.
    kinetic: (egui::Vec2, bool, Option<f64>),
    /// Copied table cells (their shown values, rows of columns).
    pub cell_clip: Option<Vec<Vec<String>>>,
    /// Dynamic backgrounds: the animated texture, its clock, and Preview Motion in the editor.
    pub motion_tex: crate::textures::MotionTex,
    pub motion_clock: Instant,
    /// Corner radius of the window's own frame now (0: square, e.g. maximized).
    pub corner: f32,
    /// The toolbar's drag-and-drop and layout this frame.
    pub tb: crate::toolbar::TbState,
    /// When the title bar was last pressed (for double-click to maximise).
    pub title_press_at: f64,
    /// Window buttons as the desktop lays them out.
    pub buttons: crate::winframe::ButtonLayout,
    pub preview_motion: bool,
    /// View > Guides: user guides shown / the layout's guides shown; Slide > Copy Guides.
    pub show_guides: bool,
    pub show_layout_guides: bool,
    pub copied_guides: Option<Vec<kn_model::slides::Guide>>,
    /// View > Hide Tab Bar / Show All Tabs.
    pub show_tab_bar: bool,
    pub show_all_tabs: bool,
    pub warnings: crate::warnings::Warnings,
    /// View > Show Adjust Image; Format > Image > Replace (the picture to replace).
    pub show_adjust_image: bool,
    pub replace_target: Option<ObjId>,
    /// Format > Image > Edit Mask: the masked picture whose image moves inside its mask.
    pub mask_edit: Option<ObjId>,
    /// Image description being typed (object, text).
    pub desc_buf: Option<(ObjId, String)>,
    pub tools: crate::toolwins::ToolWindows,
    pub show_pd_custom: bool,
    pub show_customize_toolbar: bool,
    /// View > Zoom > Auto-Center: keep the slide centred when zoomed in.
    pub auto_center: bool,
    /// The slide view's scroll position (for zooming about the pointer).
    pub canvas_scroll: egui::Vec2,
    /// View > Zoom to Selection / Fit Content: the slide area (min x, min y, max x, max y) the
    /// canvas should zoom to and centre next frame.
    pub canvas_focus: Option<[f32; 4]>,
    pub media: crate::mediabrowser::MediaBrowser,
    pub dialogs: crate::versions::Dialogs,
    /// Draw With Pen: the stroke being drawn (Some = pen mode on).
    pub pen: Option<Vec<kn_model::Point>>,
    pub pen_color: Color32,
    pub pen_width: f32,
    /// Make Editable: point editing of a shape.
    pub point_edit: Option<crate::canvas::PointEdit>,
    /// Format > Advanced > Set as Default Text Box Appearance: the shape style new text boxes
    /// get (per presentation: document identity, style).
    pub default_textbox: Option<(Option<PathBuf>, ObjId)>,
    /// The theme chooser is open for File > Change Theme.
    pub changing_theme: bool,
    pub equation: crate::equation::EquationDialog,
}

/// A presentation that isn't the active tab: everything that belongs to one document.
#[derive(Default)]
pub struct DocTab {
    doc: Option<Document>,
    slides: Vec<SlideInfo>,
    cur: usize,
    slide_sel: Vec<usize>,
    sel: Vec<ObjId>,
    hist: History,
    tex: Textures,
    revs: HashMap<ObjId, u64>,
    scenes: HashMap<(ObjId, u64), SlideScene>,
    editing_layouts: Option<usize>,
    open_comment: Option<(ObjId, String)>,
    zoom: Option<Zoom>,
    canvas: crate::canvas::CanvasState,
    notes_buf: Option<(ObjId, String)>,
    chart_edit: Option<crate::chartedit::ChartEdit>,
    outline_bufs: HashMap<ObjId, String>,
    font_names: Option<Vec<String>>,
    untitled_name: Option<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> App {
        theme::apply(&cc.egui_ctx);
        let mut renderer = Renderer::new();
        renderer.enable_async_images();
        let mut app = App {
            doc: None,
            slides: Vec::new(),
            cur: 0,
            slide_sel: vec![0],
            nav_focus: false,
            slide_clip: None,
            sel: Vec::new(),
            hist: History::default(),
            renderer,
            tex: Textures::default(),
            revs: HashMap::new(),
            rev_counter: 1,
            scenes: HashMap::new(),
            gallery_pos: HashMap::new(),
            view: ViewMode::Navigator,
            inspector: InspectorKind::Format,
            show_inspector: true,
            show_notes: true,
            show_rulers: false,
            show_comments: true,
            show_toolbar: crate::settings::Settings::load().toolbar_shown,
            cursor_nudge: 0,
            menu_bar_width: None,
            pptx_hybrid: Default::default(),
            pointer_was_inside: false,
            show_object_list: false,
            object_list_query: String::new(),
            editing_layouts: None,
            open_comment: None,
            zoom: Zoom::Fit,
            play: None,
            canvas: Default::default(),
            insp: Default::default(),
            toast: None,
            video_update: None,
            jobs: Vec::new(),
            notes_buf: None,
            show_about: false,
            export: Default::default(),
            chart_edit: None,
            settings: crate::settings::Settings::load(),
            show_settings: false,
            show_goto: false,
            link_dialog: Default::default(),
            goto_buf: String::new(),
            fileops: Default::default(),
            replace_fonts: None,
            copied_animation: None,
            show_theme_chooser: false,
            show_help: false,
            find: Default::default(),
            nav_scrolled_to: None,
            nav_drag: None,
            nav_drag_x: 0.0,
            outline_bufs: HashMap::new(),
            snap: std::env::var_os("KN_SNAP").map(|p| (PathBuf::from(p), 0)),
            clip: None,
            copied_style: None,
            font_names: None,
            tabs: vec![None],
            active_tab: 0,
            closing: Vec::new(),
            quitting: false,
            spell_win: Default::default(),
            webmedia: Default::default(),
            hub: Default::default(),
            cond_win: Default::default(),
            confirm_clear_recording: false,
            // Keynote shows the camera on the slide while editing
            live_preview: true,
            live_cam: None,
            thumb_worker: None,
            thumb_worker_pending: false,
            kinetic: (egui::Vec2::ZERO, false, None),
            cell_clip: None,
            motion_tex: Default::default(),
            motion_clock: Instant::now(),
            corner: 0.0,
            tb: Default::default(),
            title_press_at: -1.0,
            buttons: crate::winframe::ButtonLayout::from_desktop(),
            preview_motion: true,
            show_guides: true,
            show_layout_guides: false,
            copied_guides: None,
            show_tab_bar: true,
            show_all_tabs: false,
            warnings: Default::default(),
            show_adjust_image: false,
            replace_target: None,
            mask_edit: None,
            desc_buf: None,
            tools: Default::default(),
            show_pd_custom: false,
            show_customize_toolbar: false,
            auto_center: true,
            canvas_scroll: egui::Vec2::ZERO,
            canvas_focus: None,
            media: Default::default(),
            dialogs: Default::default(),
            pen: None,
            pen_color: Color32::from_rgb(0x0a, 0x84, 0xff),
            pen_width: 4.0,
            point_edit: None,
            default_textbox: None,
            changing_theme: false,
            equation: Default::default(),
        };
        if let Some(v) = std::env::var_os("KN_VIEW") {
            app.view = match v.to_str() {
                Some("light") => ViewMode::LightTable,
                Some("outline") => ViewMode::Outline,
                Some("slide") => ViewMode::SlideOnly,
                _ => ViewMode::Navigator,
            };
        }
        crate::settings::apply(&cc.egui_ctx, &app.settings);
        for f in files {
            app.open_path(f);
        }
        app
    }

    // ------------------------------------------------------------------ document state

    pub fn slide_rev(&self, slide: ObjId) -> u64 {
        self.revs.get(&slide).copied().unwrap_or(0)
    }

    /// Mark a slide as changed (re-render thumbnail/canvas, rebuild scene).
    pub fn bump(&mut self, slide: ObjId) {
        self.rev_counter += 1;
        self.revs.insert(slide, self.rev_counter);
    }

    /// Everything may have changed (undo/redo, structural edits).
    /// After the whole file changed (Change Theme, or undoing / redoing it): picture and slide
    /// caches go (data ids may now mean other pictures), everything is redrawn.
    pub fn refresh_after_structural_change(&mut self) {
        self.renderer.clear_caches();
        self.tex.clear();
        self.thumb_worker = None;
        self.thumb_worker_pending = true;
        self.motion_tex = Default::default();
        self.bump_all();
    }

    /// Start the background thumbnail worker for the current document (once per open), and take
    /// the thumbnails it finished.
    fn thumb_worker_tick(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.thumb_worker_pending) {
            if let Some(doc) = self.doc.as_ref() {
                let revs = self.slides.iter().map(|s| (s.slide, self.slide_rev(s.slide))).collect();
                self.thumb_worker = Some(crate::textures::ThumbWorker::start(ctx, doc.file.clone(), self.slides.clone(), revs));
            }
        }
        let Some(w) = self.thumb_worker.as_ref() else { return };
        let done = w.take_results();
        for (slide, img) in done {
            let rev = self.slide_rev(slide);
            if self.thumb_worker.as_ref().and_then(|w| w.revs.get(&slide)) == Some(&rev) {
                self.tex.put_thumb(ctx, slide, rev, img);
            }
        }
    }

    pub fn bump_all(&mut self) {
        self.outline_bufs.clear();
        self.reload_slides();
        let ids: Vec<ObjId> = self.slides.iter().map(|s| s.slide).collect();
        for id in ids {
            self.bump(id);
        }
        self.scenes.clear();
        self.cur = self.cur.min(self.slides.len().saturating_sub(1));
    }

    pub fn scene(&mut self, index: usize) -> Option<SlideScene> {
        let info = self.slides.get(index)?.clone();
        let key = (info.slide, self.slide_rev(info.slide));
        if let Some(s) = self.scenes.get(&key) {
            let s = self.with_gallery_pos(s.clone());
            return Some(self.with_live_cell(s));
        }
        let doc = self.doc.as_ref()?;
        let mut sc = crate::textures::guarded("scene", || doc.scene(&self.slides, index)).flatten()?;
        // table rows grow to fit their lines, as they're drawn (selection and editing follow)
        for d in &mut sc.drawables {
            if let kn_model::DrawableKind::Table(t) = &mut d.kind {
                kn_render::table::fit_rows(&mut self.renderer.fonts, t, None);
            }
        }
        if self.scenes.len() > 400 {
            self.scenes.clear();
        }
        self.scenes.insert(key, sc.clone());
        let sc = self.with_gallery_pos(sc);
        Some(self.with_live_cell(sc))
    }

    /// A table cell being edited with several lines: its text as typed, its row grown to fit
    /// (as Keynote's row grows while typing; the document changes only when the cell is entered).
    fn with_live_cell(&mut self, mut sc: SlideScene) -> SlideScene {
        let Some((id, r, c, text, _)) = self.canvas.cell.as_ref() else { return sc };
        if self.canvas.cell_lines <= 1 {
            return sc;
        }
        if let Some(kn_model::DrawableKind::Table(t)) = sc.drawables.iter_mut().find(|d| d.id == *id).map(|d| &mut d.kind) {
            if let Some(cell) = t.cells.get_mut((r * t.cols + c) as usize) {
                // (a new last line, still empty, counts too)
                *cell = if text.ends_with('\n') { format!("{text} ") } else { text.clone() };
                kn_render::table::fit_rows(&mut self.renderer.fonts, t, None);
            }
        }
        sc
    }

    /// Galleries show the image picked with their arrows (the file's image is the first one).
    fn with_gallery_pos(&self, mut sc: SlideScene) -> SlideScene {
        if self.gallery_pos.is_empty() {
            return sc;
        }
        for (id, pos) in &self.gallery_pos {
            sc.show_gallery_image(*id, *pos);
        }
        sc
    }

    pub fn current_slide(&self) -> Option<&SlideInfo> {
        self.slides.get(self.cur)
    }

    pub fn go_to(&mut self, i: usize) {
        crate::textedit::commit(self);
        if i < self.slides.len() {
            self.cur = i;
            self.slide_sel = vec![i];
            self.sel.clear();
            self.canvas.cancel();
        }
    }

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    /// Notices: web video trouble, the video-support update, and the toast pill (also drawn
    /// over the slideshow).
    fn notices(&mut self, ctx: &egui::Context) {
        // web video trouble and the video-support update, told as notices
        if let Some(e) = crate::media::take_web_error() {
            self.notify(e);
        }
        if let Some(rx) = &self.video_update {
            if let Ok(r) = rx.try_recv() {
                self.video_update = None;
                self.notify(r.unwrap_or_else(|e| e));
            }
        }
        if let Some((msg, at)) = &self.toast {
            if at.elapsed() < Duration::from_secs(if msg.len() > 60 { 8 } else { 4 }) {
                let msg = msg.clone();
                egui::Area::new(egui::Id::unique("toast")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -24.0]).show(ctx, |ui| {
                    egui::Frame::new().fill(Color32::from_black_alpha(220)).corner_radius(8).inner_margin(10).show(ui, |ui| {
                        // light text on the dark pill in either theme
                        ui.label(egui::RichText::new(msg).color(Color32::WHITE));
                    });
                });
                ctx.request_repaint_after(Duration::from_millis(200));
            } else {
                self.toast = None;
            }
        }
    }

    // ------------------------------------------------------------------ files

    pub fn open_path(&mut self, path: PathBuf) {
        self.open_path_with(path, None);
    }

    /// Open, with the password for a protected presentation.
    pub fn open_path_with(&mut self, path: PathBuf, password: Option<(String, String)>) {
        let (tx, rx) = channel();
        let p = path.clone();
        let progress = std::sync::Arc::new(kn_format::progress::Progress::default());
        let prog = progress.clone();
        std::thread::spawn(move || {
            kn_format::progress::watch(Some(prog));
            let r = std::panic::catch_unwind(|| {
                let opened = match &password {
                    _ if crate::pptx::is_pptx(&p) => crate::pptx::open_document(&p),
                    Some((pw, hint)) => Document::open_with_password(&p, pw, hint),
                    None => Document::open(&p),
                };
                opened.map(|mut d| {
                    if let Some(dir) = crate::themes::theme_dir() {
                        d.find_layout_donor(&dir);
                    }
                    d
                })
            })
                .map_err(|_| "the file could not be read (internal error)".to_string())
                .and_then(|r| {
                    r.map_err(|e| match e {
                        // asked for in a password sheet (see versions.rs)
                        kn_format::Error::NeedsPassword(hint) => format!("{PASSWORD_MARK}{hint}"),
                        e => e.to_string(),
                    })
                });
            // KN_SLOW_OPEN (automated checks): take three seconds, to show the Opening window
            if std::env::var_os("KN_SLOW_OPEN").is_some() {
                for i in 0..60 {
                    kn_format::progress::report(i as f32 / 60.0);
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            kn_format::progress::report(1.0);
            kn_format::progress::watch(None);
            let _ = tx.send(r);
        });
        self.jobs.push(Job::Open(path, rx, Instant::now(), progress));
    }

    /// Recently opened files (most recent first), kept in ~/.config/keydeck/recent.txt.
    pub fn recent_files() -> Vec<PathBuf> {
        let Some(p) = recent_path() else { return Vec::new() };
        let all: Vec<PathBuf> = std::fs::read_to_string(p).unwrap_or_default().lines().map(PathBuf::from).collect();
        // whether they still exist is checked on another thread: a file on a network or cloud
        // mount that stops answering must not freeze the window (the menus ask every frame)
        static GONE: std::sync::Mutex<Option<(std::collections::HashSet<PathBuf>, Instant, bool)>> = std::sync::Mutex::new(None);
        let mut g = GONE.lock().unwrap();
        let (gone, checked, busy) = g.get_or_insert_with(|| (Default::default(), Instant::now() - Duration::from_secs(60), false));
        if !*busy && checked.elapsed() > Duration::from_secs(5) {
            *busy = true;
            let list = all.clone();
            std::thread::spawn(move || {
                let missing: std::collections::HashSet<PathBuf> = list.into_iter().filter(|p| !p.exists()).collect();
                if let Some(g) = GONE.lock().unwrap().as_mut() {
                    *g = (missing, Instant::now(), false);
                }
            });
        }
        let gone = gone.clone();
        drop(g);
        all.into_iter().filter(|p| !gone.contains(p)).take(10).collect()
    }

    pub fn remember_path(&self, path: &std::path::Path) {
        App::remember(path);
    }

    fn remember(path: &std::path::Path) {
        // automated checks (KN_SNAP) leave the user's recent list alone
        if std::env::var_os("KN_SNAP").is_some() {
            return;
        }
        let Some(rp) = recent_path() else { return };
        let mut list = App::recent_files();
        list.retain(|p| p != path);
        list.insert(0, path.to_path_buf());
        if let Some(dir) = rp.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text: Vec<String> = list.iter().take(10).map(|p| p.display().to_string()).collect();
        let _ = std::fs::write(rp, text.join("\n"));
    }

    /// Refresh the navigator's list: slides, or layouts while editing slide layouts.
    pub fn reload_slides(&mut self) {
        if let Some(doc) = &self.doc {
            self.slides = if self.editing_layouts.is_some() { doc.layout_infos() } else { doc.slides() };
        }
    }

    /// View > Edit Slide Layouts (toggle).
    pub fn toggle_layout_editing(&mut self) {
        crate::textedit::commit(self);
        self.commit_notes();
        match self.editing_layouts.take() {
            Some(saved) => {
                self.reload_slides();
                self.bump_all();
                self.go_to(saved.min(self.slides.len().saturating_sub(1)));
            }
            None => {
                // start on the layout the current slide uses
                let layout = self.current_slide().and_then(|s| self.doc.as_ref()?.file.obj(s.slide)?.ref_id("template_slide"));
                self.editing_layouts = Some(self.cur);
                self.reload_slides();
                self.bump_all();
                let i = self.slides.iter().position(|s| Some(s.slide) == layout).unwrap_or(0);
                self.go_to(i);
            }
        }
    }

    /// A new, unsaved presentation (File > New).
    pub fn new_document(&mut self, doc: Document) {
        self.make_room();
        self.set_document(doc);
    }

    /// Put `doc` in place of the current presentation (same tab).
    pub fn replace_document(&mut self, doc: Document) {
        self.set_document(doc);
        self.bump_all();
    }

    fn set_document(&mut self, doc: Document) {
        self.editing_layouts = None;
        if let Some(p) = &doc.path {
            App::remember(p);
        }
        self.slides = doc.slides();
        self.doc = Some(doc);
        self.thumb_worker = None;
        self.thumb_worker_pending = true;
        self.cur = 0;
        self.slide_sel = vec![0];
        self.sel.clear();
        self.hist = History::default();
        self.tex.clear();
        self.scenes.clear();
        self.revs.clear();
        self.renderer.clear_caches();
        self.notes_buf = None;
        self.font_names = None;
    }

    // ------------------------------------------------------------------ tabs

    /// Move the active presentation's state out of the app.
    fn stash(&mut self) -> DocTab {
        crate::textedit::commit(self);
        self.commit_notes();
        DocTab {
            doc: self.doc.take(),
            slides: std::mem::take(&mut self.slides),
            cur: std::mem::take(&mut self.cur),
            slide_sel: std::mem::replace(&mut self.slide_sel, vec![0]),
            sel: std::mem::take(&mut self.sel),
            hist: std::mem::take(&mut self.hist),
            tex: std::mem::take(&mut self.tex),
            revs: std::mem::take(&mut self.revs),
            scenes: std::mem::take(&mut self.scenes),
            editing_layouts: self.editing_layouts.take(),
            open_comment: self.open_comment.take(),
            zoom: Some(self.zoom),
            canvas: std::mem::take(&mut self.canvas),
            notes_buf: self.notes_buf.take(),
            chart_edit: self.chart_edit.take(),
            outline_bufs: std::mem::take(&mut self.outline_bufs),
            font_names: self.font_names.take(),
            untitled_name: self.fileops.untitled_name.take(),
        }
    }

    fn restore(&mut self, t: DocTab) {
        self.doc = t.doc;
        self.slides = t.slides;
        self.cur = t.cur;
        self.slide_sel = t.slide_sel;
        self.sel = t.sel;
        self.hist = t.hist;
        self.tex = t.tex;
        self.revs = t.revs;
        self.scenes = t.scenes;
        self.editing_layouts = t.editing_layouts;
        self.open_comment = t.open_comment;
        if let Some(z) = t.zoom {
            self.zoom = z;
        }
        self.canvas = t.canvas;
        self.notes_buf = t.notes_buf;
        self.chart_edit = t.chart_edit;
        self.outline_bufs = t.outline_bufs;
        self.font_names = t.font_names;
        self.fileops.untitled_name = t.untitled_name;
        // object caches are keyed by object ids, which differ between documents
        self.renderer.clear_caches();
        self.nav_scrolled_to = None;
        self.nav_drag = None;
    }

    /// Switch to tab `i`.
    pub fn select_tab(&mut self, ctx: &egui::Context, i: usize) {
        if i == self.active_tab || i >= self.tabs.len() {
            return;
        }
        let t = self.stash();
        self.tabs[self.active_tab] = Some(t);
        let next = self.tabs[i].take().unwrap_or_default();
        self.active_tab = i;
        self.restore(next);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.window_title()));
    }

    /// A new, empty tab becomes active (documents opened from now on go into it).
    pub fn new_tab(&mut self) {
        let t = self.stash();
        self.tabs[self.active_tab] = Some(t);
        self.tabs.push(None);
        self.active_tab = self.tabs.len() - 1;
        self.restore(DocTab::default());
        self.zoom = if self.settings.fit_zoom { Zoom::Fit } else { self.zoom };
    }

    /// Close tab `i` (the window stays, empty, when it was the last one).
    pub fn close_tab(&mut self, ctx: &egui::Context, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        if i == self.active_tab {
            if self.tabs.len() == 1 {
                self.restore(DocTab::default());
                return;
            }
            let _ = self.stash();
            self.tabs.remove(i);
            let n = i.min(self.tabs.len() - 1);
            let next = self.tabs[n].take().unwrap_or_default();
            self.active_tab = n;
            self.restore(next);
        } else {
            self.tabs.remove(i);
            if i < self.active_tab {
                self.active_tab -= 1;
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.window_title()));
    }

    /// Quit Keynote: close every tab (asking about unsaved changes), then the window.
    pub fn quit(&mut self) {
        self.quitting = true;
        self.request_close((0..self.tabs.len()).collect());
    }

    /// Nothing unsaved is left open.
    fn quitting_done(&self, _ctx: &egui::Context) -> bool {
        !(0..self.tabs.len()).any(|i| self.tab_dirty(i))
    }

    /// macOS-style tab bar: equal-width tabs across the window, the active one lighter, a close
    /// button on hover, "+" for a new presentation.
    /// The classic tab bar (Settings > Classic look): square tabs edge to edge.
    fn tab_bar_classic(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        let plus_w = 28.0;
        let n = self.tabs.len();
        let w = (full.width() - plus_w) / n as f32;
        let p = ui.painter().clone();
        let mut select = None;
        let mut close = None;
        for i in 0..n {
            let r = egui::Rect::from_min_size(full.min + egui::vec2(i as f32 * w, 0.0), egui::vec2(w, full.height()));
            let resp = ui.interact(r, egui::Id::unique(("doc-tab", i)), egui::Sense::click());
            let active = i == self.active_tab;
            let fill = if active { theme::TOOLBAR_BG() } else if resp.hovered() { theme::gray(0x26) } else { theme::gray(0x1c) };
            p.rect_filled(r, 0.0, fill);
            if i > 0 {
                p.line_segment([r.left_top() + egui::vec2(0.0, 5.0), r.left_bottom() - egui::vec2(0.0, 5.0)], egui::Stroke::new(1.0, theme::gray(0x3a)));
            }
            let dirty = self.tab_dirty(i);
            let name = format!("{}{}", self.tab_name(i), if dirty { " \u{2014} Edited" } else { "" });
            let col = if active { theme::TEXT() } else { theme::TEXT_DIM() };
            crate::theme::centered(&p, r.center(), name, egui::FontId::proportional(12.0), col);
            // close button, left, on hover
            let cr = egui::Rect::from_center_size(r.left_center() + egui::vec2(14.0, 0.0), egui::vec2(16.0, 16.0));
            let ch = ui.interact(cr, egui::Id::unique(("doc-tab-x", i)), egui::Sense::click());
            if resp.hovered() || ch.hovered() {
                if ch.hovered() {
                    p.rect_filled(cr, 3.0, theme::gray(0x44));
                }
                let k = 4.0;
                let st = egui::Stroke::new(1.2, theme::TEXT_DIM());
                p.line_segment([cr.center() - egui::vec2(k, k), cr.center() + egui::vec2(k, k)], st);
                p.line_segment([cr.center() + egui::vec2(-k, k), cr.center() + egui::vec2(k, -k)], st);
            }
            if ch.clicked() {
                close = Some(i);
            } else if resp.clicked() || resp.middle_clicked() {
                if resp.middle_clicked() {
                    close = Some(i);
                } else {
                    select = Some(i);
                }
            }
        }
        let pr = egui::Rect::from_min_size(egui::pos2(full.max.x - plus_w, full.min.y), egui::vec2(plus_w, full.height()));
        let pl = ui.interact(pr, egui::Id::unique("doc-tab-plus"), egui::Sense::click()).on_hover_text("New Presentation");
        if pl.hovered() {
            p.rect_filled(pr, 0.0, theme::gray(0x26));
        }
        let st = egui::Stroke::new(1.3, theme::TEXT_DIM());
        p.line_segment([pr.center() - egui::vec2(5.0, 0.0), pr.center() + egui::vec2(5.0, 0.0)], st);
        p.line_segment([pr.center() - egui::vec2(0.0, 5.0), pr.center() + egui::vec2(0.0, 5.0)], st);
        if pl.clicked() {
            self.show_theme_chooser = true;
        }
        let ctx = ui.ctx().clone();
        if let Some(i) = select {
            self.select_tab(&ctx, i);
        }
        if let Some(i) = close {
            self.request_close(vec![i]);
        }
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        // Keynote 15.4 on macOS 27 (R35): a rounded bar (dark #353535 / light #E8E8E8), the
        // selected tab a lighter capsule, a + circle at the right
        let full = ui.max_rect().shrink2(egui::vec2(0.0, 2.0));
        let light = theme::is_light();
        let plus = egui::Rect::from_center_size(egui::pos2(full.max.x - full.height() / 2.0, full.center().y), egui::vec2(full.height(), full.height()));
        let bar = egui::Rect::from_min_max(full.min, egui::pos2(plus.min.x - 6.0, full.max.y));
        let n = self.tabs.len();
        let w = bar.width() / n as f32;
        let p = ui.painter().clone();
        let r = bar.height() / 2.0;
        let glass = crate::glass::enabled();
        if glass {
            // liquid glass, as macOS 26's tab bars (the selected tab a lighter pill on it)
            for sh in crate::glass::shapes(ui.ctx(), bar, r, true) {
                p.add(sh);
            }
        } else {
            p.rect_filled(bar, r, if light { theme::tint(Color32::from_rgb(0xe8, 0xe8, 0xe8)) } else { theme::tint(Color32::from_rgb(0x35, 0x35, 0x35)) });
        }
        let mut select = None;
        let mut close = None;
        for i in 0..n {
            let t = egui::Rect::from_min_size(bar.min + egui::vec2(i as f32 * w, 0.0), egui::vec2(w, bar.height()));
            let resp = ui.interact(t, egui::Id::unique(("doc-tab", i)), egui::Sense::click());
            let active = i == self.active_tab;
            if active && glass {
                let pill = t.shrink(3.0);
                p.rect_filled(pill, r - 3.0, if light { Color32::from_white_alpha(150) } else { Color32::from_white_alpha(26) });
                p.rect_stroke(pill, r - 3.0, egui::Stroke::new(0.5, Color32::from_white_alpha(if light { 90 } else { 40 })), egui::StrokeKind::Inside);
            } else if active {
                p.rect_filled(t.shrink(2.0), r - 2.0, if light { theme::tint(Color32::WHITE) } else { theme::tint(Color32::from_rgb(0x4a, 0x4a, 0x4a)) });
            } else if resp.hovered() {
                p.rect_filled(t.shrink(2.0), r - 2.0, if light { Color32::from_black_alpha(10) } else { Color32::from_white_alpha(8) });
            }
            if i > 0 && !active && i != self.active_tab + 1 && !glass {
                p.vline(t.min.x, t.y_range().shrink(6.0), egui::Stroke::new(1.0, if light { theme::tint(Color32::from_gray(0xc8)) } else { theme::tint(Color32::from_gray(0x48)) }));
            }
            let dirty = self.tab_dirty(i);
            let name = format!("{}{}", self.tab_name(i), if dirty { " \u{2014} Edited" } else { "" });
            let col = if active { theme::TEXT() } else { theme::TEXT_DIM() };
            crate::theme::centered(&p, t.center(), name, egui::FontId::proportional(12.0), col);
            // close button, left, on hover
            let cr = egui::Rect::from_center_size(t.left_center() + egui::vec2(14.0, 0.0), egui::vec2(16.0, 16.0));
            let ch = ui.interact(cr, egui::Id::unique(("doc-tab-x", i)), egui::Sense::click());
            if resp.hovered() || ch.hovered() {
                if ch.hovered() {
                    p.circle_filled(cr.center(), 8.0, theme::gray(0x44));
                }
                let k = 3.5;
                let st = egui::Stroke::new(1.2, theme::TEXT_DIM());
                p.line_segment([cr.center() - egui::vec2(k, k), cr.center() + egui::vec2(k, k)], st);
                p.line_segment([cr.center() + egui::vec2(-k, k), cr.center() + egui::vec2(k, -k)], st);
            }
            if ch.clicked() {
                close = Some(i);
            } else if resp.clicked() || resp.middle_clicked() {
                if resp.middle_clicked() {
                    close = Some(i);
                } else {
                    select = Some(i);
                }
            }
        }
        let pl = ui.interact(plus, egui::Id::unique("doc-tab-plus"), egui::Sense::click()).on_hover_text("New Presentation");
        if glass {
            for sh in crate::glass::shapes(ui.ctx(), plus, plus.width() / 2.0, true) {
                p.add(sh);
            }
            if pl.hovered() {
                p.circle_filled(plus.center(), plus.width() / 2.0 - 2.0, Color32::from_white_alpha(if light { 90 } else { 18 }));
            }
        } else {
            p.circle_filled(plus.center(), plus.width() / 2.0, if pl.hovered() { theme::gray(0x48) } else if light { theme::tint(Color32::from_rgb(0xe8, 0xe8, 0xe8)) } else { theme::tint(Color32::from_rgb(0x35, 0x35, 0x35)) });
        }
        let st = egui::Stroke::new(1.4, theme::TEXT());
        p.line_segment([plus.center() - egui::vec2(5.0, 0.0), plus.center() + egui::vec2(5.0, 0.0)], st);
        p.line_segment([plus.center() - egui::vec2(0.0, 5.0), plus.center() + egui::vec2(0.0, 5.0)], st);
        if pl.clicked() {
            self.show_theme_chooser = true;
        }
        let ctx = ui.ctx().clone();
        if let Some(i) = select {
            self.select_tab(&ctx, i);
        }
        if let Some(i) = close {
            self.request_close(vec![i]);
        }
    }

    /// View > Show All Tabs: every open presentation as a card; click one to go to it.
    fn all_tabs_ui(&mut self, ctx: &egui::Context) {
        if !self.show_all_tabs {
            return;
        }
        let mut pick = None;
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::Modal::new(egui::Id::unique("all-tabs")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(theme::bold("All Tabs").size(16.0));
            ui.add_space(8.0);
            egui::Grid::new("all-tabs-grid").spacing([12.0, 12.0]).show(ui, |ui| {
                for i in 0..self.tabs.len() {
                    let name = self.tab_name(i);
                    let b = egui::Button::new(theme::bold(&name)).min_size(egui::vec2(170.0, 100.0)).selected(i == self.active_tab);
                    if ui.add(b).clicked() {
                        pick = Some(i);
                    }
                    if i % 3 == 2 {
                        ui.end_row();
                    }
                }
            });
            ui.add_space(8.0);
            if ui.button("Done").clicked() {
                close = true;
            }
        });
        if let Some(i) = pick {
            self.select_tab(ctx, i);
            close = true;
        }
        if close {
            self.show_all_tabs = false;
        }
    }

    /// Close these tabs, asking about unsaved changes one by one.
    pub fn request_close(&mut self, mut tabs: Vec<usize>) {
        tabs.sort();
        tabs.dedup();
        self.closing = tabs;
    }

    fn tab_dirty(&self, i: usize) -> bool {
        if i == self.active_tab {
            self.doc.as_ref().is_some_and(|d| d.dirty)
        } else {
            self.tabs.get(i).and_then(|t| t.as_ref()).and_then(|t| t.doc.as_ref()).is_some_and(|d| d.dirty)
        }
    }

    /// Work through `closing`: clean tabs close at once, a changed one shows Keynote's
    /// "Do you want to save the changes…" sheet.
    fn closing_ui(&mut self, ctx: &egui::Context) {
        // the window's close button / the desktop asking us to close (automated screenshot
        // runs just quit)
        if ctx.input(|i| i.viewport().close_requested()) && self.snap.is_none() && !self.quitting_done(ctx) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.quit();
        }
        if self.quitting && self.closing.is_empty() {
            if self.quitting_done(ctx) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.quitting = false; // cancelled
            }
        }
        while let Some(&i) = self.closing.last() {
            if i >= self.tabs.len() {
                self.closing.pop();
                continue;
            }
            if self.tab_dirty(i) {
                break;
            }
            self.closing.pop();
            self.close_tab(ctx, i);
        }
        let Some(&i) = self.closing.last() else { return };
        self.select_tab(ctx, i);
        let name = self.tab_name(i);
        let mut choice = None;
        egui::Modal::new(egui::Id::unique("save-changes")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(theme::bold(&format!("Do you want to save the changes made to the document \u{201c}{name}\u{201d}?")));
            ui.add_space(4.0);
            ui.label("Your changes will be lost if you don\u{2019}t save them.");
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Don\u{2019}t Save").clicked() {
                    choice = Some(0);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("Save").fill(theme::ACCENT())).clicked() {
                        choice = Some(2);
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        choice = Some(1);
                    }
                });
            });
        });
        match choice {
            Some(0) => {
                self.closing.pop();
                self.close_tab(ctx, i);
            }
            Some(1) => {
                self.closing.clear();
                self.quitting = false;
            }
            Some(_) => {
                // saved (or a save dialog opened): close once it's no longer dirty
                self.save();
                if !self.tab_dirty(i) {
                    self.closing.pop();
                    self.close_tab(ctx, i);
                } else {
                    self.closing.clear();
                }
            }
            None => {}
        }
    }

    /// Name shown on tab `i`.
    pub fn tab_name(&self, i: usize) -> String {
        let (doc, untitled) = if i == self.active_tab {
            (self.doc.as_ref(), self.fileops.untitled_name.as_ref())
        } else {
            let t = self.tabs.get(i).and_then(|t| t.as_ref());
            (t.and_then(|t| t.doc.as_ref()), t.and_then(|t| t.untitled_name.as_ref()))
        };
        match (doc.and_then(|d| d.path.as_ref()), untitled) {
            (_, Some(n)) => n.clone(),
            (Some(p), None) => p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            (None, None) if doc.is_some() => "Untitled".into(),
            _ => "New Tab".into(),
        }
    }

    pub fn window_title(&self) -> String {
        match self.doc.as_ref().and_then(|d| d.path.as_ref()) {
            Some(p) if self.fileops.untitled_name.is_none() => title_for(p),
            _ => match &self.fileops.untitled_name {
                Some(n) => format!("{n} — Keydeck"),
                None if self.doc.is_some() => "Untitled — Keydeck".into(),
                None => "Keydeck".into(),
            },
        }
    }

    /// Before putting a newly opened/created presentation in: open it in a new tab when this
    /// one already shows a presentation (Keynote with "Prefer tabs" on).
    fn make_room(&mut self) {
        if self.doc.is_some() {
            self.new_tab();
        }
    }

    pub fn pick_open(&mut self) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let f = kn_ui::files::FileDialog::new().add_filter("Presentations", &["key", "pptx"]).add_filter("Keynote Presentation", &["key"]).add_filter("PowerPoint", &["pptx"]).set_title("Open").pick_file();
            let _ = tx.send(f);
        });
        self.jobs.push(Job::PickOpen(rx));
    }

    /// Insert ▸ Choose File / toolbar Media: pick a picture (read on a background thread).
    /// Insert > Choose…: a picture, movie or sound.
    pub fn pick_media(&mut self) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut all: Vec<&str> = crate::media::IMAGE_EXT.to_vec();
            all.extend(crate::media::MOVIE_EXT);
            all.extend(crate::media::AUDIO_EXT);
            let f = kn_ui::files::FileDialog::new()
                .add_filter("Pictures, Movies and Sounds", &all)
                .add_filter("Pictures", crate::media::IMAGE_EXT)
                .add_filter("Movies", crate::media::MOVIE_EXT)
                .add_filter("Sounds", crate::media::AUDIO_EXT)
                .set_title("Insert")
                .pick_file();
            if let Some(p) = f {
                let _ = tx.send(crate::media::prepare(&p));
            }
        });
        self.jobs.push(Job::Media(rx));
    }

    /// Insert a file from disk (media browsers, recordings): read on a background thread.
    pub fn insert_media_path(&mut self, path: PathBuf) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::media::prepare(&path));
        });
        self.jobs.push(Job::Media(rx));
    }

    pub fn pick_image(&mut self) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let f = kn_ui::files::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg"]).set_title("Insert Picture").pick_file();
            let _ = tx.send(f.map(|p| (p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), std::fs::read(&p))));
        });
        self.jobs.push(Job::PickImage(rx));
    }

    pub fn save(&mut self) {
        let path = self.doc.as_ref().and_then(|d| d.path.clone());
        match path {
            Some(p) if p.is_file() => self.save_to(p),
            _ => self.pick_save(),
        }
    }

    pub fn pick_save(&mut self) {
        let name = self
            .doc
            .as_ref()
            .and_then(|d| d.path.as_ref())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .or_else(|| self.fileops.untitled_name.as_ref().map(|n| format!("{n}.key")))
            .unwrap_or_else(|| "Untitled.key".into());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let f = if name.to_ascii_lowercase().ends_with(".pptx") {
                kn_ui::files::FileDialog::new().add_filter("PowerPoint", &["pptx"]).add_filter("Keynote Presentation", &["key"]).set_file_name(&name).save_file()
            } else {
                kn_ui::files::FileDialog::new().add_filter("Keynote Presentation", &["key"]).add_filter("PowerPoint", &["pptx"]).set_file_name(&name).save_file()
            };
            let _ = tx.send(f);
        });
        self.jobs.push(Job::PickSave(rx));
    }

    pub(crate) fn save_to(&mut self, mut p: PathBuf) {
        if p.extension().is_none() {
            p.set_extension("key");
        }
        self.commit_notes();
        // keep the file being saved over as a version (File > Revert To)
        crate::versions::keep_version(&p);
        let Some(doc) = self.doc.as_mut() else { return };
        doc.end(&mut self.hist);
        // PowerPoint (plain, or with the Keynote document inside)
        if crate::pptx::is_pptx(&p) {
            let hybrid = self.pptx_hybrid.contains(&p);
            let r = &mut self.renderer;
            let res = crate::textures::guarded("save", || {
                let key = if hybrid { Some(crate::pptx::key_bytes(doc)?) } else { None };
                crate::pptx::write(doc, r, &p, key)
            });
            match res {
                Some(Ok(())) => {
                    doc.path = Some(p.clone());
                    doc.dirty = false;
                    self.remember_path(&p);
                    self.notify(format!("Saved {} ({})", p.display(), if hybrid { "PowerPoint + Keydeck" } else { "PowerPoint" }));
                }
                Some(Err(e)) => self.notify(format!("Couldn't save: {e}")),
                None => self.notify("Couldn't save: internal error"),
            }
            return;
        }
        match crate::textures::guarded("save", || doc.save_as(&p)) {
            Some(Ok(())) => self.notify(format!("Saved {}", p.display())),
            Some(Err(e)) => self.notify(format!("Couldn't save: {e}")),
            None => self.notify("Couldn't save: internal error"),
        }
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        let mut keep = Vec::new();
        for job in std::mem::take(&mut self.jobs) {
            match job {
                Job::Open(path, rx, started, progress) => match rx.try_recv() {
                    Ok(Ok(mut doc)) => {
                        // PowerPoint: which kind, and whether its Keydeck copy was out of date
                        if crate::pptx::is_pptx(&path) {
                            doc.path = Some(path.clone());
                            match crate::pptx::mode(&path) {
                                Some((true, current)) => {
                                    self.pptx_hybrid.insert(path.clone());
                                    if !current {
                                        self.notify("This file was changed in another app: opened its PowerPoint slides (the Keydeck copy inside was older)");
                                    }
                                }
                                _ => {
                                    self.pptx_hybrid.remove(&path);
                                }
                            }
                        }
                        // File > Duplicate opened us on a temporary copy: show it as untitled
                        let untitled = std::env::var("KN_UNTITLED").ok().filter(|_| path.starts_with(dirs_cache_duplicates()));
                        if untitled.is_some() {
                            doc.path = None;
                        }
                        let autoplay = doc.show_settings().auto_play_on_open;
                        // already open: show its tab
                        let already = (0..self.tabs.len()).find(|&i| {
                            let p = if i == self.active_tab { self.doc.as_ref() } else { self.tabs[i].as_ref().and_then(|t| t.doc.as_ref()) };
                            p.and_then(|d| d.path.as_ref()) == Some(&path)
                        });
                        if let Some(i) = already.filter(|_| untitled.is_none()) {
                            self.select_tab(ctx, i);
                            continue;
                        }
                        self.make_room();
                        self.set_document(doc);
                        if autoplay {
                            // Document > Automatically play upon open
                            self.start_slideshow(0);
                        }
                        let title = untitled.as_ref().map(|n| format!("{n} — Keydeck")).unwrap_or_else(|| title_for(&path));
                        self.fileops.untitled_name = untitled;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
                    }
                    Ok(Err(e)) if e.starts_with(PASSWORD_MARK) => {
                        let hint = e[PASSWORD_MARK.len()..].to_string();
                        self.dialogs.password_prompt = Some(crate::versions::PasswordPrompt::new(path, hint));
                    }
                    Ok(Err(_)) if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) => {}
                    Ok(Err(e)) => self.notify(format!("Couldn't open {}: {e}", path.display())),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        // Keynote's "Opening…" panel, once opening takes a moment (about a second)
                        if started.elapsed() > Duration::from_millis(1000) {
                            opening_window(ctx, &path, &progress);
                        }
                        ctx.request_repaint_after(Duration::from_millis(50));
                        keep.push(Job::Open(path, rx, started, progress));
                    }
                    Err(_) => self.notify("Opening failed"),
                },
                Job::PickOpen(rx) => match rx.try_recv() {
                    Ok(Some(p)) => self.open_path(p),
                    Ok(None) => {}
                    Err(std::sync::mpsc::TryRecvError::Empty) => keep.push(Job::PickOpen(rx)),
                    Err(_) => {}
                },
                Job::Media(rx) => match rx.try_recv() {
                    Ok(Ok(crate::media::Picked::Image(name, bytes))) => crate::editing::insert_object(self, kn_model::insert::NewObject::Image(bytes, name)),
                    Ok(Ok(crate::media::Picked::Movie(m))) => {
                        let Some(slide) = self.current_slide().map(|s| s.slide) else { continue };
                        let label = if m.audio_only { "Insert Audio" } else { "Insert Movie" };
                        let mut res = None;
                        if let Some(doc) = self.doc.as_mut() {
                            doc.begin(&mut self.hist, label);
                            res = Some(doc.insert_movie(&mut self.hist, slide, m));
                            doc.end(&mut self.hist);
                        }
                        match res {
                            Some(Ok(id)) => {
                                self.sel = vec![id];
                                self.bump(slide);
                            }
                            Some(Err(e)) => self.notify(format!("Couldn't insert: {e}")),
                            None => {}
                        }
                    }
                    Ok(Err(e)) => self.notify(e),
                    Err(std::sync::mpsc::TryRecvError::Empty) => keep.push(Job::Media(rx)),
                    Err(_) => {}
                },
                Job::PickImage(rx) => match rx.try_recv() {
                    Ok(Some((name, Ok(bytes)))) => match self.replace_target.take() {
                        // Format > Image > Replace
                        Some(id) => {
                            let slide = self.current_slide().map(|s| s.slide);
                            let mut err = None;
                            if let Some(doc) = self.doc.as_mut() {
                                doc.begin(&mut self.hist, "Replace Image");
                                err = doc.replace_image(&mut self.hist, id, bytes, &name).err();
                                doc.end(&mut self.hist);
                            }
                            if let Some(e) = err {
                                self.notify(format!("Couldn't replace the picture: {e}"));
                            }
                            if let Some(s) = slide {
                                self.bump(s);
                            }
                        }
                        None => crate::editing::insert_object(self, kn_model::insert::NewObject::Image(bytes, name)),
                    },
                    Ok(Some((_, Err(e)))) => self.notify(format!("Couldn't read the picture: {e}")),
                    Ok(None) => self.replace_target = None,
                    Err(std::sync::mpsc::TryRecvError::Empty) => keep.push(Job::PickImage(rx)),
                    Err(_) => {}
                },
                Job::PickSave(rx) => match rx.try_recv() {
                    Ok(Some(p)) => {
                        let p2 = p.clone();
                        self.save_to(p);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title_for(&p2)));
                    }
                    Ok(None) => {}
                    Err(std::sync::mpsc::TryRecvError::Empty) => keep.push(Job::PickSave(rx)),
                    Err(_) => {}
                },
            }
        }
        self.jobs.extend(keep);
        if !self.jobs.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    pub fn loading(&self) -> bool {
        self.jobs.iter().any(|j| matches!(j, Job::Open(..)))
    }

    // ------------------------------------------------------------------ editing helpers

    pub fn commit_notes(&mut self) {
        let Some((slide, text)) = self.notes_buf.take() else { return };
        let Some(doc) = self.doc.as_mut() else { return };
        if doc.notes_storage(slide).map(|s| doc.storage_text(s)).unwrap_or_default() != text {
            doc.begin(&mut self.hist, "Edit Presenter Notes");
            doc.set_notes(&mut self.hist, slide, &text);
            doc.end(&mut self.hist);
        }
    }

    pub fn undo(&mut self) {
        self.commit_notes();
        crate::textedit::commit(self);
        let whole = self.hist.undo_label() == Some("Change Theme");
        if let Some(doc) = self.doc.as_mut() {
            if doc.undo(&mut self.hist) {
                if whole {
                    self.refresh_after_structural_change();
                } else {
                    self.bump_all();
                }
            }
        }
    }

    pub fn redo(&mut self) {
        let whole = self.hist.redo_label() == Some("Change Theme");
        if let Some(doc) = self.doc.as_mut() {
            if doc.redo(&mut self.hist) {
                if whole {
                    self.refresh_after_structural_change();
                } else {
                    self.bump_all();
                }
            }
        }
    }

    /// Slide groups are a flat list with an indent level per slide (Keynote, Mac round 9), so
    /// adding, duplicating and deleting slides work in indented decks too.
    pub fn blocked_by_slide_groups(&mut self) -> bool {
        false
    }

    pub fn start_slideshow(&mut self, from: usize) {
        self.start_play(from, false);
    }

    /// Play the slideshow (Play > In Window / In Fullscreen), or rehearse it: presenter display only.
    pub fn start_play(&mut self, from: usize, rehearse: bool) {
        self.commit_notes();
        if self.doc.is_some() && !self.slides.is_empty() {
            // the show needs every picture at once (no grey stand-ins while they decode)
            self.renderer.set_async_images(false);
            self.play = Some(crate::play::Play::new(from, self.settings.play_in_window, rehearse, self.settings.presenter_in_window));
            // fetch every web video in the deck now, so none keeps the audience waiting
            if let Some(doc) = &self.doc {
                for i in 0..self.slides.len() {
                    if let Some(sc) = doc.scene(&self.slides, i) {
                        for d in &sc.drawables {
                            if let kn_model::DrawableKind::Image(im) = &d.kind {
                                if let Some(w) = &im.web_video {
                                    let _ = crate::media::web_stream(&w.url);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------ layout

    /// Keynote 15.4's toolbar (from its screenshots): View (sidebar) button and the document name,
    /// then Play, Add Slide, the insert group (Table, Chart, Text, Shape, Media), Zoom, Comment,
    /// Comment and the inspector group (Format, Animate, Document). Icons only, on capsules.
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        crate::toolbar::ui(self, ui);
    }

    /// Slide > Go To > Slide…
    fn goto_window(&mut self, ctx: &egui::Context) {
        if !self.show_goto {
            return;
        }
        let mut open = true;
        let mut go = None;
        egui::Window::new("Go to Slide").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 120.0]).open(&mut open).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Slide (1–{}):", self.slides.len()));
                let r = ui.add(egui::TextEdit::singleline(&mut self.goto_buf).desired_width(60.0));
                r.request_focus();
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    go = self.goto_buf.trim().parse::<usize>().ok();
                }
            });
        });
        if let Some(n) = go {
            self.go_to(n.saturating_sub(1).min(self.slides.len().saturating_sub(1)));
            open = false;
        }
        self.show_goto = open;
    }

    /// Add Slide popover: the theme's layouts as thumbnails.
    pub(crate) fn layout_picker(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = self.doc.as_ref() else { return };
        let layouts = doc.layouts();
        let aspect = doc.slide_size().h / doc.slide_size().w;
        let mut budget = crate::textures::Budget::new(30);
        let mut pick = None;
        ui.set_max_width(560.0);
        egui::Grid::new("layouts").spacing([10.0, 10.0]).show(ui, |ui| {
            for (i, (id, name)) in layouts.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(120.0, 120.0 * aspect + 18.0), egui::Sense::click());
                let img = egui::Rect::from_min_size(rect.min, egui::vec2(120.0, 120.0 * aspect));
                let ctx = ui.ctx().clone();
                let doc = self.doc.as_ref().unwrap();
                let tex = self.tex.thumb(&ctx, &mut self.renderer, doc, || doc.layout_scene(*id), *id, 0, &mut budget);
                match tex {
                    Some(t) => {
                        ui.painter().image(t.id(), img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        ui.painter().rect_filled(img, 2.0, crate::theme::gray(80));
                        ctx.request_repaint();
                    }
                }
                let stroke = if resp.hovered() { egui::Stroke::new(2.0, theme::ACCENT()) } else { egui::Stroke::new(1.0, crate::theme::gray(90)) };
                ui.painter().rect_stroke(img, 2.0, stroke, egui::StrokeKind::Outside);
                crate::theme::centered(ui.painter(), img.center_bottom() + egui::vec2(0.0, 10.0), name, egui::FontId::proportional(11.0), theme::TEXT_DIM());
                if resp.clicked() {
                    pick = Some(*id);
                }
                if i % 4 == 3 {
                    ui.end_row();
                }
            }
        });
        if let Some(layout) = pick {
            ui.close();
            if self.blocked_by_slide_groups() {
                return;
            }
            let at = (self.cur + 1).min(self.slides.len());
            let Some(doc) = self.doc.as_mut() else { return };
            doc.begin(&mut self.hist, "Add Slide");
            let r = doc.new_slide_from_layout(&mut self.hist, layout, at);
            doc.end(&mut self.hist);
            match r {
                Ok(i) => {
                    self.bump_all();
                    self.go_to(i);
                }
                Err(e) => self.notify(format!("Couldn't add the slide: {e}")),
            }
        }
    }

    fn notes_panel(&mut self, ui: &mut egui::Ui) {
        let Some(slide) = self.current_slide().map(|s| s.slide) else { return };
        if self.notes_buf.as_ref().map(|(s, _)| *s) != Some(slide) {
            self.commit_notes();
            // edit the raw notes text so the diff-based commit lines up with the storage
            let text = self.doc.as_ref().and_then(|d| d.notes_storage(slide).map(|s| d.storage_text(s))).unwrap_or_default();
            self.notes_buf = Some((slide, text));
        }
        let (_, buf) = self.notes_buf.as_mut().unwrap();
        let resp = egui::ScrollArea::vertical()
            .show(ui, |ui| {
                ui.add_sized(
                    ui.available_size(),
                    egui::TextEdit::multiline(buf).hint_text("Click to add presenter notes").frame(egui::Frame::NONE).font(egui::FontId::proportional(15.0)),
                )
            })
            .inner;
        if resp.lost_focus() {
            self.commit_notes();
            self.bump(slide);
        }
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        crate::welcome::show(self, ui);
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if self.canvas.cell.is_some() {
            return;
        }
        if self.canvas.is_editing_text() {
            // typing owns the keyboard; undo/redo end the typing session first
            use egui::{Key, KeyboardShortcut as KS, Modifiers as M};
            let (undo, redo) = ctx.input_mut(|i| (i.consume_shortcut(&KS::new(M::COMMAND, Key::Z)), i.consume_shortcut(&KS::new(M::COMMAND | M::SHIFT, Key::Z))));
            if undo || redo {
                crate::textedit::commit(self);
                if redo {
                    self.redo()
                } else {
                    self.undo()
                }
            }
            return;
        }
        use egui::{Key, KeyboardShortcut as KS, Modifiers as M};
        // Ctrl+Tab / Ctrl+Shift+Tab: next / previous presentation tab
        if self.tabs.len() > 1 {
            let (next, prev) = ctx.input_mut(|i| (i.consume_shortcut(&KS::new(M::CTRL, Key::Tab)), i.consume_shortcut(&KS::new(M::CTRL | M::SHIFT, Key::Tab))));
            if next || prev {
                self.command(ctx, if next { "Show Next Tab" } else { "Show Previous Tab" });
                return;
            }
        }
        let cmd = M::COMMAND;
        let shift = M::COMMAND | M::SHIFT;
        let alt = M::COMMAND | M::ALT;
        let shortcuts: &[(KS, &str)] = &[
            (KS::new(cmd, Key::O), "Open…"),
            (KS::new(cmd, Key::N), "New…"),
            (KS::new(cmd, Key::F), "Find…"),
            (KS::new(cmd, Key::K), "Add Link"),
            (KS::new(shift, Key::G), "Find Previous"),
            (KS::new(cmd, Key::G), "Find Next"),
            (KS::new(M::COMMAND | M::CTRL, Key::G), "Go To > Slide…"),
            (KS::new(cmd, Key::S), "Save…"),
            (KS::new(shift, Key::S), "Save As…"),
            (KS::new(shift, Key::Z), "Redo"),
            (KS::new(cmd, Key::Z), "Undo"),
            (KS::new(cmd, Key::D), "Duplicate Selection"),
            (KS::new(cmd, Key::A), "Select All"),
            (KS::new(shift, Key::A), "Deselect All"),
            (KS::new(shift, Key::N), "New Slide"),
            (KS::new(shift, Key::H), "Skip Slide"),
            (KS::new(alt, Key::P), "Play Slideshow"),
            (KS::new(cmd, Key::L), "Lock"),
            (KS::new(alt, Key::L), "Unlock"),
            (KS::new(shift, Key::F), "Bring to Front"),
            (KS::new(shift, Key::B), "Send to Back"),
            (KS::new(shift, Key::P), "Presenter Notes"),
            (KS::new(shift, Key::E), "Edit Slide Layouts"),
            (KS::new(shift, Key::D), "Show Chart Data Editor"),
            (KS::new(M::COMMAND | M::ALT | M::SHIFT, Key::G), "Ungroup"),
            (KS::new(alt, Key::G), "Group"),
            (KS::new(cmd, Key::R), "Rulers"),
            (KS::new(alt, Key::C), "Copy Style"),
            (KS::new(alt, Key::V), "Paste Style"),
            (KS::new(cmd, Key::B), "Bold"),
            (KS::new(cmd, Key::I), "Italic"),
            (KS::new(cmd, Key::U), "Underline"),
            (KS::new(cmd, Key::Plus), "Bigger"),
            (KS::new(cmd, Key::Equals), "Bigger"),
            (KS::new(cmd, Key::Minus), "Smaller"),
            (KS::new(M::NONE, Key::Delete), "Delete"),
            (KS::new(M::NONE, Key::Backspace), "Delete"),
        ];
        let mut fired = Vec::new();
        // egui turns Ctrl+C/X/V into clipboard events rather than key presses. Ctrl+V only comes
        // through when the system clipboard holds something, so copying objects also puts a
        // marker there; pasted text from elsewhere becomes a text box, an image a picture.
        let mut copied = false;
        let mut paste_text: Option<String> = None;
        let mut paste_image: Option<std::sync::Arc<egui::ColorImage>> = None;
        ctx.input(|i| {
            for e in &i.events {
                match e {
                    egui::Event::Copy => {
                        fired.push("Copy");
                        copied = true;
                    }
                    egui::Event::Cut => {
                        fired.push("Cut");
                        copied = true;
                    }
                    egui::Event::Paste(t) if t == crate::editing::CLIP_MARKER || (self.clip.is_some() && t.trim().is_empty()) => fired.push("Paste"),
                    egui::Event::Paste(t) => paste_text = Some(t.clone()),
                    egui::Event::PasteImage(img) => paste_image = Some(img.clone()),
                    _ => {}
                }
            }
        });
        if let Some(t) = paste_text {
            crate::editing::paste_text_as_box(self, &t);
        }
        if let Some(img) = paste_image {
            crate::editing::paste_image(self, &img);
        }
        ctx.input_mut(|i| {
            for (s, name) in shortcuts {
                if i.consume_shortcut(s) {
                    fired.push(*name);
                }
            }
        });
        for name in fired {
            self.command(ctx, name);
        }
        if copied && self.clip.is_some() {
            ctx.copy_text(crate::editing::CLIP_MARKER.to_string());
        }
        // slide navigation with arrow keys when nothing is selected on the canvas
        if self.sel.is_empty() {
            let (up, down) = ctx.input(|i| (i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::PageUp), i.key_pressed(Key::ArrowDown) || i.key_pressed(Key::PageDown)));
            if up && self.cur > 0 {
                self.go_to(self.cur - 1);
            }
            if down && self.cur + 1 < self.slides.len() {
                self.go_to(self.cur + 1);
            }
        }
    }

    /// Run a menu/toolbar command by its (Keynote) menu title.
    pub fn command(&mut self, ctx: &egui::Context, name: &str) {
        crate::menus::run(self, ctx, name);
    }
}

impl App {
    /// KN_RECORD=out.mp4|out.gif (automated checks): once the document is open, export the show
    /// as a movie with builds and transitions, then quit.
    fn record_hook(&mut self) {
        static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let Some(out) = std::env::var_os("KN_RECORD").map(std::path::PathBuf::from) else { return };
        if self.doc.is_none() || self.slides.is_empty() || STARTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let gif = out.extension().is_some_and(|e| e == "gif");
        self.export.format = if gif { crate::export::Format::Gif } else { crate::export::Format::Movie };
        self.export.seconds = 1.0;
        crate::export::record_to(self, out);
        // KN_SLIDE=n: start there; KN_RECORD_UNTIL=n: stop after slide n
        if let (Some(p), Ok(n)) = (self.play.as_mut(), std::env::var("KN_SLIDE").unwrap_or_default().parse::<usize>()) {
            p.index = n.saturating_sub(1);
        }
    }

    fn snapshot(&mut self, ctx: &egui::Context) {
        let loading = self.loading() || (std::env::args().nth(1).is_some() && self.doc.is_none());
        let Some((path, frames)) = self.snap.as_mut() else { return };
        // (the app sleeps when nothing changes: keep frames coming until the shot is taken)
        ctx.request_repaint_after(Duration::from_millis(16));
        if loading {
            // KN_SNAP_LOADING: shoot the Opening window
            static LOAD_T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
            static LOAD_SHOT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            let t0 = *LOAD_T0.get_or_init(Instant::now);
            if std::env::var_os("KN_SNAP_LOADING").is_some() && t0.elapsed() > Duration::from_millis(1500) && !LOAD_SHOT.swap(true, std::sync::atomic::Ordering::Relaxed) {
                let p = std::path::PathBuf::from(std::env::var("KN_SNAP_LOADING").unwrap_or_default());
                let cb = egui::ScreenshotCallback::new(move |img: std::sync::Arc<egui::ColorImage>| {
                    let [w, h] = img.size;
                    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                    if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                        let _ = buf.save(&p);
                        eprintln!("loading snapshot saved");
                    }
                });
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
            }
            ctx.request_repaint();
            return;
        }
        *frames += 1;
        let frames = *frames;
        let path = path.clone();
        let ready = frames > 90;
        if ready && frames == 91 {
            if let Ok(i) = std::env::var("KN_SLIDE").unwrap_or_default().parse::<usize>() {
                self.go_to(i.saturating_sub(1));
            }
            if let Ok(id) = std::env::var("KN_SELECT").unwrap_or_default().parse::<u64>() {
                self.sel = vec![id];
            }
            // KN_ZOOM=percent: the canvas zoom
            if let Some(z) = std::env::var("KN_ZOOM").ok().and_then(|z| z.parse::<f32>().ok()) {
                self.zoom = Zoom::Percent(z);
            }
            // KN_CELL="row,col[,row2,col2]": the table cursor (and range) in the selected table
            if let (Ok(c), Some(id)) = (std::env::var("KN_CELL"), self.sel.first().copied()) {
                let n: Vec<u32> = c.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if n.len() >= 2 {
                    self.canvas.table_cursor = Some((id, n[0], n[1]));
                }
                if n.len() >= 4 {
                    self.canvas.table_range = Some((id, n[0], n[1], n[2], n[3]));
                }
            }
            if let (Ok(t), Some(id)) = (std::env::var("KN_TYPE"), self.sel.first().copied()) {
                if let Some(sc) = self.scene(self.cur) {
                    if crate::textedit::begin(self, &sc, id, None) {
                        let storage = self.canvas.editing.as_ref().unwrap().storage;
                        let doc = self.doc.as_mut().unwrap();
                        let end = doc.storage_text(storage).len();
                        doc.replace_text(&mut self.hist, storage, end, end, &t.replace("\\n", "\n"));
                        let n = doc.storage_text(storage).len();
                        let ed = self.canvas.editing.as_mut().unwrap();
                        ed.cursor = n;
                        ed.anchor = n.saturating_sub(4);
                        ed.version += 1;
                        ed.scene = None;
                    }
                }
            }
            if std::env::var_os("KN_LAYOUTS").is_some() {
                self.toggle_layout_editing();
            }
            // KN_EDIT=1: start editing the selected text (KN_CMD then runs while editing)
            if std::env::var_os("KN_EDIT").is_some() {
                if let (Some(sc), Some(id)) = (self.scene(self.cur), self.sel.first().copied()) {
                    crate::textedit::begin(self, &sc, id, None);
                }
            }
            if std::env::var_os("KN_AUDIT").is_some() {
                crate::menus::audit(self, ctx);
                std::process::exit(0);
            }
            // KN_CMD="Shape > Star;Bold": run menu commands (automated editing checks)
            if let Ok(cmds) = std::env::var("KN_CMD") {
                for c in cmds.split(';').filter(|c| !c.is_empty()) {
                    self.command(ctx, c);
                }
            }
            if let Ok(t) = std::env::var("KN_TAB").unwrap_or_default().parse::<usize>() {
                self.insp.format_tab = t;
            }
            match std::env::var("KN_INSPECTOR").as_deref() {
                Ok("animate") => self.inspector = InspectorKind::Animate,
                Ok("document") => self.inspector = InspectorKind::Document,
                _ => {}
            }
            if std::env::var_os("KN_PLAY").is_some() {
                self.start_slideshow(self.cur);
            }
        }
        // KN_SNAP_AFTER=<seconds>: shoot that long after the slideshow/slide was opened instead
        static OPENED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        if frames >= 91 {
            OPENED.get_or_init(Instant::now);
        }
        let after = std::env::var("KN_SNAP_AFTER").ok().and_then(|s| s.parse::<f32>().ok());
        let shoot = match after {
            Some(s) => ready && OPENED.get().is_some_and(|t| t.elapsed().as_secs_f32() >= s) && !SHOT.swap(true, std::sync::atomic::Ordering::Relaxed),
            None => ready && frames == 150,
        };
        static SHOT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if shoot {
            let ctx2 = ctx.clone();
            let cb = egui::ScreenshotCallback::new(move |img: std::sync::Arc<egui::ColorImage>| {
                let [w, h] = img.size;
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                    let _ = buf.save(&path);
                    eprintln!("snapshot saved to {}", path.display());
                }
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
                ctx2.request_repaint();
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
        }
        if frames > 600 && after.is_none() || OPENED.get().is_some_and(|t| t.elapsed().as_secs() > 100) {
            eprintln!("snapshot timed out");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

/// KN_PERF: report a part of the frame that took long.
fn perf_note(what: &str, t0: Instant) {
    if t0.elapsed().as_millis() >= 4 && std::env::var_os("KN_PERF").is_some() {
        eprintln!("perf: {what} took {:?}", t0.elapsed());
    }
}

fn recent_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("keydeck").join("recent.txt"))
}

fn dirs_cache_duplicates() -> PathBuf {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/keydeck/duplicates")).unwrap_or_default()
}

/// Marks an open error that is a request for the password (followed by the hint).
pub const PASSWORD_MARK: &str = "\u{1}password:";

fn title_for(p: &std::path::Path) -> String {
    format!("{} — Keydeck", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
}

impl App {
    /// GNOME 46 has no cursor-shape protocol, so the toolkit loads the cursor image itself — at
    /// scale 1 the first time the pointer enters (before it knows the monitor's scale), which
    /// looks blurry on HiDPI until the cursor shape next changes. Changing it for one frame just
    /// after the pointer enters makes it reload at the right scale (the system cursor, sharp).
    fn cursor_refresh(&mut self, ctx: &egui::Context) {
        let inside = ctx.input(|i| i.pointer.has_pointer());
        if inside && !self.pointer_was_inside {
            self.cursor_nudge = 4;
        }
        self.pointer_was_inside = inside;
        if self.cursor_nudge > 0 {
            self.cursor_nudge -= 1;
            if self.cursor_nudge == 1 {
                ctx.set_cursor_icon(egui::CursorIcon::Alias);
            }
            ctx.request_repaint();
        }
    }
}

impl App {
    /// Menus in the title bar (Settings > Window): the menu bar row doubles as the window's
    /// title bar — drag the empty part to move, double-click to maximise, GNOME-style window
    /// buttons on the right.
    fn title_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let row = ui.available_rect_before_wrap();
        let row = egui::Rect::from_min_size(row.min, egui::vec2(row.width(), 30.0));
        // window buttons where the desktop puts them (GNOME's button-layout setting)
        let draw_button = |ui: &mut egui::Ui, what: &str, r: egui::Rect| {
            let resp = ui.interact(r, ui.scope_id().with(("winbtn", what.to_string())), egui::Sense::click());
            let c = r.center();
            let bg = if resp.hovered() { crate::theme::gray(0x55) } else { crate::theme::gray(0x3c) };
            ui.painter().circle_filled(c, 11.0, bg);
            let st = egui::Stroke::new(1.3, theme::TEXT());
            match what {
                "minimize" => {
                    ui.painter().line_segment([c + egui::vec2(-4.0, 0.0), c + egui::vec2(4.0, 0.0)], st);
                }
                "maximize" => {
                    ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(8.0, 8.0)), 1.0, st, egui::StrokeKind::Middle);
                }
                _ => {
                    ui.painter().line_segment([c + egui::vec2(-3.5, -3.5), c + egui::vec2(3.5, 3.5)], st);
                    ui.painter().line_segment([c + egui::vec2(-3.5, 3.5), c + egui::vec2(3.5, -3.5)], st);
                }
            }
            if resp.clicked() {
                match what {
                    "minimize" => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                    "maximize" => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized)),
                    _ => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                }
            }
        };
        let layout = self.buttons.clone();
        let mut x = row.max.x - 4.0;
        for what in layout.right.iter().rev() {
            draw_button(ui, what, egui::Rect::from_min_max(egui::pos2(x - 30.0, row.min.y), egui::pos2(x, row.max.y)));
            x -= 32.0;
        }
        let buttons_left = x + 2.0;
        let mut lx = row.min.x + 2.0;
        for what in &layout.left {
            draw_button(ui, what, egui::Rect::from_min_max(egui::pos2(lx, row.min.y), egui::pos2(lx + 30.0, row.max.y)));
            lx += 32.0;
        }
        let row = egui::Rect::from_min_max(egui::pos2(if layout.left.is_empty() { row.min.x } else { lx + 6.0 }, row.min.y), row.max);
        // menus, left (in a child ui so the menu bar can't take the whole row)
        // the menu bar gets only the width its menus took last frame, so it can't cover (and
        // swallow presses on) the drag area to its right; the bar's own rect may fill its space,
        // so measure the menus themselves
        let menus_w = self.menu_bar_width.unwrap_or(620.0).min(buttons_left - row.min.x);
        let menus_rect = egui::Rect::from_min_max(row.min, egui::pos2(row.min.x + menus_w, row.max.y));
        let used = ui
            .scope_builder(egui::UiBuilder::new().max_rect(menus_rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| crate::menus::menu_bar(self, ui))
            .inner;
        self.menu_bar_width = Some(used - row.min.x + 4.0);
        // the empty middle: drag to move, double-click to maximise; document name centred
        let drag_rect = egui::Rect::from_min_max(egui::pos2(used.min(buttons_left) + 8.0, row.min.y), egui::pos2(buttons_left - 8.0, row.max.y));
        if std::env::var_os("KN_DEBUG").is_some() && ui.ctx().cumulative_frame_nr() % 60 == 5 {
            eprintln!("title bar: menus end {used:.0}, drag area {:.0}..{:.0}", drag_rect.min.x, drag_rect.max.x);
        }
        if drag_rect.width() > 10.0 {
            // (the document's name is in the toolbar, as in Keynote: not repeated here)
            let _drag = ui.interact(drag_rect, ui.scope_id().with("titlebar-drag"), egui::Sense::click());
            // the move starts on the press itself (as GNOME's own window frames do): waiting for
            // the pointer to move first lost the drag when the press also activated the window;
            // a second press soon after is a double-click and maximises instead
            let press = ui.input(|i| i.pointer.primary_pressed() && i.pointer.press_origin().is_some_and(|p| drag_rect.contains(p)));
            if press {
                let now = ui.input(|i| i.time);
                if now - self.title_press_at < 0.4 {
                    self.title_press_at = -1.0;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                } else {
                    self.title_press_at = now;
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            }
        }
        ui.expand_to_include_rect(row);
    }

    /// Without system decorations the window edges are ours: show resize cursors near them and
    /// hand the press to the compositor to resize.
    fn window_edges(&mut self, ctx: &egui::Context, r: egui::Rect) {
        use egui::viewport::ResizeDirection as D;
        if self.play.is_some() || ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)) {
            return;
        }
        let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return };
        // a band along the window's edge, reaching into its shadow (as GTK windows have)
        let m = 6.0;
        if !r.expand(12.0).contains(p) {
            return;
        }
        let (l, rt, t, b) = (p.x - r.min.x < m, r.max.x - p.x < m, p.y - r.min.y < m, r.max.y - p.y < m);
        let dir = match (l, rt, t, b) {
            (true, _, true, _) => Some(D::NorthWest),
            (_, true, true, _) => Some(D::NorthEast),
            (true, _, _, true) => Some(D::SouthWest),
            (_, true, _, true) => Some(D::SouthEast),
            (true, ..) => Some(D::West),
            (_, true, ..) => Some(D::East),
            (_, _, true, _) => Some(D::North),
            (_, _, _, true) => Some(D::South),
            _ => None,
        };
        let Some(dir) = dir else { return };
        ctx.set_cursor_icon(match dir {
            D::North | D::South => egui::CursorIcon::ResizeVertical,
            D::East | D::West => egui::CursorIcon::ResizeHorizontal,
            D::NorthWest | D::SouthEast => egui::CursorIcon::ResizeNwSe,
            _ => egui::CursorIcon::ResizeNeSw,
        });
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
        }
    }
}

impl eframe::App for App {
    /// Touchpad scrolling keeps going after the fingers lift, slowing down, as in GNOME / macOS
    /// apps (Linux reports the lift but no momentum of its own).
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // fingers: one scrolls the canvas, a hold is a right click (the iPad's way)
        kn_ui::touch::raw_input(ctx, raw);
        let now = raw.time.unwrap_or(0.0);
        let dt = self.kinetic.2.map(|t| (now - t).clamp(0.001, 0.1) as f32).unwrap_or(1.0 / 60.0);
        self.kinetic.2 = Some(now);
        let (mut vel, mut coasting) = (self.kinetic.0, self.kinetic.1);
        let mut wheel = false;
        for e in &raw.events {
            match e {
                egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, phase, modifiers, .. } if modifiers.is_none() => {
                    wheel = true;
                    match phase {
                        egui::TouchPhase::Start => {
                            vel = egui::Vec2::ZERO;
                            coasting = false;
                        }
                        egui::TouchPhase::Move => {
                            coasting = false;
                            vel = vel * 0.6 + (*delta / dt) * 0.4;
                        }
                        egui::TouchPhase::End | egui::TouchPhase::Cancel => coasting = vel.length() > 120.0,
                    }
                }
                egui::Event::MouseWheel { .. } | egui::Event::PointerButton { pressed: true, .. } | egui::Event::Key { .. } => {
                    coasting = false;
                    vel = egui::Vec2::ZERO;
                }
                _ => {}
            }
        }
        if coasting && !wheel {
            vel *= (-dt * 3.2).exp();
            if vel.length() < 15.0 {
                coasting = false;
                vel = egui::Vec2::ZERO;
            } else {
                raw.events.push(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vel * dt, phase: egui::TouchPhase::Move, modifiers: egui::Modifiers::NONE, source: egui::MouseWheelSource::Momentum });
                ctx.request_repaint();
            }
        }
        self.kinetic = (vel, coasting, Some(now));
        // egui eases touchpad steps of 8 points or more over a tenth of a second (meant for
        // mouse wheels), which makes fast swipes trail behind the fingers: hand them over in
        // pieces below that so they apply at once, as on the Mac
        let mut out = Vec::with_capacity(raw.events.len());
        for e in raw.events.drain(..) {
            match e {
                egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, phase: egui::TouchPhase::Move, modifiers, source } if delta.length() >= 7.5 => {
                    let n = (delta.length() / 7.5).ceil();
                    for _ in 0..n as usize {
                        out.push(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: delta / n, phase: egui::TouchPhase::Move, modifiers, source });
                    }
                }
                e => out.push(e),
            }
        }
        raw.events = out;
        // KN_SCROLL=points per frame (automated checks): keep scrolling the navigator
        if let Some(v) = std::env::var("KN_SCROLL").ok().and_then(|v| v.parse::<f32>().ok()) {
            let n = ((now * 60.0) as i64 / 120) % 2;
            raw.events.push(egui::Event::PointerMoved(egui::pos2(100.0, 400.0)));
            raw.events.push(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, if n == 0 { -v } else { v }), phase: egui::TouchPhase::Move, modifiers: egui::Modifiers::NONE, source: egui::MouseWheelSource::Trackpad });
            ctx.request_repaint();
        }
        // KN_DRAG=x0,y0,x1,y1 (automated checks): drag with the mouse once, after a moment
        if let Some(v) = std::env::var("KN_DRAG").ok().map(|v| v.split(',').filter_map(|n| n.trim().parse::<f32>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 4) {
            thread_local!(static STEP: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
            let step = STEP.with(|s| { s.set(s.get() + 1); s.get() });
            let (a, b) = (egui::pos2(v[0], v[1]), egui::pos2(v[2], v[3]));
            // KN_DRAG_AT: the frame the press comes at (default 180)
            let at: u32 = std::env::var("KN_DRAG_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(180);
            let step = (step + 180).saturating_sub(at);
            let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            match step {
                180 => raw.events.extend([egui::Event::PointerMoved(a), button(a, true)]),
                181..=210 => raw.events.push(egui::Event::PointerMoved(a + (b - a) * ((step - 180) as f32 / 30.0))),
                211 => raw.events.push(button(b, false)),
                _ => {}
            }
            if step <= 212 {
                ctx.request_repaint();
            }
        }
        // KN_KEYS="Down,Shift+Right,Text:5,Enter,Delete" (automated checks): one key per frame,
        // after a moment
        // KN_HOVER="x,y" (tests): the pointer rests there
        if let Some((x, y)) = std::env::var("KN_HOVER").ok().and_then(|v| v.split_once(',').and_then(|(x, y)| Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?)))) {
            raw.events.push(egui::Event::PointerMoved(egui::pos2(x, y)));
        }
        if let Ok(spec) = std::env::var("KN_KEYS") {
            thread_local!(static AT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
            let f = AT.with(|a| { a.set(a.get() + 1); a.get() });
            thread_local!(static KEYS_RELEASE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
            if (f - 200) % 4 == 2 && KEYS_RELEASE.with(|r| r.replace(false)) {
                raw.events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
            }
            let keys: Vec<&str> = spec.split(',').collect();
            if f >= 200 && (f - 200) % 4 == 0 {
                if let Some(k) = keys.get(((f - 200) / 4) as usize) {
                    if let Some(t) = k.strip_prefix("Text:") {
                        raw.events.push(egui::Event::Text(t.to_string()));
                    } else {
                        let (mods, name) = match k.split_once('+') {
                            Some(("Shift", n)) => (egui::Modifiers::SHIFT, n),
                            Some(("Ctrl", n)) => (egui::Modifiers::COMMAND, n),
                            _ => (egui::Modifiers::NONE, *k),
                        };
                        let key = match name {
                            "Up" => Some(egui::Key::ArrowUp),
                            "Down" => Some(egui::Key::ArrowDown),
                            "Left" => Some(egui::Key::ArrowLeft),
                            "Right" => Some(egui::Key::ArrowRight),
                            "Enter" => Some(egui::Key::Enter),
                            "Tab" => Some(egui::Key::Tab),
                            "Escape" => Some(egui::Key::Escape),
                            "Delete" => Some(egui::Key::Delete),
                            "Backspace" => Some(egui::Key::Backspace),
                            n => egui::Key::from_name(n),
                        };
                        if let Some(key) = key {
                            raw.events.push(egui::Event::ModifiersChanged(mods));
                            for pressed in [true, false] {
                                raw.events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: mods });
                            }
                            // (modifiers released again next frame)
                            if mods != egui::Modifiers::NONE {
                                KEYS_RELEASE.with(|r| r.set(true));
                            }
                        }
                    }
                }
            }
            if (f as usize) < 200 + keys.len() * 4 + 4 {
                ctx.request_repaint();
            }
        }
        // KN_PERF (automated checks): frame times, once a second
        if std::env::var_os("KN_PERF").is_some() {
            thread_local!(static PERF: std::cell::RefCell<(f64, Vec<f64>)> = const { std::cell::RefCell::new((0.0, Vec::new())) });
            PERF.with(|p| {
                let mut p = p.borrow_mut();
                if p.0 > 0.0 {
                    let d = now - p.0;
                    p.1.push(d);
                }
                p.0 = now;
                if p.1.iter().sum::<f64>() > 1.0 {
                    let v = std::mem::take(&mut p.1);
                    let max = v.iter().cloned().fold(0.0, f64::max);
                    eprintln!("perf: {} frames, avg {:.1} ms, max {:.1} ms", v.len(), v.iter().sum::<f64>() / v.len() as f64 * 1000.0, max * 1000.0);
                }
            });
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // transparent around the window: its shadow is drawn there
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        crate::glass::native_blur(_frame, crate::glass::translucent_panes());
        let ctx = ui.ctx().clone();
        kn_ui::begin_frame(&ctx);
        kn_ui::scale::apply(&ctx, self.settings.ui_scale);
        kn_ui::form::set_pref(self.settings.ui_layout);
        if self.canvas.editing.is_some() {
            kn_ui::typing(&ctx);
        }
        fit_window_to_screen(&ctx);
        // macOS: documents opened from Finder / Open With / the Dock arrive as an Apple event
        // (patched winit queues them), not on the command line
        #[cfg(target_os = "macos")]
        {
            for path in winit::platform::macos::take_opened_files() {
                self.open_path(path);
            }
            // (the event may come while the app is idle: look again shortly)
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        // our own frame (menus in the title bar on a desktop that leaves frames to apps)
        let m = if crate::winframe::system_title_bar() { 0.0 } else { crate::winframe::margin() };
        let inner = ui.max_rect().shrink(m);
        self.corner = if m > 0.0 { crate::winframe::RADIUS } else { 0.0 };
        let dark = !theme::is_light();
        if m > 0.0 {
            let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
            crate::winframe::paint_shadow(&ctx, inner, self.corner, focused, dark);
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| self.main_ui(ui, inner));
        if self.play.is_none() && self.show_toolbar {
            crate::toolbar::sheet(self, &ctx);
            crate::toolbar::drag_overlay(self, &ctx);
        }
        if m > 0.0 {
            crate::winframe::paint_outline(&ctx, inner, self.corner, dark);
        }
    }
}

impl App {
    /// Rounded window corners for a panel touching them (`[nw, ne, sw, se]`).
    fn corners(&self, c: [bool; 4]) -> egui::CornerRadius {
        let r = self.corner.round() as u8;
        let k = |b: bool| if b { r } else { 0 };
        egui::CornerRadius { nw: k(c[0]), ne: k(c[1]), sw: k(c[2]), se: k(c[3]) }
    }

    fn main_ui(&mut self, ui: &mut egui::Ui, window: egui::Rect) {
        let ctx = ui.ctx().clone();
        self.cursor_refresh(&ctx);
        self.poll_jobs(&ctx);
        // the slideshow renders with every image decoded immediately; editing decodes in the background
        self.renderer.set_async_images(self.play.is_none());
        if self.renderer.images_arrived() {
            self.tex.invalidate();
            ctx.request_repaint();
        }
        self.snapshot(&ctx);
        self.record_hook();
        self.closing_ui(&ctx);
        self.all_tabs_ui(&ctx);

        // dropped files (on Android, also files picked in the system picker): presentations
        // open, pictures, movies and sounds go on the slide
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        for p in dropped {
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext == "key" || ext == "pptx" {
                self.open_path(p);
            } else if self.doc.is_some() && crate::media::IMAGE_EXT.iter().chain(crate::media::MOVIE_EXT).chain(crate::media::AUDIO_EXT).any(|e| *e == ext) {
                self.insert_media_path(p);
            }
        }

        if self.play.is_some() {
            crate::play::ui(self, ui);
            let ctx = ui.ctx().clone();
            self.notices(&ctx);
            return;
        }
        self.handle_shortcuts(&ctx);

        // the window's visible area (menus keep inside it)
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("kd-window-rect"), window));
        let integrated = !crate::winframe::system_title_bar();
        if integrated {
            self.window_edges(&ctx, window);
        }
        let top = self.corners([true, true, false, false]);
        let mut top_y = egui::Panel::top("menubar")
            .frame(egui::Frame::new().fill(theme::TOOLBAR_BG()).corner_radius(top).inner_margin(egui::Margin::symmetric(6, 2)))
            .show(ui, |ui| {
                if integrated {
                    self.title_bar(ui);
                } else {
                    crate::menus::menu_bar(self, ui);
                }
            })
            .response
            .rect
            .max
            .y;
        // macOS 26 layout: no toolbar strip; the sidebars and the inspector reach up to the
        // menu bar, the toolbar's buttons float over them and over the canvas; window tabs are a
        // second row under the buttons, right of the sidebar (R35)
        let mut tabs_h = if self.tabs.len() > 1 && self.show_tab_bar { 28.0 } else { 0.0 };
        let mut tb_h = if self.show_toolbar { crate::toolbar::height(self) } else { 0.0 } + tabs_h;
        // Settings > Classic look: the toolbar as a strip of its own under the menus (sidebar
        // button at its left end, plain capsules), square tabs under it, the panels below
        if crate::glass::classic() {
            if self.show_toolbar {
                self.tb.left_w = 0.0;
                top_y = egui::Panel::top("toolbar")
                    .exact_size(crate::toolbar::height(self))
                    .frame(egui::Frame::new().fill(theme::TOOLBAR_BG()).inner_margin(egui::Margin::symmetric(10, 4)))
                    .show(ui, |ui| self.toolbar(ui))
                    .response
                    .rect
                    .max
                    .y;
            }
            if tabs_h > 0.0 {
                top_y = egui::Panel::top("tabbar").exact_size(28.0).frame(egui::Frame::new().fill(theme::gray(0x1c)).inner_margin(egui::Margin::ZERO)).show(ui, |ui| self.tab_bar_classic(ui)).response.rect.max.y;
            }
            tabs_h = 0.0;
            tb_h = 0.0;
        }
        let tb_m = tb_h as i8;

        self.notices(&ctx);
        if self.show_about {
            egui::Window::new("About Keydeck").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut self.show_about).show(&ctx, |ui| {
                ui.label("Keydeck");
                ui.label("Reads and writes Keynote (.key) presentations.");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            });
        }

        crate::export::ui(self, &ctx);
        crate::find::ui(self, &ctx);
        crate::filemenu::ui(self, &ctx);
        crate::replacefonts::ui(self, &ctx);
        crate::inspector::build_order_window(self, &ctx);
        crate::menus::debug_menu(self, &ctx);
        self.goto_window(&ctx);
        crate::links::dialog(self, &ctx);
        crate::themes::chooser(self, &ctx);
        crate::chartedit::ui(self, &ctx);
        crate::settings::settings_window(self, &ctx);
        crate::spellwin::window(self, &ctx);
        crate::webmedia::window(self, &ctx);
        if self.hub.needs_shapes() {
            (self.hub.basic, self.hub.mine) = crate::editing::hub_shapes();
        }
        if let Some(p) = self.hub.show(&ctx) {
            crate::editing::insert_from_hub(self, p);
        }
        crate::condwin::window(self, &ctx);
        self.thumb_worker_tick(&ctx);
        crate::play::clear_recording_prompt(self, &ctx);
        crate::warnings::window(self, &ctx);
        crate::imageadj::window(self, &ctx);
        crate::toolwins::ui(self, &ctx);
        crate::mediabrowser::ui(self, &ctx);
        crate::versions::ui(self, &ctx);
        crate::equation::ui(self, &ctx);
        crate::settings::help_window(self, &ctx);
        if self.doc.is_none() {
            let cr = self.corners([false, false, true, true]);
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::CANVAS_BG()).corner_radius(cr).inner_margin(egui::Margin { top: tb_m, ..Default::default() })).show(ui, |ui| self.welcome(ui));
            self.floating_toolbar(ui, window, top_y, tb_h, tabs_h, 0.0, 0.0);
            return;
        }

        // panes slide in and out (Keynote: ~250 ms ease-in-out, the canvas re-fitting as they move)
        let editing_view = matches!(self.view, ViewMode::Navigator | ViewMode::SlideOnly);
        let mut nav_open = self.view == ViewMode::Navigator;
        // which panes reach the window's bottom corners
        let list_shown = self.show_object_list && editing_view;
        let insp_shown = self.show_inspector && editing_view;
        let left_shown = nav_open || list_shown;
        let bottom_bar = editing_view && (self.editing_layouts.is_some() || self.show_notes);
        let (nav_cr, list_cr, insp_cr) = (self.corners([false, false, true, false]), self.corners([false, false, !nav_open, false]), self.corners([false, false, false, true]));
        let bottom_cr = self.corners([false, false, !left_shown, !insp_shown]);
        let central_cr = if bottom_bar && editing_view { egui::CornerRadius::ZERO } else { bottom_cr };
        // the panes slide (show_collapsible times itself by animation_time); everything else,
        // popups and menus included, is instant as on the Mac (N21) - so inside the panes too
        ui.ctx().all_styles_mut(|s| s.animation_time = theme::PANE_SLIDE_S);
        let nav_rect = egui::Panel::left("navigator")
            .show_separator_line(false)
            .resizable(true)
            .default_size(144.0)
            .size_range(110.0..=242.0)
            .frame(egui::Frame::new().fill(if crate::glass::translucent_panes() { Color32::TRANSPARENT } else { theme::SIDEBAR_BG() }).corner_radius(nav_cr))
            .show_collapsible(ui, &mut nav_open, |ui| instant_inside(ui, |ui| {
                if crate::glass::translucent_panes() {
                    crate::glass::pane_background_rounded(ui, theme::SIDEBAR_BG(), nav_cr);
                }
                let t0 = Instant::now();
                crate::navigator::ui(self, ui);
                perf_note("navigator", t0)
            }))
            .map(|r| r.response.rect);
        // the panes' right edge: the toolbar's title starts after it
        let mut left_edge = nav_rect.map(|r| r.max.x).unwrap_or(window.min.x);
        if !nav_open && self.view == ViewMode::Navigator {
            // dragged closed, as in Keynote
            self.view = ViewMode::SlideOnly;
        }
        let mut list_open = self.show_object_list && editing_view;
        if let Some(r) = egui::Panel::left("objectlist")
            .show_separator_line(false)
            .resizable(true)
            .default_size(270.0)
            .size_range(180.0..=420.0)
            .frame(egui::Frame::new().fill(theme::SIDEBAR_BG()).corner_radius(list_cr))
            .show_collapsible(ui, &mut list_open, |ui| instant_inside(ui, |ui| crate::objectlist::panel(self, ui)))
        {
            left_edge = left_edge.max(r.response.rect.max.x);
        }
        // (the phone layout: the inspector is a sheet from the bottom; it and the navigator are
        // closed to begin with)
        let compact = kn_ui::form::compact(ui.ctx());
        if kn_ui::form::switched(ui.ctx()) == Some(true) {
            self.show_inspector = false;
            self.show_notes = false;
            if self.view == ViewMode::Navigator {
                self.view = ViewMode::SlideOnly;
            }
        }
        if compact && self.show_inspector && editing_view {
            kn_ui::form::sheet(ui, "inspector-sheet", theme::PANEL_BG(), |ui| crate::inspector::ui(self, ui));
        }
        let mut insp_open = self.show_inspector && editing_view && !compact;
        let right_edge = egui::Panel::right("inspector")
            .show_separator_line(false)
            .resizable(false)
            .exact_size(300.0)
            .frame(egui::Frame::new().fill(if crate::glass::translucent_panes() { Color32::TRANSPARENT } else { theme::PANEL_BG() }).corner_radius(insp_cr).inner_margin(egui::Margin { left: 12, right: 12, top: 12 + tb_m, bottom: 12 }))
            .show_collapsible(ui, &mut insp_open, |ui| instant_inside(ui, |ui| {
                if crate::glass::translucent_panes() {
                    crate::glass::pane_background_rounded(ui, theme::PANEL_BG(), insp_cr);
                }
                let t0 = Instant::now();
                crate::inspector::ui(self, ui);
                perf_note("inspector", t0)
            }))
            .map(|r| r.response.rect.min.x)
            .unwrap_or(window.max.x);
        let left_w = left_edge - window.min.x;
        let right_w = window.max.x - right_edge;
        // the rest of the window (canvas, toolbar popovers, menus): no fades
        ui.ctx().all_styles_mut(|s| s.animation_time = 0.0);
        match self.view {
            ViewMode::LightTable => {
                egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::CANVAS_BG()).corner_radius(central_cr).inner_margin(egui::Margin { top: tb_m, ..Default::default() })).show(ui, |ui| crate::navigator::light_table(self, ui));
            }
            ViewMode::Outline => {
                egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL_BG()).corner_radius(central_cr).inner_margin(egui::Margin { left: 16, right: 16, top: 16 + tb_m, bottom: 16 })).show(ui, |ui| crate::navigator::outline(self, ui));
            }
            _ => {
                if self.editing_layouts.is_some() {
                    egui::Panel::bottom("layouts-done").exact_size(44.0).frame(egui::Frame::new().fill(theme::TOOLBAR_BG()).corner_radius(bottom_cr).inner_margin(8)).show(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.label(RichText::new("Editing slide layouts — changes apply to every slide that uses the layout").color(theme::TEXT_DIM()));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.add(egui::Button::new(RichText::new("Done").strong()).fill(theme::ACCENT())).clicked() {
                                    self.toggle_layout_editing();
                                }
                            });
                        });
                    });
                } else {
                    let mut notes_open = self.show_notes;
                    ui.ctx().all_styles_mut(|s| s.animation_time = theme::PANE_SLIDE_S);
                    egui::Panel::bottom("notes")
                        .resizable(true)
                        .default_size(140.0)
                        .size_range(60.0..=400.0)
                        .frame(egui::Frame::new().fill(theme::CANVAS_BG()).corner_radius(bottom_cr).inner_margin(12))
                        .show_collapsible(ui, &mut notes_open, |ui| instant_inside(ui, |ui| self.notes_panel(ui)));
                    ui.ctx().all_styles_mut(|s| s.animation_time = 0.0);
                    if !notes_open {
                        self.show_notes = false;
                    }
                }
                self.tb.inset = tb_h;
                egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::CANVAS_BG()).corner_radius(central_cr)).show(ui, |ui| { let t0 = Instant::now(); crate::canvas::ui(self, ui); perf_note("canvas", t0) });
            }
        }
        self.floating_toolbar(ui, window, top_y, tb_h, tabs_h, left_w, right_w);
    }

    /// The toolbar of macOS 26 Keynote: no bar of its own; its buttons float over the
    /// sidebar, the canvas and the inspector, and the canvas fades under them (a tint at the
    /// top keeps the title and buttons readable over the slide).
    fn floating_toolbar(&mut self, ui: &mut egui::Ui, window: egui::Rect, top_y: f32, tb_h: f32, tabs_h: f32, left_w: f32, right_w: f32) {
        // the panels' dividers, the whole height of the panels
        let from = top_y;
        for (x, show) in [(window.min.x + left_w, left_w > 0.0), (window.max.x - right_w, right_w > 0.0)] {
            if show {
                ui.painter().vline(x, from..=window.max.y, egui::Stroke::new(1.0, theme::SEPARATOR()));
            }
        }
        if tb_h <= 0.0 {
            return;
        }
        let buttons_h = tb_h - tabs_h;
        let strip = egui::Rect::from_min_max(egui::pos2(window.min.x, top_y), egui::pos2(window.max.x, top_y + buttons_h));
        // the canvas's top edge: the slide blurs under the toolbar (drawn by the canvas) and a
        // tint of the canvas colour fades out below the buttons, in both appearances
        let edge = egui::Rect::from_min_max(egui::pos2(window.min.x + left_w, top_y), egui::pos2(window.max.x - right_w, top_y + tb_h + crate::glass::FEATHER_REACH));
        // (Keydeck's feathered look only: the Pages and Solid looks have a flat band, hard-edged)
        if edge.width() > 0.0 && crate::glass::toolbar_background() == crate::glass::ToolbarBackground::Feathered {
            let [r, g, b, _] = theme::CANVAS_BG().to_array();
            let mut mesh = egui::Mesh::default();
            let steps = 10;
            for k in 0..=steps {
                let t = k as f32 / steps as f32;
                let y = edge.min.y + t * edge.height();
                let a = (1.0 - t * t * (3.0 - 2.0 * t)) * 0.35;
                let col = Color32::from_rgba_unmultiplied(r, g, b, (a * 255.0) as u8);
                mesh.colored_vertex(egui::pos2(edge.min.x, y), col);
                mesh.colored_vertex(egui::pos2(edge.max.x, y), col);
                if k > 0 {
                    let i = (2 * k) as u32;
                    mesh.add_triangle(i - 2, i - 1, i);
                    mesh.add_triangle(i - 1, i + 1, i);
                }
            }
            ui.painter().add(egui::Shape::mesh(mesh));
        }
        if tabs_h > 0.0 {
            // the same left and right edges as the buttons above it
            let row = egui::Rect::from_min_max(egui::pos2(window.min.x + 10.0 + left_w, top_y + buttons_h), egui::pos2(window.max.x - 10.0, top_y + tb_h));
            let mut tabs_ui = ui.new_child(egui::UiBuilder::new().max_rect(row).id_salt("tab-row"));
            self.tab_bar(&mut tabs_ui);
        }
        self.tb.left_w = left_w;
        let inner = strip.shrink2(egui::vec2(10.0, 4.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt("floating-toolbar"));
        self.toolbar(&mut child);
    }
}

/// The "Opening “name”…" window shown while a big presentation loads, with its progress and Cancel.
fn opening_window(ctx: &egui::Context, path: &std::path::Path, progress: &kn_format::progress::Progress) {
    use std::sync::atomic::Ordering;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let cancelling = progress.cancel.load(Ordering::Relaxed);
    if std::env::var_os("KN_DEBUG").is_some() {
        eprintln!("opening window {:.2}", progress.permille.load(Ordering::Relaxed) as f32 / 1000.0);
    }
    // the first slide's picture (the package's preview), read on another thread
    static PREVIEW: std::sync::Mutex<Option<(PathBuf, Option<egui::ColorImage>)>> = std::sync::Mutex::new(None);
    thread_local!(static TEX: std::cell::RefCell<Option<(PathBuf, egui::TextureHandle)>> = const { std::cell::RefCell::new(None) });
    {
        let mut pv = PREVIEW.lock().unwrap();
        if pv.as_ref().is_none_or(|(p, _)| p != path) {
            *pv = Some((path.to_path_buf(), None));
            let (p, ctx) = (path.to_path_buf(), ctx.clone());
            std::thread::spawn(move || {
                let img = (|| {
                    let mut z = zip::ZipArchive::new(std::fs::File::open(&p).ok()?).ok()?;
                    let mut bytes = Vec::new();
                    let entry = if z.index_for_name("preview-web.jpg").is_some() { "preview-web.jpg" } else { "preview.jpg" };
                    std::io::Read::read_to_end(&mut z.by_name(entry).ok()?, &mut bytes).ok()?;
                    let im = image::load_from_memory(&bytes).ok()?.to_rgba8();
                    Some(egui::ColorImage::from_rgba_unmultiplied([im.width() as usize, im.height() as usize], im.as_raw()))
                })();
                if let Some(pv) = PREVIEW.lock().unwrap().as_mut().filter(|(q, _)| *q == p) {
                    pv.1 = img;
                }
                ctx.request_repaint();
            });
        }
    }
    let thumb = TEX.with(|t| {
        let mut t = t.borrow_mut();
        if t.as_ref().is_none_or(|(p, _)| p != path) {
            if let Some(img) = PREVIEW.lock().unwrap().as_ref().and_then(|(_, i)| i.clone()) {
                *t = Some((path.to_path_buf(), ctx.load_texture("opening-preview", img, egui::TextureOptions::LINEAR)));
            }
        }
        t.as_ref().filter(|(p, _)| p == path).map(|(_, h)| h.clone())
    });
    // Keynote's panel: 349 × 112 pt, dark, an empty title strip, then the first slide, the
    // name, a bar with a blue segment sweeping across it, and a round × to cancel
    let dark = !crate::theme::is_light();
    let (bg, track, text) = if dark { (crate::theme::tint(Color32::from_gray(0x1e)), crate::theme::tint(Color32::from_gray(0x2c)), crate::theme::tint(Color32::from_gray(0xdc))) } else { (crate::theme::tint(Color32::from_gray(0xf2)), crate::theme::tint(Color32::from_gray(0xd8)), crate::theme::tint(Color32::from_gray(0x22))) };
    let size = egui::vec2(349.0, 112.0);
    let at = ctx.content_rect().center_top() + egui::vec2(-size.x / 2.0, ctx.content_rect().height() / 3.0 - size.y / 2.0);
    egui::Area::new(egui::Id::unique(("opening", path))).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::click());
        let p = ui.painter();
        p.rect(rect, 12.0, bg, egui::Stroke::new(1.0, crate::theme::tint(Color32::from_gray(if dark { 0x48 } else { 0xb8 }))), egui::StrokeKind::Inside);
        p.hline(rect.x_range(), rect.min.y + 28.0, egui::Stroke::new(1.0, crate::theme::tint(Color32::from_gray(if dark { 0x10 } else { 0xd0 }))));
        let row = egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.min.y + 28.0), rect.max);
        let cy = row.center().y;
        // first slide
        let th = egui::Rect::from_min_size(egui::pos2(rect.min.x + 16.0, cy - 17.0), egui::vec2(60.0, 34.0));
        match &thumb {
            Some(t) => {
                p.image(t.id(), th, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                p.rect_filled(th, 1.0, Color32::WHITE);
            }
        }
        let x0 = th.max.x + 18.0;
        // a long name is shortened in the middle to fit before the button
        let font = egui::FontId::new(14.0, egui::FontFamily::Name("bold".into()));
        let room = rect.max.x - 52.0 - x0;
        let fits = |s: &str| ui.fonts_mut(|f| f.layout_no_wrap(s.to_string(), font.clone(), text).size().x) <= room;
        let chars: Vec<char> = name.chars().collect();
        let mut label = format!("Opening \u{201c}{name}\u{201d}\u{2026}");
        let mut keep = chars.len();
        while !fits(&label) && keep > 4 {
            keep -= 1;
            let (h, t) = (keep / 2, keep - keep / 2);
            let short: String = chars[..h].iter().chain(['\u{2026}'].iter()).chain(chars[chars.len() - t..].iter()).collect();
            label = format!("Opening \u{201c}{short}\u{201d}\u{2026}");
        }
        p.text(egui::pos2(x0, cy - 9.0), egui::Align2::LEFT_BOTTOM, label, font, text);
        // bar: a blue segment sweeping left to right over and over
        let bar = egui::Rect::from_min_size(egui::pos2(x0, cy + 4.0), egui::vec2(rect.max.x - 52.0 - x0, 5.0));
        p.rect_filled(bar, 2.5, track);
        let t = (ui.input(|i| i.time) / 1.6).fract() as f32;
        let seg = bar.width() * 0.35;
        let a = (bar.min.x - seg + (bar.width() + seg) * t).max(bar.min.x);
        let b = (bar.min.x + (bar.width() + seg) * t).min(bar.max.x);
        if b > a {
            p.rect_filled(egui::Rect::from_min_max(egui::pos2(a, bar.min.y), egui::pos2(b, bar.max.y)), 2.5, Color32::from_rgb(0x0a, 0x84, 0xff));
        }
        // round cancel button
        let c = egui::pos2(rect.max.x - 26.0, cy);
        let resp = ui.interact(egui::Rect::from_center_size(c, egui::vec2(24.0, 24.0)), egui::Id::unique("opening-cancel"), egui::Sense::click());
        p.circle_stroke(c, 11.5, egui::Stroke::new(3.0, Color32::from_rgb(0x2f, 0x6f, 0xb0)));
        p.circle_filled(c, 9.0, if resp.hovered() { crate::theme::tint(Color32::from_gray(0xc8)) } else { crate::theme::tint(Color32::from_gray(0x9c)) });
        let k = 3.5;
        let col = if dark { crate::theme::tint(Color32::from_gray(0x2a)) } else { Color32::WHITE };
        p.line_segment([c + egui::vec2(-k, -k), c + egui::vec2(k, k)], egui::Stroke::new(2.0, col));
        p.line_segment([c + egui::vec2(-k, k), c + egui::vec2(k, -k)], egui::Stroke::new(2.0, col));
        if resp.clicked() && !cancelling {
            progress.cancel.store(true, Ordering::Relaxed);
        }
    });
    ctx.request_repaint();
}

/// Run a sliding pane's contents with animations off (popups opened there are instant, as on
/// the Mac), then put the pane slide's time back for the next pane.
fn instant_inside<R>(ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let ctx = ui.ctx().clone();
    ctx.all_styles_mut(|s| s.animation_time = 0.0);
    let r = f(ui);
    ctx.all_styles_mut(|s| s.animation_time = theme::PANE_SLIDE_S);
    r
}

/// Once, as soon as the screen's size is known: a first window bigger than the screen shrinks
/// to fit (1440×900 is larger than many laptop screens' usable area), and on macOS / Windows it
/// is centred, clear of the menu bar and Dock (Wayland doesn't let apps place windows).
fn fit_window_to_screen(ctx: &egui::Context) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.load(std::sync::atomic::Ordering::Relaxed) || std::env::var_os("KN_SIZE").is_some() {
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
        // centred, a little below the middle (the menu bar takes the top)
        let pos = egui::pos2(((m.x - size.x) / 2.0).max(0.0), ((m.y - size.y) / 2.0 + m.y * 0.02).max(25.0));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    }
}
