//! The window: title bar with menus, then Apple Notes' three columns — sidebar | note list |
//! editor — under one toolbar split at the columns (Notes 4.13, screenshots/ui-*.png). Gallery
//! view puts the note cards where the list and editor were.
//!
//! The editor (editor.rs, the notes-editor session's) draws in the editor column:
//! `crate::editor::show(app, ui)` with `ui.max_rect()` = the column and `app.note` = the
//! selected note. Menu, toolbar and context-menu commands for it arrive in `app.editor_cmds`
//! (Notes' menu labels, "Bold", "Checklist", "Table", …), and each frame it reports what the
//! menus need in `app.editor_state`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Color32, Rect};
use nt_core::model::{AttachmentKind, Folder, FolderKind, Library, Note, ParaStyle};

use crate::look::{self, Look};
use crate::settings::Settings;

/// The columns' widths as Notes allows them: the sidebar 150–380 pt (dragged under 100 it
/// closes), the note list 180–500 pt, the editor at least 300 pt.
pub const SIDEBAR_MIN: f32 = 150.0;
pub const SIDEBAR_MAX: f32 = 380.0;
const SIDEBAR_COLLAPSE: f32 = 100.0;
pub const LIST_MIN: f32 = 180.0;
pub const LIST_MAX: f32 = 500.0;
const EDITOR_MIN: f32 = 300.0;

/// What the sidebar has selected (what the note list shows).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// "All iCloud", "All on This Device": every note of the account.
    All(String),
    /// A folder (or Smart Folder).
    Folder(String),
    /// A tag in the Tags browser ("" = All Tags).
    Tag(String),
    /// An account's Recently Deleted.
    Deleted(String),
    /// Shared with others.
    Shared,
}

/// What the editor reports each frame (for the menus' greyed and checked states).
#[derive(Clone, Debug, Default)]
pub struct EditorState {
    pub focused: bool,
    pub has_selection: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub style: Option<ParaStyle>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub in_table: bool,
    pub in_checklist: bool,
    pub on_link: bool,
    pub on_attachment: bool,
    /// Edit > Substitutions > Data Detectors
    pub data_detectors: bool,
    /// the selected paragraphs' writing direction when they agree (1 natural, 0 LTR, 2 RTL)
    pub direction: Option<u32>,
}

/// Which column has the keyboard (the list's selection is yellow only while it has).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    #[default]
    List,
    Editor,
}

/// A folder's own View > Sort By / Group By Date (None: Settings' default).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FolderView {
    pub sort: Option<u8>,
    pub oldest_first: bool,
    pub group: Option<bool>,
}

/// Sheets and alerts.
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    DeleteFolder(String),
    DeleteForever(Vec<String>),
    /// Import to Notes? for these files and folders (not Mac libraries: those go straight in)
    Import(Vec<PathBuf>),
    SmartFolder { name: String, smart: bool, any: bool, rules: Vec<crate::smart::Rule>, convert: Option<String> },
    /// Settings > Locked notes > Set Password… (or Lock Note without one): then lock `then_lock`.
    /// `old` is asked for when changing a password.
    SetPassword { account: String, old: Option<String>, password: String, verify: String, hint: String, then_lock: Option<String> },
    /// The notes password, to lock a note or remove its lock (`remove`).
    EnterPassword { note: String, remove: bool, password: String, wrong: bool },
    /// File > Print…: the note's pages (a PDF made on a worker), the printers, copies
    Print { pdf: PathBuf, printers: Vec<String>, printer: usize, copies: u32 },
    /// Settings > Locked notes > Use Fingerprint (Windows Hello, Touch ID) turned on: the notes
    /// password, to keep in the system's secret store.
    BiometricPassword { account: String, password: String, wrong: bool },
    /// Importing a Mac library with locked notes: its notes password.
    /// Edit > Add Link… (⌘K): Link To (a URL or a note's title), Name or Use Note Title.
    AddLink { link: String, name: String, use_title: bool },
    Alert(String, String),
    /// Settings > Locked notes > Reset Password…: asks, then the Set Password sheet (no old password)
    ResetPassword(String),
    /// Settings > Sync > Replace…: asks first (Push: the server's copy replaced by this library;
    /// Pull: this library by the server's)
    SyncReplace(nt_core::sync::Force),
    /// Edit > Record Audio…: the recording sheet (the recorder itself is App.recorder)
    RecordAudio,
    /// File > Export…: the formats and their options
    Export(crate::export_sheet::Choices),
    /// Help > Keyboard Shortcuts
    Shortcuts,
}

/// A folder whose name the sidebar is editing (inline, as Notes does).
#[derive(Clone, Debug, PartialEq)]
pub struct Renaming {
    pub folder: String,
    pub text: String,
    pub fresh: bool,
}

/// What a note's list row shows on its right (the first image, PDF or drawing).
#[derive(Clone, Debug, PartialEq)]
pub enum Thumb {
    Image(std::sync::Arc<[u8]>),
    File(PathBuf),
    Pdf,
    Drawing,
}

pub struct App {
    pub library: Library,
    /// lookups and views worked out once and kept until the library changes (a big library:
    /// nothing per frame goes through every note)
    cache: Caches,
    /// work on workers (exports, printing), what each comes back with
    pub(crate) jobs: Vec<std::sync::mpsc::Receiver<JobDone>>,
    /// how far the note is scrolled from its top, in points (0 at rest): the editor sets it each
    /// frame (the toolbar's edge shows a hairline once it's scrolled)
    pub editor_scroll_y: f32,
    /// the app's own library on its way in (opened on a worker at startup)
    loading: Option<std::sync::mpsc::Receiver<Result<(crate::store::Store, Library), String>>>,
    /// frames drawn (the caches' per-frame parts go by it)
    frame: u64,
    /// the app's own library (None: the demo or a Mac store, read only, as in snapshots)
    pub db: Option<crate::store::Store>,
    /// notes whose text changed and isn't saved yet: id -> when
    pub dirty: HashMap<String, f64>,
    pub store: Option<PathBuf>,
    pub place: Place,
    pub note: Option<String>,
    pub editor_cmds: Vec<String>,
    pub editor_state: EditorState,
    pub look: Look,
    pub settings: Settings,
    pub show_settings: bool,
    pub gallery: bool,
    /// Gallery view: a note opened from its card (the editor fills the window).
    pub gallery_open: bool,
    /// View > Show Attachments Browser: open (in place of the list and the note)
    pub browser: Option<crate::attachments::Browser>,
    /// `bramble-notes --quick-note` (bound to a desktop shortcut, the Mac's fn-Q): a Quick Note once
    /// the library is open
    pub quick_note: bool,
    /// How far the note's rect reaches up under the toolbar's strip (pt): the editor starts its
    /// content below it and draws its text and background up under the translucent strip
    pub editor_top_inset: f32,
    /// Settings > Sync: the WebDAV account and the syncs it runs
    pub sync: crate::sync::Sync,
    /// Window > Open Note in New Window: the note windows, and the one used last (Float on Top)
    pub note_windows: Vec<crate::windows::NoteWindow>,
    pub last_note_window: Option<egui::ViewportId>,
    pub window_seq: u64,
    /// Edit > Record Audio…: the microphone while recording, and the last take when stopped
    pub recorder: Option<crate::audio::Recorder>,
    pub take: Option<(Vec<f32>, u32, u16)>,
    /// the note list's swipe actions (the row slid aside, the gesture)
    pub swipe: crate::swipe::Swipe,
    /// the list's multi-selection (⇧-click, ⌘-click, ⇧↑↓, ⌘A): the notes picked besides `note`
    /// (empty: just `note`), and where a ⇧ range starts
    pub picked: Vec<String>,
    pub anchor: Option<String>,
    /// Edit > Undo for the library's changes (delete, move, pin, new / delete folder)
    pub undo: crate::undo::Undo,
    /// the library's files as last seen, to notice another process writing them
    pub disk: crate::store::DiskWatch,
    /// scripted snapshots: when the script ran out (for the time-out)
    snap_idle: Option<std::time::Instant>,
    pub show_sidebar: bool,
    pub show_toolbar: bool,
    pub show_counts: bool,
    pub sidebar_w: f32,
    pub list_w: f32,
    pub focus: Focus,
    pub tb: crate::toolbar::TbState,
    pub search: String,
    pub collapsed: HashSet<String>,
    pub pinned_collapsed: bool,
    pub views: HashMap<String, FolderView>,
    pub thumbs: HashMap<String, Thumb>,
    /// note id -> (the picture it was made from, its texture, the frame it was last drawn in)
    pub thumb_tex: HashMap<String, (String, egui::TextureHandle, u64)>,
    /// the thumbnail worker (pictures decoded off the UI thread)
    pub thumb_jobs: ThumbJobs,
    pub dialog: Option<Dialog>,
    pub renaming: Option<Renaming>,
    pub recent: Vec<String>,
    pub message: Option<(String, f64)>,
    pub zoom: f32,
    pub list_scroll_to: bool,
    pub lock: crate::lock::LockState,
    /// the list's, gallery's and sidebar's scrolling (the editor's Scroller: Letters' feel)
    pub list_scroll: crate::editor::scroll::Scroller,
    pub sidebar_scroll: crate::editor::scroll::Scroller,
    // window chrome
    pub buttons: kn_ui::winframe::ButtonLayout,
    pub menu_bar_width: Option<f32>,
    pub title_press_at: f64,
    pub corner: f32,
    /// window drags started from the title bar (NT_DUMP reports it), and where Settings is
    pub window_drags: u32,
    pub settings_rect: Option<Rect>,
    /// the window's own rect this frame (inside the shadow margin)
    pub window_rect: Rect,
    /// hidden title-bar menus shown for now (Alt / F10), and the Alt tap being watched
    pub menus_peek: bool,
    pub alt_down: bool,
    pub alt_clean: bool,
    // test hooks
    snap: Option<(PathBuf, u32)>,
    script: std::collections::VecDeque<Vec<egui::Event>>,
    script_cmds: Vec<String>,
    pub automated: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext, store: Option<PathBuf>) -> App {
        let _new = std::env::var_os("NT_PERF").is_some().then(|| crate::PhaseTimer::new("App::new"));
        let snapping = std::env::var_os("NT_SNAP").is_some();
        let mut look = Look::load();
        if let Ok(v) = std::env::var("NT_DARK") {
            look.dark = v != "0";
        } else if snapping && std::env::var_os("NT_CONFIG").is_none() {
            // (NT_CONFIG=1 keeps the look.txt of the test's own XDG_CONFIG_HOME, as the live
            // window has it: glass, toolbar background, dark)
            look = Look { system_title_bar: look.system_title_bar, ..Look::default() };
        }
        // NT_GLASS=1/0: liquid glass on or off (snapshots)
        if let Ok(v) = std::env::var("NT_GLASS") {
            look.liquid_glass = v != "0";
        }
        {
            let _t = std::env::var_os("NT_PERF").is_some().then(|| crate::PhaseTimer::new("look+fonts"));
            look.apply(&cc.egui_ctx);
        }
        // a wheel notch goes as far as in GTK apps (Letters'), for egui's own scroll areas too
        cc.egui_ctx.options_mut(|o| o.input_options.line_scroll_speed = 100.0);
        // (scripted runs use the defaults, unless NT_CONFIG=1 asks for the settings files: a test of
        // its own XDG_CONFIG_HOME)
        let settings = if snapping && std::env::var_os("NT_CONFIG").is_none() { Settings::default() } else { Settings::load() };
        let widths = (settings.sidebar_w.clamp(SIDEBAR_MIN, SIDEBAR_MAX), settings.list_w.clamp(LIST_MIN, LIST_MAX));
        // (the demo is built only when no library is opened, or opening one fails: a big one
        // shows sooner)
        let own = store.is_none() && (!snapping || std::env::var_os("NT_LIBRARY").is_some());
        let (library, thumbs) = if own || store.is_some() {
            (Library::default(), Default::default())
        } else {
            let _t = std::env::var_os("NT_PERF").is_some().then(|| crate::PhaseTimer::new("demo library"));
            crate::demo::library()
        };
        let mut app = App {
            library,
            db: None,
            dirty: HashMap::new(),
            store: None,
            place: Place::Folder("local-bramble".into()),
            note: None,
            editor_cmds: Vec::new(),
            editor_state: EditorState::default(),
            look,
            settings,
            show_settings: false,
            gallery: false,
            gallery_open: false,
            show_sidebar: std::env::var_os("NT_NOSIDEBAR").is_none(),
            show_toolbar: true,
            show_counts: true,
            sidebar_w: widths.0,
            list_w: widths.1,
            focus: Focus::List,
            tb: Default::default(),
            search: String::new(),
            collapsed: HashSet::new(),
            pinned_collapsed: false,
            views: HashMap::new(),
            thumbs: thumbs.into_iter().collect(),
            thumb_tex: HashMap::new(),
            browser: None,
            quick_note: std::env::args().any(|a| a == "--quick-note"),
            snap_idle: None,
            editor_top_inset: 0.0,
            sync: Default::default(),
            note_windows: Vec::new(),
            last_note_window: None,
            window_seq: 0,
            recorder: None,
            take: None,
            swipe: Default::default(),
            picked: Vec::new(),
            anchor: None,
            undo: Default::default(),
            disk: Default::default(),
            thumb_jobs: ThumbJobs::default(),
            cache: Caches::default(),
            jobs: Vec::new(),
            editor_scroll_y: 0.0,
            loading: None,
            frame: 0,
            dialog: None,
            renaming: None,
            recent: Vec::new(),
            message: None,
            zoom: 1.0,
            list_scroll_to: false,
            lock: Default::default(),
            list_scroll: Default::default(),
            sidebar_scroll: Default::default(),
            buttons: kn_ui::winframe::ButtonLayout::from_desktop(),
            menu_bar_width: None,
            title_press_at: -1.0,
            corner: 0.0,
            window_drags: 0,
            settings_rect: None,
            window_rect: Rect::NOTHING,
            menus_peek: false,
            alt_down: false,
            alt_clean: false,
            snap: std::env::var_os("NT_SNAP").map(|p| (PathBuf::from(p), 0)),
            script: std::env::var("NT_SCRIPT").map(|s| parse_script(&s)).unwrap_or_default(),
            script_cmds: Vec::new(),
            automated: snapping,
        };
        // the app's own library, saved as it changes (snapshots keep the demo unless NT_LIBRARY
        // names a library of their own; a store named on the command line is shown read only)
        if store.is_none() && (!snapping || std::env::var_os("NT_LIBRARY").is_some()) {
            if app.automated && std::env::var_os("NT_PERF").is_none() {
                // (scripted snapshots: at once, so they're the same every time)
                app.own_library_opened(load_own());
            } else {
                // the window shows at once; the library is opened and read on a worker and comes
                // in a few frames later (a big library doesn't hold the first frame back)
                let (tx, rx) = std::sync::mpsc::channel();
                let ctx = cc.egui_ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(load_own());
                    ctx.request_repaint();
                });
                app.loading = Some(rx);
            }
        }
        // a library named on the command line is the window's library (instead of the demo)
        if let Some(p) = store {
            match crate::store::open(&p) {
                Ok(lib) => {
                    app.library = lib;
                    app.thumbs.clear();
                    app.store = Some(p);
                    let first = app.library.folders.iter().find(|f| f.default || f.name == "Notes").or(app.library.folders.first()).map(|f| f.id.clone());
                    app.place = match first {
                        Some(f) if app.library.folders.iter().filter(|x| x.kind != FolderKind::Trash).count() > 1 => Place::All(app.folder(&f).map(|x| x.account.clone()).unwrap_or_default()),
                        Some(f) => Place::Folder(f),
                        None => Place::Shared,
                    };
                }
                Err(e) => {
                    if app.library.notes.is_empty() {
                        let (lib, thumbs) = crate::demo::library();
                        app.library = lib;
                        app.thumbs = thumbs.into_iter().collect();
                    }
                    app.dialog = Some(Dialog::Alert("Import to Notes failed.".into(), e));
                }
            }
        }
        app.select_first();
        // NT_NOTE=<title>: open that note (test snapshots)
        if let Ok(t) = std::env::var("NT_NOTE") {
            if let Some(id) = app.library.notes.iter().find(|n| n.title == t).map(|n| n.id.clone()) {
                app.go(Place::Folder(String::new()));
                crate::menus::menu_action(&mut app, &cc.egui_ctx, &format!("note:{id}"));
            }
        }
        // NT_LIST_OFFSET=<pt>: the list scrolled that far (test snapshots)
        if let Some(o) = std::env::var("NT_LIST_OFFSET").ok().and_then(|v| v.parse::<f32>().ok()) {
            app.list_scroll.offset = o;
        }
        // NT_SIDEBAR_OFFSET=<pt>: the sidebar scrolled that far (test snapshots)
        if let Some(o) = std::env::var("NT_SIDEBAR_OFFSET").ok().and_then(|v| v.parse::<f32>().ok()) {
            app.sidebar_scroll.offset = o;
        }
        app
    }

    // ---- the library -------------------------------------------------------------------

    pub fn folder(&self, id: &str) -> Option<&Folder> {
        let at = |i: usize| self.library.folders.get(i).filter(|f| f.id == id);
        if let Some(f) = self.cache.folder_at.borrow().get(id).copied().and_then(at) {
            return Some(f);
        }
        // (the folders changed since: index them again)
        let map: HashMap<String, usize> = self.library.folders.iter().enumerate().map(|(i, f)| (f.id.clone(), i)).collect();
        let i = map.get(id).copied();
        *self.cache.folder_at.borrow_mut() = map;
        i.and_then(at)
    }

    pub fn is_smart(f: &Folder) -> bool {
        f.smart.is_some() || f.kind == FolderKind::Smart
    }

    /// An account's name as shown (see `account_label`).
    pub fn account_name(&self, id: &str) -> String {
        self.library.accounts.iter().find(|a| a.id == id).map(account_label).unwrap_or_default()
    }

    /// The selected note as a whole (its body and files): the library holds light notes (their
    /// list fields) until one is opened.
    pub fn ensure_full_selected(&mut self) {
        let Some(id) = self.note.clone() else { return };
        if !self.selected_note().is_some_and(|n| n.light) {
            return;
        }
        let Some(full) = self.db.as_ref().and_then(|st| st.note(&id).ok()) else { return };
        if std::env::var_os("NT_DUMP").is_some() {
            if let Some(l) = self.selected_note() {
                eprintln!("full note {id}: light created={:.0} modified={:.0} -> full created={:.0} modified={:.0}", l.created, l.modified, full.created, full.modified);
            }
        }
        if let Some(n) = self.library.notes.iter_mut().find(|n| n.id == id) {
            *n = full;
        }
    }

    /// Whether the app's own library is still on its way in (the first frames).
    pub fn loading(&self) -> bool {
        self.loading.is_some()
    }

    /// The app's own library, opened (on the worker at startup, or at once in snapshots).
    pub fn own_library_opened(&mut self, opened: Result<(crate::store::Store, Library), String>) {
        match opened {
            Ok((st, lib)) => {
                self.library = lib;
                self.thumbs.clear();
                self.db = Some(st);
                let acct = self.library.accounts.first().map(|a| a.id.clone()).unwrap_or_default();
                if !self.library.accounts.iter().any(|a| a.id == self.settings.default_account) {
                    self.settings.default_account = acct.clone();
                }
                if !self.library.accounts.iter().any(|a| a.id == self.settings.locked_account) {
                    self.settings.locked_account = acct;
                }
                self.place = Place::Folder(nt_core::store::DEFAULT_FOLDER.into());
                if self.library.folders.iter().filter(|f| f.kind != FolderKind::Trash && !Self::is_smart(f)).count() > 1 {
                    self.place = Place::All(nt_core::store::ACCOUNT_ID.into());
                }
                self.select_first();
                crate::sync::opened(self);
                // the library converted to the native format on this open (once), or not
                let migrated = self.db.as_ref().and_then(|st| st.migration().cloned());
                match migrated {
                    Some(Ok(m)) => self.notify(format!("Library updated: {} notes, {} folders, {} attachments ({:.0} s). A backup is beside it.", m.notes, m.folders, m.attachments, m.seconds)),
                    Some(Err(msg)) => self.dialog = Some(Dialog::Alert("Couldn’t update the library.".into(), format!("Couldn't update the library: {msg}. Your notes are unchanged."))),
                    None => {}
                }
                if std::mem::take(&mut self.quick_note) {
                    self.open_quick_note();
                }
            }
            Err(e) => {
                let (lib, thumbs) = crate::demo::library();
                self.library = lib;
                self.thumbs = thumbs.into_iter().collect();
                self.dialog = Some(Dialog::Alert("Notes couldn’t open its library.".into(), format!("{e}\n\nThe demo notes are shown instead; changes won’t be saved.")));
                self.select_first();
            }
        }
    }

    pub fn note_by_id(&self, id: &str) -> Option<&Note> {
        let at = |i: usize| self.library.notes.get(i).filter(|n| n.id == id);
        if let Some(n) = self.cache.note_at.borrow().get(id).copied().and_then(at) {
            return Some(n);
        }
        // (the notes changed since: index them again)
        let map: HashMap<String, usize> = self.library.notes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();
        let i = map.get(id).copied();
        *self.cache.note_at.borrow_mut() = map;
        i.and_then(at)
    }

    /// A cheap fingerprint of everything the list, counts and tags depend on: each note's dates,
    /// flags, folder and title, and the folders (any edit moves a note's date).
    fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.library.notes.len().hash(&mut h);
        for n in &self.library.notes {
            n.modified.to_bits().hash(&mut h);
            n.created.to_bits().hash(&mut h);
            (n.pinned, n.deleted, n.locked).hash(&mut h);
            n.folder.hash(&mut h);
            n.title.len().hash(&mut h);
        }
        for f in &self.library.folders {
            (&f.id, &f.smart, &f.account).hash(&mut h);
        }
        h.finish()
    }

    /// The fingerprint, worked out once per frame (`frame`).
    fn frame_fingerprint(&self) -> u64 {
        let mut c = self.cache.frame_fp.borrow_mut();
        if c.0 != self.frame || c.0 == 0 {
            *c = (self.frame, self.fingerprint());
        }
        c.1
    }

    pub fn selected_note(&self) -> Option<&Note> {
        self.note.as_deref().and_then(|id| self.note_by_id(id))
    }

    pub fn selected_note_mut(&mut self) -> Option<&mut Note> {
        let id = self.note.clone()?;
        self.library.notes.iter_mut().find(|n| n.id == id)
    }

    /// The editor changed the selected note: its edit date moves on (the list re-sorts), its
    /// title and snippet follow its first lines.
    pub fn note_changed(&mut self) {
        let now = crate::dates::now();
        if let Some(id) = self.note.clone() {
            self.dirty.insert(id, now);
        }
        if let Some(n) = self.selected_note_mut() {
            n.modified = now;
            let lines: Vec<String> = n.body.paragraphs.iter().map(|p| p.text().replace(nt_core::model::OBJ, "")).filter(|t| !t.trim().is_empty()).collect();
            n.title = lines.first().map(|t| t.trim().to_string()).unwrap_or_default();
            n.snippet = lines.get(1).map(|s| s.trim().to_string()).unwrap_or_default();
        }
    }

    pub fn account_of(&self, n: &Note) -> String {
        self.folder(&n.folder).map(|f| f.account.clone()).unwrap_or_default()
    }

    /// The tags used in a note: its tag tokens, or "#word"s in its text (the demo's).
    pub fn tags_of(n: &Note) -> Vec<String> {
        let mut out = n.tags();
        if !out.is_empty() {
            return out;
        }
        let text = format!("{}\n{}", n.body.text(), n.snippet);
        for (i, _) in text.match_indices('#') {
            if i > 0 && !text[..i].ends_with(char::is_whitespace) {
                continue;
            }
            let w: String = text[i + 1..].chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect();
            if !w.is_empty() && !out.contains(&w) {
                out.push(w);
            }
        }
        out
    }

    /// Every tag in the library, sorted (kept until the library changes).
    pub fn all_tags(&self) -> Vec<String> {
        let fp = self.frame_fingerprint();
        if let Some((k, t)) = self.cache.tags.borrow().as_ref() {
            if *k == fp {
                return t.clone();
            }
        }
        let t = self.all_tags_now();
        *self.cache.tags.borrow_mut() = Some((fp, t.clone()));
        t
    }

    fn all_tags_now(&self) -> Vec<String> {
        // (only tags some note uses, as Notes' tag browser: a tag left on no note drops out, even
        // though its ICHashtag row stays in the store)
        let mut t: Vec<String> = self.library.notes.iter().filter(|n| !n.deleted).flat_map(Self::tags_of).collect();
        // (no empty tag: its chip would be a second "All Tags", the same widget)
        t.retain(|s| !s.trim().is_empty());
        t.sort_by_key(|s| s.to_lowercase());
        t.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        t
    }

    fn smart_matches(&self, query: &str, n: &Note) -> bool {
        // (each query read once and kept: this runs for every note)
        let q = {
            let mut c = self.cache.queries.borrow_mut();
            c.entry(query.to_string()).or_insert_with(|| crate::smart::parse(query)).clone()
        };
        let tags = Self::tags_of(n);
        // (light notes carry these from the store's columns: no note is read for them)
        let checklist = n.checklist_counts();
        let kinds: Vec<AttachmentKind> = n.attachment_kinds();
        let facts = crate::smart::Facts {
            tags: &tags,
            quick_note: self.folder(&n.folder).is_some_and(|f| f.name == "Quick Notes" && f.parent.is_none()),
            checklist,
            kinds: &kinds,
            has_attachment: !kinds.is_empty() || n.thumbnail().is_some(),
        };
        crate::smart::matches(&q, n, &facts, crate::dates::now())
    }

    /// The notes `place` shows (not in order).
    pub fn notes_in(&self, place: &Place) -> Vec<&Note> {
        self.library
            .notes
            .iter()
            .filter(|n| match place {
                Place::Deleted(a) => n.deleted && self.account_of(n) == *a,
                _ if n.deleted => false,
                Place::All(a) => self.account_of(n) == *a,
                Place::Folder(f) => match self.folder(f) {
                    Some(fo) if Self::is_smart(fo) => self.smart_matches(fo.smart.as_deref().unwrap_or(""), n),
                    _ => n.folder == *f,
                },
                // the tag browser's selection (one tag, several, some excluded; all or any)
                Place::Tag(t) => crate::smart::TagSelection::parse(t).matches(&Self::tags_of(n)),
                Place::Shared => false,
            })
            .collect()
    }

    /// How many notes `place` shows (kept until the library changes).
    pub fn count(&self, place: &Place) -> usize {
        let fp = self.frame_fingerprint();
        let key = Self::place_key(place);
        {
            let mut c = self.cache.counts.borrow_mut();
            if c.0 != fp {
                *c = (fp, HashMap::new());
            }
            if let Some(n) = c.1.get(&key) {
                return *n;
            }
        }
        let n = self.notes_in(place).len();
        self.cache.counts.borrow_mut().1.insert(key, n);
        n
    }

    fn place_key(place: &Place) -> String {
        match place {
            Place::All(a) => format!("all:{a}"),
            Place::Folder(f) => f.clone(),
            Place::Tag(t) => format!("tag:{t}"),
            Place::Deleted(a) => format!("deleted:{a}"),
            Place::Shared => "shared".into(),
        }
    }

    pub fn view_of(&self, place: &Place) -> FolderView {
        self.views.get(&Self::place_key(place)).copied().unwrap_or_default()
    }

    pub fn view(&self) -> FolderView {
        self.view_of(&self.place)
    }

    pub fn view_mut_of(&mut self, place: &Place) -> &mut FolderView {
        self.views.entry(Self::place_key(place)).or_default()
    }

    /// 0 Date Edited, 1 Date Created, 2 Title.
    pub fn sort(&self) -> u8 {
        self.view().sort.unwrap_or(self.settings.sort)
    }

    pub fn grouped(&self) -> bool {
        self.sort() != 2 && self.view().group.unwrap_or(self.settings.group_by_date)
    }

    /// The list's title: the folder's name, "All iCloud", the tag…
    pub fn place_title(&self) -> String {
        if !self.search.trim().is_empty() {
            return "Search".into();
        }
        match &self.place {
            Place::All(a) => all_label(&self.account_name(a)),
            Place::Folder(f) => self.folder(f).map(|f| f.name.clone()).unwrap_or_default(),
            Place::Tag(t) => crate::smart::TagSelection::parse(t).title(),
            Place::Deleted(_) => "Recently Deleted".into(),
            Place::Shared => "Shared".into(),
        }
    }

    pub fn matches_search(&self, n: &Note) -> bool {
        let q = self.search.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        // the app's library: nt-core's search over every note's text (light notes have none
        // here), asked once per query and kept until the library changes
        if let Some(st) = &self.db {
            let fp = self.frame_fingerprint();
            let mut c = self.cache.search.borrow_mut();
            if c.as_ref().is_none_or(|(k, f, _)| *k != q || *f != fp) {
                let ids: HashSet<String> = st.search(&q).unwrap_or_default().into_iter().collect();
                *c = Some((q.clone(), fp, ids));
            }
            return c.as_ref().is_some_and(|(_, _, ids)| ids.contains(&n.id)) || n.snippet.to_lowercase().contains(&q);
        }
        n.title.to_lowercase().contains(&q) || n.snippet.to_lowercase().contains(&q) || n.body.text().to_lowercase().contains(&q)
    }

    /// The list's sections in order: (header, note ids). "Pinned" first, then date groups (or one
    /// unnamed section). A search lists every account's notes that match.
    pub fn sections(&self) -> Vec<(String, Vec<String>)> {
        // (worked out again only when what it depends on changes: the library, the place, the
        // search, the sort, or the day)
        let key = format!("{}|{}|{}|{}|{}|{}|{}", Self::place_key(&self.place), self.search, self.sort(), self.grouped(), self.view().oldest_first, (crate::dates::now() / 86400.0).floor(), self.fingerprint());
        if let Some((k, v)) = self.cache.sections.borrow().as_ref() {
            if *k == key {
                return v.clone();
            }
        }
        let v = self.sections_now();
        *self.cache.sections.borrow_mut() = Some((key, v.clone()));
        v
    }

    fn sections_now(&self) -> Vec<(String, Vec<String>)> {
        let now = crate::dates::now();
        let searching = !self.search.trim().is_empty();
        let mut notes: Vec<&Note> = if searching { self.library.notes.iter().filter(|n| !n.deleted && self.matches_search(n)).collect() } else { self.notes_in(&self.place) };
        let sort = self.sort();
        let oldest = self.view().oldest_first;
        let key = |n: &Note| if sort == 1 { n.created } else { n.modified };
        notes.sort_by(|a, b| {
            if sort == 2 {
                a.title.to_lowercase().cmp(&b.title.to_lowercase())
            } else {
                let o = key(b).partial_cmp(&key(a)).unwrap_or(std::cmp::Ordering::Equal);
                if oldest { o.reverse() } else { o }
            }
        });
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        if searching {
            out.push(("Top Hits".into(), notes.iter().take(3).map(|n| n.id.clone()).collect()));
            let rest: Vec<String> = notes.iter().skip(3).map(|n| n.id.clone()).collect();
            if !rest.is_empty() {
                out.push(("Notes".into(), rest));
            }
            return out;
        }
        let deleted = matches!(self.place, Place::Deleted(_));
        if !deleted {
            let pinned: Vec<String> = notes.iter().filter(|n| n.pinned).map(|n| n.id.clone()).collect();
            if !pinned.is_empty() {
                out.push(("Pinned".into(), pinned));
            }
        }
        let rest: Vec<&&Note> = notes.iter().filter(|n| deleted || !n.pinned).collect();
        if self.grouped() {
            for n in rest {
                let (_, g) = crate::dates::group(key(n), now);
                match out.last_mut() {
                    Some((h, ids)) if *h == g => ids.push(n.id.clone()),
                    _ => out.push((g, vec![n.id.clone()])),
                }
            }
        } else if !rest.is_empty() {
            out.push((if out.is_empty() { String::new() } else { "Notes".into() }, rest.iter().map(|n| n.id.clone()).collect()));
        }
        out
    }

    /// The list's notes in order (a collapsed Pinned section's too).
    pub fn ordered(&self) -> Vec<String> {
        self.sections().into_iter().flat_map(|(_, ids)| ids).collect()
    }

    pub fn select_first(&mut self) {
        let order = self.ordered();
        if self.note.as_ref().is_some_and(|n| order.contains(n)) {
            return;
        }
        self.note = order.into_iter().next();
    }

    pub fn select_note(&mut self, id: &str) {
        // (one note chosen: the multi-selection ends, a ⇧ range starts from it)
        self.picked.clear();
        self.anchor = Some(id.to_string());
        self.lead_to(id);
    }

    /// The note that leads the selection (shown, and the one arrows move from), keeping the rest.
    fn lead_to(&mut self, id: &str) {
        if self.note.as_deref() != Some(id) {
            // (the note being left is saved first)
            if let Some(old) = self.note.clone().filter(|o| self.dirty.contains_key(o)) {
                crate::store::save_note(self, &old);
            }
            if let Some(old) = self.note.clone() {
                self.recent.retain(|r| *r != old);
                self.recent.insert(0, old);
                self.recent.truncate(10);
            }
            self.note = Some(id.to_string());
        }
    }

    pub fn go(&mut self, place: Place) {
        // (a folder chosen while the attachments browser shows: back to the list)
        self.browser = None;
        if self.place != place {
            self.place = place;
            self.list_scroll = Default::default();
            self.note = None;
            self.gallery_open = false;
            self.select_first();
        }
    }

    /// The first image, PDF or drawing of a note, for its list row and gallery card.
    pub fn thumb_of(&self, n: &Note) -> Option<Thumb> {
        if let Some(t) = self.thumbs.get(&n.id) {
            return Some(t.clone());
        }
        // (a light note keeps its list picture's file, without its attachments)
        if n.light {
            return n.thumbnail().map(|p| Thumb::File(p.to_path_buf()));
        }
        n.attachments.iter().find_map(|a| match a.kind {
            AttachmentKind::Image | AttachmentKind::Scan => a.preview.clone().or(a.file.clone()).map(Thumb::File),
            // (no preview: the PDF itself, its first page drawn)
            AttachmentKind::Pdf => Some(a.preview.clone().or(a.file.clone()).or(a.fallback.clone()).map(Thumb::File).unwrap_or(Thumb::Pdf)),
            AttachmentKind::Drawing => Some(a.fallback.clone().filter(|p| p.extension().is_some_and(|e| e == "png")).map(Thumb::File).unwrap_or(Thumb::Drawing)),
            _ => None,
        })
    }

    // ---- what the commands do ----------------------------------------------------------

    /// The folder new notes go in: the selected folder, else the account's Notes.
    pub fn target_folder(&self) -> Option<String> {
        if let Place::Folder(f) = &self.place {
            if self.folder(f).is_some_and(|f| !Self::is_smart(f) && f.kind != FolderKind::Trash) {
                return Some(f.clone());
            }
        }
        if self.db.is_some() {
            return Some(nt_core::store::DEFAULT_FOLDER.into());
        }
        let acct = match &self.place {
            Place::All(a) | Place::Deleted(a) => a.clone(),
            Place::Folder(f) => self.folder(f).map(|f| f.account.clone()).unwrap_or_else(|| self.settings.default_account.clone()),
            _ => self.settings.default_account.clone(),
        };
        let notes_folder = |acct: &str| self.library.folders.iter().find(|f| f.account == acct && (f.default || f.name == "Notes") && f.parent.is_none()).map(|f| f.id.clone());
        notes_folder(&acct).or_else(|| self.library.folders.iter().find(|f| !Self::is_smart(f)).map(|f| f.id.clone()))
    }

    /// A Quick Note: a new note in Quick Notes (made if there's none), or the last one when Settings'
    /// "Always resume to last Quick Note" is on.
    pub fn open_quick_note(&mut self) {
        let quick = self.library.folders.iter().find(|f| f.name == "Quick Notes" && f.parent.is_none() && f.kind == FolderKind::Normal && !Self::is_smart(f)).map(|f| f.id.clone());
        let quick = match quick {
            Some(q) => q,
            None if self.db.is_some() => {
                let Some(q) = crate::store::with_db(self, "Making Quick Notes", |st| st.create_folder("Quick Notes", None)) else { return };
                crate::store::reload(self);
                q
            }
            None => return,
        };
        self.go(Place::Folder(quick.clone()));
        if self.settings.resume_quick_note {
            let last = self.library.notes.iter().filter(|n| n.folder == quick && !n.deleted).max_by(|a, b| a.modified.total_cmp(&b.modified)).map(|n| n.id.clone());
            if let Some(id) = last {
                self.select_note(&id);
                self.focus = Focus::Editor;
                return;
            }
        }
        self.new_note();
    }

    pub fn new_note(&mut self) {
        let Some(folder) = self.target_folder() else { return };
        let shows_folder = matches!(&self.place, Place::Folder(f) if *f == folder) || matches!(&self.place, Place::All(a) if self.folder(&folder).is_some_and(|f| f.account == *a));
        if !shows_folder {
            self.place = Place::Folder(folder.clone());
        }
        let now = crate::dates::now();
        let id = format!("NEW-{:X}-{}", now as u64, self.library.notes.len());
        let style = match self.settings.new_note_style {
            0 => ParaStyle::Title,
            1 => ParaStyle::Heading,
            2 => ParaStyle::Subheading,
            _ => ParaStyle::Body,
        };
        let mut body = nt_core::model::Body { paragraphs: vec![nt_core::model::Paragraph { style, ..Default::default() }] };
        let id = if self.db.is_some() {
            let f = folder.clone();
            let made = {
                let _t = crate::PartTimer::start("store create_note");
                crate::store::with_db(self, "Creating the note", |st| st.create_note(Some(&f), &mut body))
            };
            let Some(made) = made else { return };
            let nid = made.id.clone();
            self.library.notes.push(made);
            nid
        } else {
            self.library.notes.push(Note { id: id.clone(), folder, created: now, modified: now, body, ..Default::default() });
            id
        };
        self.search.clear();
        {
            let _t = crate::PartTimer::start("select the new note");
            self.select_note(&id);
        }
        self.focus = Focus::Editor;
        self.list_scroll_to = true;
        if self.gallery {
            self.gallery_open = true;
        }
    }

    /// Delete: to Recently Deleted (from there, for good after asking).
    pub fn delete_note(&mut self, id: &str) {
        self.delete_notes(&[id.to_string()]);
    }

    /// Delete for several notes (the selection): to Recently Deleted, as one Undo Delete Notes;
    /// those already there, for good after asking.
    pub fn delete_notes(&mut self, ids: &[String]) {
        let gone: Vec<String> = ids.iter().filter(|id| self.note_by_id(id).is_some_and(|n| n.deleted)).cloned().collect();
        if !gone.is_empty() {
            self.dialog = Some(Dialog::DeleteForever(gone));
            return;
        }
        let order = self.ordered();
        // (the next note after the last of them is selected, as on the Mac)
        let last = ids.iter().filter_map(|id| order.iter().position(|x| x == id)).max();
        let next = last.and_then(|i| order[i + 1..].iter().find(|x| !ids.contains(x)).or_else(|| order[..i].iter().rev().find(|x| !ids.contains(x)))).cloned();
        let mut done = Vec::new();
        for id in ids {
            let Some((folder, pinned)) = self.note_by_id(id).map(|n| (n.folder.clone(), n.pinned)) else { continue };
            if self.trash_note(id) {
                done.push((id.clone(), folder, pinned));
            }
        }
        if !done.is_empty() {
            self.undo.record(crate::undo::Op::Delete(done));
        }
        if ids.iter().any(|id| self.note.as_deref() == Some(id.as_str())) || !self.picked.is_empty() {
            self.picked.clear();
            self.note = next.clone();
            self.anchor = next;
        }
    }

    /// One note to Recently Deleted (no undo record, no selection change).
    pub fn trash_note(&mut self, id: &str) -> bool {
        if self.note_by_id(id).is_none_or(|n| n.deleted) {
            return false;
        }
        if self.dirty.contains_key(id) {
            crate::store::save_note(self, id);
        }
        if self.db.is_some() && crate::store::with_db(self, "Deleting the note", |st| st.delete_note(id)).is_none() {
            return false;
        }
        if let Some(n) = self.library.notes.iter_mut().find(|n| n.id == id) {
            n.deleted = true;
            n.pinned = false;
        }
        true
    }

    /// Undo Delete: the note out of Recently Deleted, into `folder`, pinned again if it was.
    pub fn restore_note(&mut self, id: &str, folder: &str, pinned: bool) {
        let to = Some(folder.to_string()).filter(|f| self.folder(f).is_some_and(|f| f.kind != FolderKind::Trash));
        if self.db.is_some() {
            let t = to.clone();
            if crate::store::with_db(self, "Restoring the note", |st| st.recover_note(id, t.as_deref())).is_none() {
                return;
            }
            if pinned {
                crate::store::with_db(self, "Pinning the note", |st| st.set_pinned(id, true));
            }
        }
        if let Some(n) = self.library.notes.iter_mut().find(|n| n.id == id) {
            n.deleted = false;
            n.folder = to.unwrap_or_else(|| nt_core::store::DEFAULT_FOLDER.into());
            n.pinned = pinned;
        }
    }

    /// The selection: the picked notes in the list's order, or just the selected one.
    pub fn selection(&self) -> Vec<String> {
        if self.picked.is_empty() {
            return self.note.clone().into_iter().collect();
        }
        let order = self.ordered();
        let mut v: Vec<String> = order.into_iter().filter(|id| self.picked.contains(id)).collect();
        // (picked notes the list no longer shows still count)
        for p in &self.picked {
            if !v.contains(p) {
                v.push(p.clone());
            }
        }
        v
    }

    pub fn is_selected(&self, id: &str) -> bool {
        self.note.as_deref() == Some(id) || self.picked.iter().any(|p| p == id)
    }

    /// These notes selected (the first leading).
    pub fn select_these(&mut self, ids: &[String]) {
        let Some(first) = ids.first() else { return };
        self.select_note(first);
        if ids.len() > 1 {
            self.picked = ids.to_vec();
        }
    }

    /// ⇧-click / ⇧↑↓: everything from the anchor to `id` selected, `id` leading.
    pub fn pick_range(&mut self, id: &str) {
        let order = self.ordered();
        let from = self.anchor.clone().or_else(|| self.note.clone()).unwrap_or_else(|| id.to_string());
        let (Some(a), Some(b)) = (order.iter().position(|x| *x == from), order.iter().position(|x| x == id)) else {
            self.select_note(id);
            return;
        };
        let (lo, hi) = (a.min(b), a.max(b));
        self.picked = order[lo..=hi].to_vec();
        self.anchor = Some(from);
        self.lead_to(id);
        if self.picked.len() == 1 {
            self.picked.clear();
        }
    }

    /// ⌘-click: `id` added to the selection or taken out of it.
    pub fn toggle_pick(&mut self, id: &str) {
        if self.picked.is_empty() {
            self.picked = self.note.clone().into_iter().collect();
        }
        if let Some(i) = self.picked.iter().position(|p| p == id) {
            self.picked.remove(i);
            match self.picked.len() {
                0 => self.note = None,
                1 => {
                    let only = self.picked[0].clone();
                    self.select_note(&only);
                }
                _ => {
                    if self.note.as_deref() == Some(id) {
                        let lead = self.picked.last().cloned().unwrap_or_default();
                        self.lead_to(&lead);
                    }
                }
            }
        } else {
            self.picked.push(id.to_string());
            self.anchor = Some(id.to_string());
            self.lead_to(id);
        }
    }

    /// ⌘A in the list: every note it shows.
    pub fn select_all_notes(&mut self) {
        let order = self.ordered();
        let lead = self.note.clone().filter(|n| order.contains(n)).or_else(|| order.first().cloned());
        let Some(lead) = lead else { return };
        self.picked = order;
        self.anchor = self.picked.first().cloned();
        self.lead_to(&lead);
        if self.picked.len() == 1 {
            self.picked.clear();
        }
    }

    pub fn delete_forever(&mut self, ids: &[String]) {
        for id in ids {
            self.dirty.remove(id);
            crate::store::with_db(self, "Deleting the note", |st| st.erase_note(id));
        }
        self.library.notes.retain(|n| !ids.contains(&n.id));
        self.note = None;
        self.select_first();
    }

    pub fn recover(&mut self, id: &str) {
        // (back to its folder when it still is one, else the Notes folder)
        let back = self.note_by_id(id).map(|n| n.folder.clone()).filter(|f| self.folder(f).is_some_and(|f| f.kind != FolderKind::Trash));
        if self.db.is_some() {
            let b = back.clone();
            if crate::store::with_db(self, "Recovering the note", |st| st.recover_note(id, b.as_deref())).is_none() {
                return;
            }
        }
        if let Some(n) = self.library.notes.iter_mut().find(|n| n.id == id) {
            n.deleted = false;
            if back.is_none() && n.folder == nt_core::store::TRASH_FOLDER {
                n.folder = nt_core::store::DEFAULT_FOLDER.into();
            }
        }
        self.note = None;
        self.select_first();
    }

    pub fn duplicate_note(&mut self, id: &str) {
        let Some(mut n) = self.note_by_id(id).cloned() else { return };
        // (a light note is read in full first: its body is what's copied)
        if n.light {
            if let Some(full) = self.db.as_ref().and_then(|st| st.note(id).ok()) {
                n = full;
            }
        }
        if self.db.is_some() {
            let (folder, mut body) = (n.folder.clone(), n.body.clone());
            if let Some(made) = crate::store::with_db(self, "Duplicating the note", |st| st.create_note(Some(&folder), &mut body)) {
                let nid = made.id.clone();
                self.library.notes.push(made);
                self.select_note(&nid);
            }
            return;
        }
        let now = crate::dates::now();
        n.id = format!("DUP-{:X}-{}", now as u64, self.library.notes.len());
        n.created = now;
        n.modified = now;
        n.pinned = false;
        let nid = n.id.clone();
        self.library.notes.push(n);
        self.select_note(&nid);
    }

    pub fn toggle_pin(&mut self, id: &str) {
        self.toggle_pins(&[id.to_string()]);
    }

    /// Pin Note for several (the selection): all pinned if any isn't, else all unpinned; one
    /// Undo Pin Notes.
    pub fn toggle_pins(&mut self, ids: &[String]) {
        let on = ids.iter().any(|id| !self.note_by_id(id).is_some_and(|n| n.pinned));
        let mut done = Vec::new();
        for id in ids {
            let was = self.note_by_id(id).is_some_and(|n| n.pinned);
            if was != on && self.set_pin(id, on) {
                done.push((id.clone(), was));
            }
        }
        if !done.is_empty() {
            self.undo.record(crate::undo::Op::Pin(done));
        }
    }

    /// One note pinned or not (no undo record).
    pub fn set_pin(&mut self, id: &str, on: bool) -> bool {
        if self.db.is_some() && crate::store::with_db(self, "Pinning the note", |st| st.set_pinned(id, on)).is_none() {
            return false;
        }
        if let Some(n) = self.library.notes.iter_mut().find(|n| n.id == id) {
            n.pinned = on;
        }
        true
    }

    pub fn move_note(&mut self, id: &str, folder: &str) {
        self.move_notes(&[id.to_string()], folder);
    }

    /// Move to for several (the selection); one Undo Move Notes.
    pub fn move_notes(&mut self, ids: &[String], folder: &str) {
        let name = self.folder(folder).map(|f| f.name.clone()).unwrap_or_default();
        let mut done = Vec::new();
        for id in ids {
            let Some(from) = self.note_by_id(id).map(|n| n.folder.clone()) else { continue };
            if from != folder && self.move_note_quiet(id, folder) {
                done.push((id.clone(), from, folder.to_string()));
            }
        }
        let k = done.len();
        if k > 0 {
            self.undo.record(crate::undo::Op::Move(done));
        }
        self.notify(format!("Moved {k} {} to {name}", if k == 1 { "note" } else { "notes" }));
        // (they've left the list: the selection starts again)
        self.picked.clear();
        self.select_first();
    }

    /// One note into `folder` (no undo record, no message).
    pub fn move_note_quiet(&mut self, id: &str, folder: &str) -> bool {
        if self.db.is_some() && crate::store::with_db(self, "Moving the note", |st| st.move_note(id, folder)).is_none() {
            return false;
        }
        if let Some(n) = self.library.notes.iter_mut().find(|n| n.id == id) {
            n.folder = folder.to_string();
            n.deleted = false;
        }
        true
    }

    /// A folder made (Undo Delete Folder, Redo New Folder): its id.
    pub fn make_folder(&mut self, name: &str, parent: Option<&str>, account: &str) -> Option<String> {
        let id = if self.db.is_some() {
            let (n, p, a) = (name.to_string(), parent.map(str::to_string), account.to_string());
            crate::store::with_db(self, "Creating the folder", |st| match p.as_deref() {
                Some(p) => st.create_folder(&n, Some(p)),
                None => st.create_folder_in(&a, &n),
            })?
        } else {
            format!("FOLDER-{:X}-{}", crate::dates::now() as u64, self.library.folders.len())
        };
        self.library.folders.push(Folder { id: id.clone(), account: account.to_string(), name: name.to_string(), parent: parent.map(str::to_string), ..Default::default() });
        if self.db.is_some() {
            crate::store::reload(self);
        }
        Some(id)
    }

    /// File > New Folder: "New Folder" in the selected account (inside `inside` when given),
    /// its name ready to type over.
    pub fn new_folder(&mut self, inside: Option<String>) {
        let acct = inside
            .as_ref()
            .and_then(|p| self.folder(p))
            .map(|f| f.account.clone())
            .or_else(|| match &self.place {
                Place::All(a) | Place::Deleted(a) => Some(a.clone()),
                Place::Folder(f) => self.folder(f).map(|f| f.account.clone()),
                _ => None,
            })
            .unwrap_or_else(|| self.settings.default_account.clone());
        let mut name = "New Folder".to_string();
        let mut k = 1;
        while self.library.folders.iter().any(|f| f.account == acct && f.name == name) {
            k += 1;
            name = format!("New Folder {k}");
        }
        let mut id = format!("FOLDER-{:X}-{}", crate::dates::now() as u64, self.library.folders.len());
        if self.db.is_some() {
            // (a top-level folder in the account it's made in: the selected folder's, or the one shown)
            let (n, par, a) = (name.clone(), inside.clone(), acct.clone());
            let Some(made) = crate::store::with_db(self, "Creating the folder", |st| match par.as_deref() {
                Some(p) => st.create_folder(&n, Some(p)),
                None => st.create_folder_in(&a, &n),
            }) else { return };
            id = made;
        }
        if let Some(p) = &inside {
            self.collapsed.remove(p);
        }
        self.library.folders.push(Folder { id: id.clone(), account: acct.clone(), name: name.clone(), parent: inside, ..Default::default() });
        self.collapsed.remove(&format!("acct:{acct}"));
        self.show_sidebar = true;
        self.undo.record(crate::undo::Op::NewFolder { id: id.clone(), name: name.clone(), parent: self.folder(&id).and_then(|f| f.parent.clone()), account: acct.clone() });
        self.go(Place::Folder(id.clone()));
        self.focus = Focus::Sidebar;
        self.renaming = Some(Renaming { folder: id, text: name, fresh: true });
    }

    pub fn rename_folder(&mut self, id: &str, name: &str) {
        let name = name.trim();
        if !name.is_empty() && self.db.is_some() && crate::store::with_db(self, "Renaming the folder", |st| st.rename_folder(id, name)).is_none() {
            return;
        }
        if let Some(f) = self.library.folders.iter_mut().find(|f| f.id == id).filter(|_| !name.is_empty()) {
            f.name = name.to_string();
        }
    }

    /// Delete Folder: its notes go to Recently Deleted, its subfolders with it; Undo Delete Folder
    /// makes them again and brings the notes back into them.
    pub fn delete_folder(&mut self, id: &str) {
        // the folder and its subfolders, parents first, and the notes in them
        let mut tree: Vec<Folder> = self.folder(id).cloned().into_iter().collect();
        let mut k = 0;
        while k < tree.len() {
            let pid = tree[k].id.clone();
            let kids: Vec<Folder> = self.library.folders.iter().filter(|f| f.parent.as_deref() == Some(pid.as_str())).cloned().collect();
            tree.extend(kids);
            k += 1;
        }
        let in_tree = |f: &str| tree.iter().any(|t| t.id == f);
        let notes: Vec<(String, String, bool)> = self.library.notes.iter().filter(|n| !n.deleted && in_tree(&n.folder)).map(|n| (n.id.clone(), n.folder.clone(), n.pinned)).collect();
        // (a smart folder's notes stay where they are: only the folder goes)
        let smart = self.folder(id).is_some_and(Self::is_smart);
        self.delete_folder_quiet(id);
        if !tree.is_empty() && !smart {
            self.undo.record(crate::undo::Op::DeleteFolder { folders: tree, notes });
        }
    }

    /// Delete Folder without an undo record.
    pub fn delete_folder_quiet(&mut self, id: &str) {
        if self.db.is_some() {
            crate::store::flush(self, f64::INFINITY, true);
            let acct = self.folder(id).map(|f| f.account.clone()).unwrap_or_default();
            if crate::store::with_db(self, "Deleting the folder", |st| st.delete_folder(id)).is_some() {
                crate::store::reload(self);
                if matches!(&self.place, Place::Folder(f) if self.folder(f).is_none()) {
                    self.go(Place::All(acct));
                }
            }
            return;
        }
        let mut gone = vec![id.to_string()];
        let mut k = 0;
        while k < gone.len() {
            let kids: Vec<String> = self.library.folders.iter().filter(|f| f.parent.as_deref() == Some(gone[k].as_str())).map(|f| f.id.clone()).collect();
            gone.extend(kids);
            k += 1;
        }
        let acct = self.folder(id).map(|f| f.account.clone()).unwrap_or_default();
        let home = self.library.folders.iter().find(|f| f.account == acct && (f.default || f.name == "Notes") && !gone.contains(&f.id)).map(|f| f.id.clone());
        for n in self.library.notes.iter_mut().filter(|n| gone.contains(&n.folder)) {
            if let Some(h) = &home {
                n.folder = h.clone();
            }
            n.deleted = true;
        }
        self.library.folders.retain(|f| !gone.contains(&f.id));
        if matches!(&self.place, Place::Folder(f) if gone.contains(f)) {
            self.go(Place::All(acct));
        }
    }

    /// File > Import to Notes… with a Mac's NoteStore.sqlite (or its group.com.apple.notes
    /// folder): its accounts and folders join the library.
    pub fn import_store(&mut self, p: &Path) {
        if self.db.is_some() {
            crate::store::import_mac(self, p);
            return;
        }
        match crate::store::open(p) {
            Ok(lib) => {
                let n = lib.notes.len();
                crate::store::merge(self, lib);
                self.store = Some(p.to_path_buf());
                self.notify(format!("Imported {n} notes"));
            }
            Err(e) => self.dialog = Some(Dialog::Alert("Import to Notes failed.".into(), e)),
        }
    }

    /// Runs `f` on a worker thread (never the window's): what it comes back with is taken in a
    /// later frame (a message shown, a sheet opened).
    pub fn background(&mut self, ctx: &egui::Context, f: impl FnOnce() -> JobDone + Send + 'static) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
            ctx.request_repaint();
        });
        self.jobs.push(rx);
    }

    /// The workers that have finished.
    fn take_jobs(&mut self) {
        let _t = crate::PartTimer::start("jobs done");
        let mut done = Vec::new();
        self.jobs.retain(|rx| match rx.try_recv() {
            Ok(d) => {
                done.push(d);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        for d in done {
            match d {
                JobDone::Message(m) => self.notify(m),
                JobDone::Print(pdf, printers) => self.dialog = Some(Dialog::Print { pdf, printers, printer: 0, copies: 1 }),
                JobDone::Import(tree, failed) => crate::dialogs::imported(self, tree, failed),
                JobDone::Reload(m) => {
                    if let Some(st) = self.db.as_mut() {
                        let _ = st.refresh();
                    }
                    self.disk.own = true;
                    crate::store::reload(self);
                    self.notify(m);
                }
                JobDone::Attach(p) => {
                    self.editor_cmds.push(format!("Attach File:{}", p.display()));
                    self.focus = Focus::Editor;
                }
                JobDone::Attachments(items) => {
                    if let Some(b) = &mut self.browser {
                        b.items = Some(items);
                    }
                }
            }
        }
    }

    pub fn notify(&mut self, s: impl Into<String>) {
        let s = s.into();
        if std::env::var_os("NT_DEBUG").is_some() {
            eprintln!("{s}");
        }
        self.message = Some((s, 0.0));
    }

    // ---- the window --------------------------------------------------------------------

    fn snapshot(&mut self, ctx: &egui::Context) {
        if std::env::var_os("NT_SCROLL_TRACE").is_some() {
            eprintln!("scroll t={:.3} list={:.1} sidebar={:.1}", ctx.input(|i| i.time), self.list_scroll.offset, self.sidebar_scroll.offset);
        }
        let Some((path, frames)) = &mut self.snap else { return };
        *frames += 1;
        // (the time-out counts from the script's end: a long script doesn't use it up)
        if self.script.is_empty() && self.snap_idle.is_none() {
            self.snap_idle = Some(std::time::Instant::now());
        }
        let n = *frames;
        let path = path.clone();
        for c in std::mem::take(&mut self.script_cmds) {
            crate::menus::run(self, ctx, &c);
        }
        // (work still on a worker, an export, is waited for; the editor's pictures for 10 s at most,
        // as a hidden editor doesn't take them in)
        let pictures = crate::editor::busy() && self.snap_idle.is_none_or(|t| t.elapsed().as_secs() < 10);
        if n > 20 && self.script.is_empty() && self.jobs.is_empty() && !pictures {
            if std::env::var_os("NT_DUMP").is_some() {
                eprintln!("content: {:?} pt, zoom {}", ctx.content_rect().size(), ctx.zoom_factor());
            }
            if std::env::var_os("NT_DUMP").is_some() {
                eprintln!("place: {:?}  note: {:?}  sections: {:?}", self.place, self.note, self.sections().iter().map(|(h, v)| (h.clone(), v.len())).collect::<Vec<_>>());
                eprintln!("window drags: {}  settings: {:?}  outer: {:?}", self.window_drags, self.settings_rect, ctx.input(|i| i.viewport().outer_rect));
                eprintln!("swipe: {}", self.swipe.describe());
                eprintln!("view: gallery={} attachments={} search_focus={}", self.gallery, self.browser.is_some(), ctx.data(|d| d.get_temp::<bool>(egui::Id::new("focus-search")).unwrap_or(false)));
                eprintln!("selection: {} (lead {:?})  undo: {:?}  redo: {:?}  folders: {}  deleted: {}  pinned: {}", self.selection().len(), self.note.as_ref().and_then(|n| self.note_by_id(n)).map(|n| n.title.clone()), self.undo.undo_label(), self.undo.redo_label(), self.library.folders.len(), self.library.notes.iter().filter(|n| n.deleted).count(), self.library.notes.iter().filter(|n| n.pinned && !n.deleted).count());
                eprintln!("note windows: {:?}", self.note_windows.iter().map(|w| (self.note_by_id(&w.note).map(|n| n.title.clone()).unwrap_or_default(), w.on_top)).collect::<Vec<_>>());
                let tick = |n: &str| crate::menus::checked(self, n) == Some(true);
                eprintln!("editor: direction={:?} data_detectors={}  ticks: default={} ltr={} rtl={} detectors={}", self.editor_state.direction, self.editor_state.data_detectors, tick("Writing Direction > Paragraph Default"), tick("Writing Direction > Paragraph Left to Right"), tick("Writing Direction > Paragraph Right to Left"), tick("Substitutions > Data Detectors"));
                for id in self.ordered().iter().take(4) {
                    if let Some(n) = self.note_by_id(id) {
                        eprintln!("note: {:?} locked={} open={} created={:.0} modified={:.0} text={:?}", n.title, n.locked, self.lock.unlocked.contains(id), n.created, n.modified, n.body.text().chars().take(40).collect::<String>());
                    }
                }
                eprintln!("library: {} notes, saved to {:?}", self.library.notes.len(), self.db.as_ref().map(|d| d.dir().to_path_buf()));
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
        } else if self.snap_idle.is_some_and(|t| t.elapsed().as_secs() > if self.jobs.is_empty() { 30 } else { 1200 }) {
            eprintln!("snapshot timed out");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }

    fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if let Some((_, t)) = &mut self.message {
            *t += ctx.input(|i| i.stable_dt) as f64;
            if *t > 3.0 {
                self.message = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        let window = ui.max_rect();
        self.window_rect = window;
        let integrated = !kn_ui::winframe::system_title_bar();
        if integrated {
            crate::chrome::window_edges(&ctx, window);
        }
        let r = self.corner.round() as u8;
        let p = ui.painter().clone();
        // the title bar row: menus and the window buttons (the suite's; Notes on the Mac has the
        // menus in the screen's menu bar)
        let title_h = if std::env::var_os("NT_NOTITLE").is_some() { 0.0 } else { crate::chrome::TITLE_H };
        let title = Rect::from_min_size(window.min, vec2(window.width(), title_h));
        p.rect_filled(title, egui::CornerRadius { nw: r, ne: r, sw: 0, se: 0 }, look::header_bg());
        ui.scope_builder(egui::UiBuilder::new().max_rect(title.shrink2(vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            if title_h == 0.0 {
            } else if integrated {
                let _t = crate::PartTimer::start("title bar");
                crate::chrome::title_bar(self, ui);
            } else {
                crate::menus::menu_bar(self, ui);
            }
        });
        let body = Rect::from_min_max(pos2(window.min.x, title.max.y), window.max);
        // the title row's hairline (Letters' panel separator), where the columns' dividers end
        p.rect_filled(body, egui::CornerRadius { nw: 0, ne: 0, sw: r, se: r }, look::content_bg());
        // the columns
        // the sidebar slides in and out with Apple's own motion (sidebar_slide): the list and the
        // toolbar's title move with it
        let t = sidebar_slide(&ctx, self.show_sidebar);
        let sw = (self.sidebar_w * t).round();
        let tb_h = if self.show_toolbar { crate::toolbar::HEIGHT } else { 0.0 };
        let side = Rect::from_min_max(body.min, pos2(body.min.x + sw, body.max.y));
        // (drawn at its full width, pushed off to the left while it slides)
        let side_full = Rect::from_min_max(pos2(side.max.x - self.sidebar_w, side.min.y), side.max);
        let right = Rect::from_min_max(pos2(side.max.x, body.min.y), body.max);
        let content = Rect::from_min_max(pos2(right.min.x, right.min.y + tb_h), right.max);
        let gallery = (self.gallery && !self.gallery_open) || self.browser.is_some();
        // (with the sidebar closed the column also holds the sidebar button at its left: it grows by
        // that much, so the folder's name keeps its room)
        let list_w = if self.gallery || self.browser.is_some() { 0.0 } else { (self.list_w + 52.0 * (1.0 - t)).clamp(LIST_MIN, (right.width() - EDITOR_MIN).max(LIST_MIN)).round() };
        let list = Rect::from_min_max(content.min, pos2(content.min.x + list_w, content.max.y));
        let editor = Rect::from_min_max(pos2(list.max.x + if list_w > 0.0 { 1.0 } else { 0.0 }, content.min.y), content.max);
        let cols = crate::toolbar::Columns {
            side: Rect::from_min_max(side.min, pos2(side.max.x, side.min.y + tb_h)),
            list: Rect::from_min_max(right.min, pos2(list.max.x, right.min.y + tb_h)),
            editor: Rect::from_min_max(pos2(editor.min.x, right.min.y), pos2(right.max.x, right.min.y + tb_h)),
            gallery,
        };
        if tb_h > 0.0 {
            crate::toolbar::strip_menu(self, ui, Rect::from_min_max(cols.side.min, cols.editor.max));
        }
        if sw > 0.0 {
            ui.scope_builder(egui::UiBuilder::new().max_rect(side_full).id_salt("sidebar"), |ui| {
                ui.set_clip_rect(side.intersect(ui.clip_rect()));
                let _t = crate::PartTimer::start("sidebar");
                crate::sidebar::show(self, ui, tb_h);
                // (a sync under way: its line at the sidebar's foot)
                crate::sync::indicator(self, ui, side);
            });
            if t < 1.0 {
                ctx.request_repaint();
            }
            // the sidebar's edge: drag to resize; under 100 pt it closes (and opens again at its
            // last width)
            let edge = Rect::from_min_max(pos2(side.max.x - 3.0, body.min.y), pos2(side.max.x + 3.0, body.max.y));
            let er = ui.interact(edge, ui.id().with("sidebar-edge"), egui::Sense::drag());
            if er.hovered() || er.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if er.dragged() {
                if let Some(px) = ctx.pointer_interact_pos() {
                    let w = px.x - body.min.x;
                    if w < SIDEBAR_COLLAPSE {
                        self.show_sidebar = false;
                    } else {
                        self.sidebar_w = w.clamp(SIDEBAR_MIN, SIDEBAR_MAX).round();
                    }
                }
            }
            if er.drag_stopped() {
                self.settings.sidebar_w = self.sidebar_w;
                self.settings.save();
            }
        }
        if self.browser.is_some() {
            ui.scope_builder(egui::UiBuilder::new().max_rect(content).id_salt("attachments"), |ui| {
                let _t = crate::PartTimer::start("attachments");
                crate::attachments::show(self, ui)
            });
        } else if gallery {
            ui.scope_builder(egui::UiBuilder::new().max_rect(content).id_salt("gallery"), |ui| {
                let _t = crate::PartTimer::start("gallery");
                crate::list::gallery(self, ui)
            });
        } else {
            if list_w > 0.0 {
                ui.scope_builder(egui::UiBuilder::new().max_rect(list).id_salt("list"), |ui| {
                    let _t = crate::PartTimer::start("list");
                    crate::list::show(self, ui)
                });
                // the list's edge: drag to resize (the editor keeps its minimum)
                let edge = Rect::from_min_max(pos2(list.max.x - 3.0, right.min.y + tb_h), pos2(list.max.x + 3.0, right.max.y));
                let er = ui.interact(edge, ui.id().with("list-edge"), egui::Sense::drag());
                if er.hovered() || er.dragged() {
                    ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                if er.dragged() {
                    // (from where the pointer is, so the edge stays under it at the limits)
                    if let Some(px) = ctx.pointer_interact_pos() {
                        let max = LIST_MAX.min(right.width() - EDITOR_MIN).max(LIST_MIN);
                        self.list_w = (px.x - list.min.x - 52.0 * (1.0 - t)).clamp(LIST_MIN, max).round();
                    }
                }
                if er.drag_stopped() {
                    self.settings.list_w = self.list_w;
                    self.settings.save();
                }
            }
            if ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|q| editor.contains(q))) {
                self.focus = Focus::Editor;
            }
            // files dropped on the sidebar or the list are imported (on the note: attached)
            let dropped = ctx.input(|i| if i.raw.dropped_files.is_empty() || i.pointer.hover_pos().is_none_or(|p| editor.contains(p)) { Vec::new() } else { i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| p.exists()).collect::<Vec<_>>() });
            if !dropped.is_empty() {
                ctx.input_mut(|i| i.raw.dropped_files.clear());
                crate::dialogs::ask_import(self, &ctx, dropped);
            }
            // (the note reaches up under the toolbar, which shows it through its band, blurred; the
            // editor keeps its content below the inset)
            let under = tb_h > 0.0 && !crate::lock::hidden(self);
            self.editor_top_inset = if under { tb_h } else { 0.0 };
            let editor = if under { Rect::from_min_max(pos2(editor.min.x, right.min.y), editor.max) } else { editor };
            ui.scope_builder(egui::UiBuilder::new().max_rect(editor).id_salt("editor"), |ui| {
                ui.set_clip_rect(editor);
                // a locked note shows its lock, not its text
                if crate::lock::hidden(self) {
                    crate::lock::view(self, ui);
                } else {
                    let _t = crate::PartTimer::start("editor");
                    // (a note chosen in the list this frame is read in full before the editor
                    // takes it)
                    self.ensure_full_selected();
                    let picked = self.selection().len();
                    if picked > 1 {
                        // several notes selected (Mac): their count where the note would be
                        let r = ui.max_rect();
                        ui.painter().rect_filled(r, self.corners_for(r), crate::look::content_bg());
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, format!("{picked} Notes Selected"), crate::look::regular(20.0), crate::look::text_dim());
                    } else {
                        crate::editor::show(self, ui);
                    }
                }
            });
        }
        if tb_h > 0.0 {
            let _t = crate::PartTimer::start("toolbar");
            crate::toolbar::show(self, ui, &cols);
            // the editor's toolbar edge (Mac, no3-scroll-edges): no line at rest; a hairline once
            // the note is scrolled at all, fading in and out on a timer
            if !gallery && self.selected_note().is_some() {
                // (NT_EDITOR_SCROLLED=1: as if the note were scrolled, for snapshots)
                let scrolled = self.editor_scroll_y > 0.5 || (self.automated && std::env::var_os("NT_EDITOR_SCROLLED").is_some());
                let t = edge_fade(&ctx, scrolled);
                if t > 0.0 {
                    let px = 1.0 / ctx.pixels_per_point();
                    let col = if kn_ui::theme::is_light() { Color32::from_gray(228) } else { look::line() };
                    p.hline(editor.x_range(), editor.min.y + px / 2.0, egui::Stroke::new(px, col.gamma_multiply(t)));
                }
            }
        }
        if title_h > 0.0 {
            p.hline(body.x_range(), body.min.y - 0.5, egui::Stroke::new(1.0, kn_ui::theme::SEPARATOR()));
        }
        // the list | editor divider runs the whole height, through the toolbar band (Mac)
        if list_w > 0.0 && !gallery {
            p.vline(list.max.x + 0.5, right.min.y..=right.max.y, egui::Stroke::new(1.0, look::line()));
        }
        crate::menus::peek_keys(self, &ctx, title);
        if crate::editor::take_add_link() {
            self.dialog = Some(Dialog::AddLink { link: String::new(), name: crate::editor::selected_text(), use_title: false });
        }
        let _dt = crate::PartTimer::start("dialogs+menus");
        {
            let _t = crate::PartTimer::start("dialogs");
            crate::dialogs::show(self, &ctx);
        }
        crate::toolbar::sheet(self, &ctx);
        crate::toolbar::drag_overlay(self, &ctx);
        // (NT_OPENING_PREVIEW="<done> <total>": the card as a conversion shows it, for snapshots)
        if self.loading() || std::env::var_os("NT_OPENING_PREVIEW").is_some() {
            opening_sheet(&ctx);
        }
        crate::windows::show(self, &ctx);
        crate::settings::show(self, &ctx);
        if self.dialog.is_none() && !self.tb.customizing {
            crate::menus::shortcuts(self, &ctx);
            crate::list::keys(self, &ctx);
        }
        if let Some((m, _)) = &self.message {
            let g = p.layout_no_wrap(m.clone(), look::regular(13.0), Color32::WHITE);
            let bg = Rect::from_center_size(pos2(window.center().x, window.max.y - 40.0), g.size() + vec2(28.0, 14.0));
            let lp = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("toast")));
            lp.rect_filled(bg, bg.height() / 2.0, Color32::from_black_alpha(190));
            lp.galley(bg.center() - g.size() / 2.0, g, Color32::WHITE);
        }
        crate::store::flush(self, crate::dates::now(), false);
        crate::store::watch(self, &ctx);
        if !self.dirty.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        self.snapshot(&ctx);
    }

    /// The rounding a pane filling `r` needs: the window's corner radius at each of its corners
    /// that lies on one of the window's (so no pane paints square past a rounded corner).
    pub fn corners_for(&self, r: Rect) -> egui::CornerRadius {
        let w = self.window_rect;
        let c = self.corner.round() as u8;
        let near = |a: f32, b: f32| (a - b).abs() < 1.0;
        let k = |x: f32, y: f32| if c > 0 && near(x, if x < w.center().x { w.min.x } else { w.max.x }) && near(y, if y < w.center().y { w.min.y } else { w.max.y }) { c } else { 0 };
        egui::CornerRadius { nw: k(r.min.x, r.min.y), ne: k(r.max.x, r.min.y), sw: k(r.min.x, r.max.y), se: k(r.max.x, r.max.y) }
    }

    /// A thumbnail's texture (decoded once).
    /// A note's thumbnail texture, made again whenever its picture changes (a drawing's Done
    /// writes a new file each time).
    pub fn thumb_texture(&mut self, ctx: &egui::Context, id: &str, t: &Thumb) -> Option<egui::TextureHandle> {
        let source = match t {
            Thumb::Image(bytes) => format!("bytes:{}:{:p}", bytes.len(), bytes.as_ptr()),
            Thumb::File(p) => format!("file:{}:{:?}", p.display(), std::fs::metadata(p).and_then(|m| m.modified()).ok()),
            _ => return None,
        };
        let frame = self.frame;
        if let Some((src, tex, used)) = self.thumb_tex.get_mut(id) {
            *used = frame;
            if *src == source {
                return Some(tex.clone());
            }
        }
        // (scripted snapshots decode at once, so a capture never catches one half-loaded;
        // otherwise it's decoded and scaled on the worker, never on the UI thread)
        if self.automated && std::env::var_os("NT_PERF").is_none() {
            let ci = decode_thumb(t)?;
            let tex = ctx.load_texture(format!("thumb-{id}"), ci, egui::TextureOptions::LINEAR);
            self.thumb_tex.insert(id.to_string(), (source, tex.clone(), frame));
            return Some(tex);
        }
        let key = format!("{id}\u{1}{source}");
        if self.thumb_jobs.pending.insert(key.clone()) {
            self.thumb_jobs.send(ctx, key, id.to_string(), source, t.clone());
        }
        // (the old picture meanwhile, if it had one)
        self.thumb_tex.get(id).map(|(_, tex, _)| tex.clone())
    }

    /// The thumbnails the worker has finished: made into textures; ones not drawn for a while
    /// let go (a big library's pictures don't all stay on the GPU).
    pub fn take_thumbs(&mut self, ctx: &egui::Context) {
        for (key, id, source, ci) in self.thumb_jobs.done() {
            self.thumb_jobs.pending.remove(&key);
            if let Some(ci) = ci {
                let tex = ctx.load_texture(format!("thumb-{id}"), ci, egui::TextureOptions::LINEAR);
                self.thumb_tex.insert(id, (source, tex, self.frame));
            }
        }
        if self.thumb_tex.len() > 200 {
            let frame = self.frame;
            self.thumb_tex.retain(|_, (_, _, used)| frame.saturating_sub(*used) < 600);
        }
    }
}

/// A note's thumbnail decoded and scaled to 256 px (off the UI thread, on the worker).
fn decode_thumb(t: &Thumb) -> Option<egui::ColorImage> {
    let img = match t {
        Thumb::Image(bytes) => image::load_from_memory(bytes).ok()?,
        // (by what the file holds: the library's files have no extension; a PDF's first page)
        Thumb::File(p) if is_pdf(p) => image::load_from_memory(&pdf_first_page(p)?).ok()?,
        Thumb::File(p) => image::ImageReader::open(p).ok()?.with_guessed_format().ok()?.decode().ok()?,
        _ => return None,
    };
    let img = img.thumbnail(256, 256).into_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}

fn is_pdf(p: &std::path::Path) -> bool {
    use std::io::Read;
    let mut b = [0u8; 5];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut b)).is_ok() && &b == b"%PDF-"
}

/// A PDF's first page as a PNG, 256 pixels on its longer side (pdftoppm).
fn pdf_first_page(p: &std::path::Path) -> Option<Vec<u8>> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("nt-thumb-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let out = dir.join(format!("t{}", SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let ok = std::process::Command::new("pdftoppm").args(["-png", "-f", "1", "-l", "1", "-singlefile", "-scale-to", "256"]).arg(p).arg(&out).status().ok()?.success();
    let png = out.with_extension("png");
    let b = ok.then(|| std::fs::read(&png).ok()).flatten();
    let _ = std::fs::remove_file(&png);
    b
}

/// The thumbnail worker: one thread decoding pictures as rows come into view.
#[derive(Default)]
pub struct ThumbJobs {
    pending: HashSet<String>,
    tx: Option<std::sync::mpsc::Sender<(String, String, String, Thumb)>>,
    rx: Option<std::sync::mpsc::Receiver<(String, String, String, Option<egui::ColorImage>)>>,
}

impl ThumbJobs {
    fn send(&mut self, ctx: &egui::Context, key: String, id: String, source: String, t: Thumb) {
        if self.tx.is_none() {
            let (jtx, jrx) = std::sync::mpsc::channel::<(String, String, String, Thumb)>();
            let (dtx, drx) = std::sync::mpsc::channel();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                for (key, id, source, t) in jrx {
                    let _ = dtx.send((key, id, source, decode_thumb(&t)));
                    ctx.request_repaint();
                }
            });
            self.tx = Some(jtx);
            self.rx = Some(drx);
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send((key, id, source, t));
        }
    }

    fn done(&mut self) -> Vec<(String, String, String, Option<egui::ColorImage>)> {
        self.rx.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default()
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.automated {
            // a scripted run takes only its own input
            raw.events.retain(|e| !matches!(e, egui::Event::Text(_) | egui::Event::Key { .. } | egui::Event::Paste(_) | egui::Event::Ime(_) | egui::Event::PointerButton { .. } | egui::Event::PointerMoved(_) | egui::Event::MouseWheel { .. } | egui::Event::Touch { .. }));
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
        // fingers (Letters' way, kn_ui::touch): one scrolls the note list or the sidebar and coasts
        // on, a tap selects, touching and holding opens the context menu
        kn_ui::touch::raw_input(ctx, raw);
    }

    /// Quitting: every change saved, the files of opened locked notes deleted.
    fn on_exit(&mut self) {
        crate::store::flush(self, f64::INFINITY, true);
        nt_core::lock::forget_unlocked();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let _timer = crate::FrameTimer::start();
        self.frame += 1;
        let ctx = ui.ctx().clone();
        if let Some(opened) = self.loading.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.loading = None;
            let _t = std::env::var_os("NT_PERF").is_some().then(|| crate::PhaseTimer::new("library in"));
            self.own_library_opened(opened);
        }
        {
            let _t = crate::PartTimer::start("full note");
            self.ensure_full_selected();
        }
        {
            let _t = crate::PartTimer::start("thumbs in");
            self.take_thumbs(&ctx);
        }
        self.take_jobs();
        crate::sync::tick(self, &ctx);
        kn_ui::begin_frame(&ctx);
        let scale = std::env::var("NT_SCALE").ok().and_then(|s| s.parse::<u32>().ok()).unwrap_or(self.settings.ui_scale);
        kn_ui::scale::apply(&ctx, scale);
        // NT_SIZE: the window's content exactly that many points (at NT_SCALE's zoom the window
        // is resized once the zoom is known)
        if self.automated {
            if let Some((w, h)) = std::env::var("NT_SIZE").ok().and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)))) {
                let have = ctx.content_rect().size();
                if (have.x - w).abs() > 0.5 || (have.y - h).abs() > 0.5 {
                    // (the desktop maximizes a window that nearly fills the screen)
                    if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(w, h) * ctx.zoom_factor()));
                }
            }
        }
        let m = if kn_ui::winframe::system_title_bar() { 0.0 } else { kn_ui::winframe::margin() };
        let inner = ui.max_rect().shrink(m);
        self.corner = if m > 0.0 { kn_ui::winframe::RADIUS } else { 0.0 };
        let dark = !kn_ui::theme::is_light();
        if m > 0.0 {
            let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
            kn_ui::winframe::paint_shadow(&ctx, inner, self.corner, focused, dark);
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            let _t = crate::PartTimer::start("frame()");
            self.frame(ui)
        });
        if m > 0.0 {
            kn_ui::winframe::paint_outline(&ctx, inner, self.corner, dark);
        }
    }
}

/// NT_SCRIPT="click 400 300; dclick 400 300; rclick 400 300; drag 100 100 300 100; type hello;
/// key Enter; key ctrl+N; menu New Folder; scroll x y dy n; wait" (points of the window's content,
/// as Letters' CS_SCRIPT). Each step becomes a few frames of events.
fn parse_script(s: &str) -> std::collections::VecDeque<Vec<egui::Event>> {
    use egui::{Event, Key, Modifiers, PointerButton, Pos2};
    let mut out = std::collections::VecDeque::new();
    let m = if kn_ui::winframe::system_title_bar() { 0.0 } else { kn_ui::winframe::margin() };
    let at = |x: f32, y: f32| Pos2::new(x + m, y + m);
    let button = |pos: Pos2, pressed: bool| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    for step in s.split(';').map(str::trim).filter(|x| !x.is_empty()) {
        let mut it = step.splitn(2, ' ');
        let cmd = it.next().unwrap_or("");
        let rest = it.next().unwrap_or("").trim();
        let nums: Vec<f32> = rest.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        match (cmd, nums.as_slice()) {
            // touch down|move|up x y: one finger (kn_ui::touch makes it a scroll, tap or hold)
            ("touch", [x, y]) => {
                let phase = match rest.split_whitespace().next() {
                    Some("down") => egui::TouchPhase::Start,
                    Some("up") => egui::TouchPhase::End,
                    _ => egui::TouchPhase::Move,
                };
                out.push_back(vec![Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(1), phase, pos: at(*x, *y), force: None }]);
            }
            ("rclick", [x, y]) => {
                let p = at(*x, *y);
                out.push_back(vec![Event::PointerMoved(p)]);
                out.push_back(vec![Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE }]);
                out.push_back(vec![Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE }]);
                out.push_back(vec![]);
            }
            ("click", [x, y]) | ("dclick", [x, y]) => {
                let p = at(*x, *y);
                out.push_back(vec![Event::PointerMoved(p)]);
                for _ in 0..if cmd == "dclick" { 2 } else { 1 } {
                    out.push_back(vec![button(p, true)]);
                    out.push_back(vec![button(p, false)]);
                }
                out.push_back(vec![]);
            }
            // shiftclick / ctrlclick x y: a click with ⇧ or ⌘ (Ctrl) held
            ("shiftclick", [x, y]) | ("ctrlclick", [x, y]) => {
                let p = at(*x, *y);
                let m = if cmd == "shiftclick" { Modifiers::SHIFT } else { Modifiers::COMMAND | Modifiers::CTRL };
                let press = |down| Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: down, modifiers: m };
                out.push_back(vec![Event::ModifiersChanged(m), Event::PointerMoved(p)]);
                out.push_back(vec![press(true)]);
                out.push_back(vec![press(false)]);
                out.push_back(vec![Event::ModifiersChanged(Modifiers::NONE)]);
            }
            ("drag", [x1, y1, x2, y2]) => {
                let (a, b) = (at(*x1, *y1), at(*x2, *y2));
                out.push_back(vec![Event::PointerMoved(a)]);
                out.push_back(vec![button(a, true)]);
                for i in 1..=8 {
                    out.push_back(vec![Event::PointerMoved(a + (b - a) * (i as f32 / 8.0))]);
                }
                out.push_back(vec![button(b, false)]);
                out.push_back(vec![]);
            }
            ("move", [x, y]) => {
                out.push_back(vec![Event::PointerMoved(at(*x, *y))]);
                out.push_back(vec![]);
            }
            // the button alone (pressed and held, or let go)
            ("down", [x, y]) | ("up", [x, y]) => {
                let p = at(*x, *y);
                out.push_back(vec![Event::PointerMoved(p)]);
                out.push_back(vec![button(p, cmd == "down")]);
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
            ("scroll", [x, y, dy, n]) => {
                out.push_back(vec![Event::PointerMoved(at(*x, *y))]);
                for _ in 0..*n as usize {
                    out.push_back(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, *dy), phase: egui::TouchPhase::Move, modifiers: Modifiers::NONE, source: egui::MouseWheelSource::Wheel }]);
                }
            }
            // swipe x y dx n: a two-finger horizontal swipe on a trackpad, n steps of dx points,
            // with the trackpad's phases (Start .. End), as Wayland's touchpads send
            ("swipe", [x, y, dx, n]) => {
                out.push_back(vec![Event::PointerMoved(at(*x, *y))]);
                let n = (*n as usize).max(2);
                for k in 0..n {
                    let phase = if k == 0 { egui::TouchPhase::Start } else if k == n - 1 { egui::TouchPhase::End } else { egui::TouchPhase::Move };
                    let d = if k == n - 1 { 0.0 } else { *dx };
                    out.push_back(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(d, 0.0), phase, modifiers: Modifiers::NONE, source: egui::MouseWheelSource::Trackpad }]);
                }
            }
            // coast x y dx n: what coasts on after a trackpad's fingers lift (momentum, horizontal)
            ("coast", [x, y, dx, n]) => {
                out.push_back(vec![Event::PointerMoved(at(*x, *y))]);
                for _ in 0..*n as usize {
                    out.push_back(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(*dx, 0.0), phase: egui::TouchPhase::Move, modifiers: Modifiers::NONE, source: egui::MouseWheelSource::Momentum }]);
                }
            }
            // coast-plain x y dx n: the same tail from a device that doesn't mark it as momentum
            ("coastplain", [x, y, dx, n]) => {
                out.push_back(vec![Event::PointerMoved(at(*x, *y))]);
                for _ in 0..*n as usize {
                    out.push_back(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(*dx, 0.0), phase: egui::TouchPhase::Move, modifiers: Modifiers::NONE, source: egui::MouseWheelSource::Wheel }]);
                }
            }
            // mouse-wheel notches (lines; `scroll` sends trackpad points)
            ("wheel", [x, y, notches]) => {
                out.push_back(vec![Event::PointerMoved(at(*x, *y))]);
                out.push_back(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, *notches), phase: egui::TouchPhase::Move, modifiers: Modifiers::NONE, source: egui::MouseWheelSource::Wheel }]);
            }
            ("menu", _) => out.push_back(vec![Event::Text(format!("\u{2}{rest}"))]),
            ("wait", _) => {
                for _ in 0..10 {
                    out.push_back(vec![]);
                }
            }
            _ => eprintln!("NT_SCRIPT: can't read step {step:?}"),
        }
    }
    out
}

/// A vertical scroll area moved by `s` (wheel easing, trackpad glide, the overlay scrollbar:
/// editor::scroll, Letters' feel); egui's ScrollArea only lays it out. Returns `add`'s result.
pub fn scroll_area<R>(s: &mut crate::editor::scroll::Scroller, ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let area = ui.max_rect();
    let ctx = ui.ctx().clone();
    let hovered = ui.rect_contains_pointer(area) && !kn_ui::pointer_over_window(&ctx);
    let moving = s.input(&ctx, hovered, area.height() * 0.9);
    // (clamped before the layout, by last frame's content height, so egui never clamps on its
    // own: one clamp, no back-and-forth at the ends)
    let hkey = egui::Id::new((id, "content-h"));
    if let Some(h) = ctx.data(|d| d.get_temp::<f32>(hkey)) {
        s.clamp(h - area.height());
    }
    let out = egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::NONE)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .vertical_scroll_offset(s.offset)
        .show(ui, add);
    s.clamp(out.content_size.y - area.height());
    // (inside the area, clear of the column's divider)
    let track = Rect::from_min_max(pos2(area.max.x - 17.0, area.min.y + 4.0), pos2(area.max.x - 5.0, area.max.y - 4.0));
    // the overlay scrollbar only while the pointer is over the area, it moved within the last
    // second, or its thumb is held (macOS hides it when idle)
    let key = egui::Id::new((id, "scrollbar-shown"));
    let now = ctx.input(|i| i.time);
    let (mut last, mut held): (f64, bool) = ctx.data(|d| d.get_temp(key)).unwrap_or((f64::NEG_INFINITY, false));
    if moving {
        last = now;
    }
    let down = ctx.input(|i| i.pointer.any_down());
    if hovered || now - last < 1.0 || (held && down) {
        held = s.scrollbar(ui, track, out.content_size.y, area.height(), egui::Id::new((id, "scrollbar"))) || (held && down);
        if now - last < 1.0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    } else {
        held = false;
    }
    ctx.data_mut(|d| d.insert_temp(key, (last, held)));
    out.inner
}

/// Scroll `s` so `row` (a rect inside `view`) is in sight.
pub fn reveal(s: &mut crate::editor::scroll::Scroller, view: Rect, row: Rect) {
    if row.min.y < view.min.y {
        s.offset += row.min.y - view.min.y;
    } else if row.max.y > view.max.y {
        s.offset += row.max.y - view.max.y;
    }
    s.stop();
}

/// The system's file dialog (never in scripted runs).
pub(crate) fn file_dialog<T>(f: impl FnOnce(kn_ui::files::FileDialog) -> Option<T>) -> Option<T> {
    if std::env::var_os("NT_SNAP").is_some() {
        return None;
    }
    f(kn_ui::files::FileDialog::new())
}

/// How far the sidebar is open, 0..1, as Notes 4.13 moves it (out-NO1 animations, the divider
/// per 60 fps frame, Notes drawing at ~30 Hz): hiding takes ~217 ms, showing ~250 ms, both along
/// the recorded curve (no fade). Reversing midway starts from where it is.
fn sidebar_slide(ctx: &egui::Context, open: bool) -> f32 {
    // (time 0..1, progress 0..1), the frames where the divider moved
    const HIDE: &[(f32, f32)] = &[(0.0, 0.0), (1.0 / 13.0, 0.03), (3.0 / 13.0, 0.199), (4.0 / 13.0, 0.419), (6.0 / 13.0, 0.644), (10.0 / 13.0, 0.851), (12.0 / 13.0, 0.998), (1.0, 1.0)];
    const SHOW: &[(f32, f32)] = &[(0.0, 0.0), (1.0 / 14.0, 0.03), (2.0 / 14.0, 0.21), (3.0 / 14.0, 0.31), (5.0 / 14.0, 0.42), (7.0 / 14.0, 0.64), (9.0 / 14.0, 0.84), (11.0 / 14.0, 0.955), (13.0 / 14.0, 0.998), (1.0, 1.0)];
    let along = |curve: &[(f32, f32)], u: f32| {
        let i = curve.iter().position(|(t, _)| *t >= u).unwrap_or(curve.len() - 1).max(1);
        let ((t0, p0), (t1, p1)) = (curve[i - 1], curve[i]);
        p0 + (p1 - p0) * ((u - t0) / (t1 - t0).max(1e-6)).clamp(0.0, 1.0)
    };
    let id = egui::Id::new("sidebar-slide");
    let now = ctx.input(|i| i.time);
    let target = if open { 1.0 } else { 0.0 };
    // (from, to, started)
    let (mut from, mut to, mut start) = ctx.data(|d| d.get_temp::<(f32, f32, f64)>(id)).unwrap_or((target, target, f64::NEG_INFINITY));
    let dur = |to: f32| if to > 0.5 { 0.250 } else { 0.217 };
    let at = |from: f32, to: f32, start: f64| {
        let u = ((now - start) / dur(to)) as f32;
        if u >= 1.0 {
            return to;
        }
        let k = along(if to > 0.5 { SHOW } else { HIDE }, u.max(0.0));
        from + (to - from) * k
    };
    if to != target {
        from = at(from, to, start);
        to = target;
        start = now;
    }
    ctx.data_mut(|d| d.insert_temp(id, (from, to, start)));
    let t = at(from, to, start);
    if t != to {
        ctx.request_repaint();
    }
    t
}

/// An account's name as shown: the device's own account (Notes' "On My Mac", "On My iPhone",
/// "On My iPad", and ours) is "On This Device" here, in Apple's pattern; the library keeps the
/// real name, so a Mac's account still reads "On My Mac" when written back for one.
pub fn account_label(a: &nt_core::model::Account) -> String {
    // (our own account, or a Mac's On My Mac; a Mac's iCloud, kept as an account of its own, by
    // its name, though it's stored here too)
    if a.id == nt_core::store::ACCOUNT_ID || a.name.starts_with("On ") || a.name.trim().is_empty() { "On This Device".into() } else { a.name.clone() }
}

/// An account's All view (Notes: "All iCloud", "All on My Mac", so here "All on This Device").
pub fn all_label(account: &str) -> String {
    match account.strip_prefix("On ") {
        Some(rest) => format!("All on {rest}"),
        None => format!("All {account}"),
    }
}

/// App's kept lookups and views (see App::cache).
#[derive(Default)]
struct Caches {
    note_at: std::cell::RefCell<HashMap<String, usize>>,
    folder_at: std::cell::RefCell<HashMap<String, usize>>,
    /// (frame, the library's fingerprint then)
    frame_fp: std::cell::RefCell<(u64, u64)>,
    /// (fingerprint, place key -> count)
    counts: std::cell::RefCell<(u64, HashMap<String, usize>)>,
    tags: std::cell::RefCell<Option<(u64, Vec<String>)>>,
    sections: std::cell::RefCell<Option<(String, Vec<(String, Vec<String>)>)>>,
    /// Smart Folders' queries as read (query JSON -> rules)
    queries: std::cell::RefCell<HashMap<String, crate::smart::Query>>,
    /// (query, fingerprint, the notes nt-core's search found)
    search: std::cell::RefCell<Option<(String, u64, HashSet<String>)>>,
}

/// Opens the app's own library and reads it (NT_PERF=1 times both).
fn load_own() -> Result<(crate::store::Store, Library), String> {
    let mut st = {
        let _t = std::env::var_os("NT_PERF").is_some().then(|| crate::PhaseTimer::new("store open"));
        crate::store::open_own().map_err(|e| e.to_string())?
    };
    {
        // Recently Deleted keeps notes 30 days, then they go for good (as on the Mac, at launch)
        let _t = std::env::var_os("NT_PERF").is_some().then(|| crate::PhaseTimer::new("purge_trash"));
        if let Err(e) = st.purge_trash(30.0) {
            eprintln!("bramble-notes: Recently Deleted's clean-up failed: {e}");
        }
    }
    let lib = {
        // (each note light: its list fields only, its body and files read when it's opened)
        let _t = std::env::var_os("NT_PERF").is_some().then(|| crate::PhaseTimer::new("store library_meta()"));
        st.library_meta().unwrap_or_default()
    };
    {
        // (the search index built here, off the UI thread, so the first search doesn't pause)
        let _t = std::env::var_os("NT_PERF").is_some().then(|| crate::PhaseTimer::new("search warm-up"));
        let _ = st.search("");
    }
    Ok((st, lib))
}

/// How far the editor toolbar's hairline has faded in, 0..1 (Mac, rec-editor-scroll-hairline:
/// in over ~140 ms once the note is scrolled, out over ~240 ms back at its top, on a timer
/// rather than with the offset; a smoothstep each way, from wherever it is).
fn edge_fade(ctx: &egui::Context, on: bool) -> f32 {
    let id = egui::Id::new("editor-edge-fade");
    let now = ctx.input(|i| i.time);
    let target = if on { 1.0 } else { 0.0 };
    let (mut from, mut to, mut start) = ctx.data(|d| d.get_temp::<(f32, f32, f64)>(id)).unwrap_or((target, target, f64::NEG_INFINITY));
    let at = |from: f32, to: f32, start: f64| {
        let dur = if to > 0.5 { 0.14 } else { 0.24 };
        let u = (((now - start) / dur) as f32).clamp(0.0, 1.0);
        from + (to - from) * u * u * (3.0 - 2.0 * u)
    };
    if to != target {
        from = at(from, to, start);
        to = target;
        start = now;
    }
    ctx.data_mut(|d| d.insert_temp(id, (from, to, start)));
    let t = at(from, to, start);
    if t != to {
        ctx.request_repaint();
    }
    t
}

/// What work done on a worker comes back with.
pub enum JobDone {
    /// a message to show (an export done or failed)
    Message(String),
    /// File > Print…: the pages made, and the printers to choose from
    Print(PathBuf, Vec<String>),
    /// Import to Notes…: the files read (loose ones in the tree's own items; folders and .enex
    /// files as its folders), and the files that couldn't be read
    Import(crate::import::Tree, Vec<String>),
    /// View > Show Attachments Browser: every attachment, with its note's id
    Attachments(Vec<(String, nt_core::model::Attachment)>),
    /// Record Audio's file, ready: attached where the caret is
    Attach(PathBuf),
    /// a worker changed the library's files (a .bnotes import): the message, then read it again
    Reload(String),
}

/// A note being dragged (from the note list onto a folder: Move).
#[derive(Clone, Debug)]
pub struct NoteDrag(pub String);

/// A folder being dragged (onto another: nested in it; onto its account: back to the top).
#[derive(Clone, Debug)]
pub struct FolderDrag(pub String);

/// While the library opens: when it's slow (the one-time conversion, another window holding the
/// library), a card in the window's middle with what's happening and how far along it is, as the
/// Mac's migration sheet. Read from nt-core's progress, which the loading worker updates.
fn opening_sheet(ctx: &egui::Context) {
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
    let preview = std::env::var("NT_OPENING_PREVIEW").ok().and_then(|v| {
        let mut it = v.split_whitespace().filter_map(|x| x.parse::<usize>().ok());
        Some(nt_core::native::Opening::Converting { done: it.next()?, total: it.next()? })
    });
    let state = preview.unwrap_or_else(nt_core::native::opening);
    let Some(msg) = state.message() else { return };
    let frac = match state {
        nt_core::native::Opening::Converting { done, total } if total > 0 => Some(done as f32 / total as f32),
        _ => None,
    };
    let inner = kn_ui::winframe::inner_rect(ctx);
    egui::Area::new(egui::Id::new("opening-sheet")).order(egui::Order::Foreground).pivot(egui::Align2::CENTER_CENTER).fixed_pos(inner.center()).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).corner_radius(12.0).inner_margin(egui::Margin::symmetric(22, 18)).show(ui, |ui| {
            ui.set_width(320.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("Updating Your Library").font(crate::look::bold(14.0)).color(crate::look::text()));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&msg).font(crate::look::regular(12.5)).color(crate::look::text_dim()));
                ui.add_space(10.0);
                // (a bar while notes are converted; a moving one for the other steps)
                let (bar, _) = ui.allocate_exact_size(egui::vec2(260.0, 5.0), egui::Sense::hover());
                let p = ui.painter();
                p.rect_filled(bar, 2.5, crate::look::text_dim().gamma_multiply(0.25));
                let fill = match frac {
                    Some(f) => egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * f.clamp(0.02, 1.0), bar.height())),
                    None => {
                        let t = (ui.input(|i| i.time) as f32 * 0.6).fract();
                        let w = bar.width() * 0.3;
                        egui::Rect::from_min_size(egui::pos2(bar.min.x + (bar.width() - w) * t, bar.min.y), egui::vec2(w, bar.height()))
                    }
                };
                p.rect_filled(fill, 2.5, crate::look::accent());
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Your notes stay as they are until it's done. Please keep Notes open.").font(crate::look::regular(11.0)).color(crate::look::text_dim()));
            });
        });
    });
}

#[cfg(test)]
mod thumb_tests {
    use super::*;

    /// List thumbnails read the library's extension-less files by what they hold: a photo, and a
    /// PDF without a preview (its first page).
    #[test]
    fn thumbnails_of_files_without_extensions() {
        let dir = std::env::temp_dir().join(format!("nt-thumb-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("5733621153a8");
        image::RgbaImage::from_pixel(40, 30, image::Rgba([200, 30, 30, 255])).save_with_format(&photo, image::ImageFormat::Png).unwrap();
        let ci = decode_thumb(&Thumb::File(photo)).expect("a photo with no extension");
        assert_eq!(ci.size, [256, 192], "the photo, 256 pixels on its longer side");
        // a one-page PDF, a black square on white (pdftoppm mends the missing cross-reference table)
        let pdf = dir.join("44e9f2765b85");
        let content = "0 0 0 rg 20 20 60 60 re f";
        std::fs::write(&pdf, format!("%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 100 200] /Contents 4 0 R >> endobj\n4 0 obj << /Length {} >> stream\n{content}\nendstream endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n", content.len())).unwrap();
        if std::process::Command::new("pdftoppm").arg("-v").output().is_err() {
            return;
        }
        let ci = decode_thumb(&Thumb::File(pdf)).expect("a PDF's first page");
        assert_eq!(ci.size, [128, 256], "256 pixels on the longer side");
        assert!(ci.pixels.iter().any(|c| c.r() < 50), "the page is drawn, not blank");
        assert!(ci.pixels.iter().any(|c| c.r() > 200));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
