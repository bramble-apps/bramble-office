//! The note editor: paragraph styles, lists, checklists, tables, attachments, highlights, links,
//! tags, note links, dividers, math results, collapsible headings, the Aa format popover, typing
//! auto-format, selection, undo, clipboard and spelling, looking like Notes 4.13's editor.
//!
//! The shell calls [`show`] with the editor column as `ui.max_rect()`; the editor keeps its state
//! (caret, undo, the working copy of the open note) itself, runs the menu commands queued in
//! `app.editor_cmds` (Notes' menu labels, see [`Editor::command_label`]), writes edits back to the
//! note (then calls `app.note_changed()`) and reports `app.editor_state` for the menus. The Aa
//! toolbar button's popup draws [`format_popover`].

mod clip;
pub mod doc;
mod draw;
mod find;
pub mod ink;
mod input;
pub mod layout;
pub mod markdown;
mod markup;
mod math;
mod media;
mod menu;
pub mod ops;
mod paint;
mod pdf;
mod player;
pub mod scroll;
pub mod shapes;
pub mod popover;
mod detect;
mod fontpanel;
mod spelling;
mod table;
#[cfg(test)]
mod snapshot;
#[cfg(test)]
mod tests;

use std::cell::RefCell;

use egui::Rect;
use nt_core::model::{self, ParaStyle};

use crate::app::App;
pub use doc::Align;
use doc::Doc;
use ops::{Kind, Snap};

/// A table cell: the table's paragraph in the note and the cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Cell {
    pub para: usize,
    pub row: usize,
    pub col: usize,
}

/// A position in the text: in the note (cell None) or in a table cell; byte offset in a paragraph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Loc {
    pub cell: Option<Cell>,
    pub para: usize,
    pub off: usize,
}

/// Selected table cells (row and column handles, or a drag across cells).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableSel {
    pub para: usize,
    pub r0: usize,
    pub c0: usize,
    pub r1: usize,
    pub c1: usize,
}

/// Editor settings (Notes > Settings and Edit > Substitutions).
#[derive(Clone, Debug)]
pub struct Settings {
    /// Settings > "Automatically sort checked items"
    pub sort_checked: bool,
    /// Settings > Default text size and View > Zoom (1 = the default)
    pub text_scale: f32,
    pub smart_quotes: bool,
    pub smart_dashes: bool,
    pub smart_lists: bool,
    pub smart_links: bool,
    pub smart_tags: bool,
    pub text_replacement: bool,
    pub spelling: bool,
    /// Edit > Spelling and Grammar > Correct Spelling Automatically
    pub autocorrect: bool,
    /// Format > Math Results: 0 off, 1 suggest, 2 insert
    pub math: u8,
    /// Edit > Substitutions > Data Detectors
    pub data_detectors: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            sort_checked: true,
            text_scale: 1.0,
            smart_quotes: true,
            smart_dashes: true,
            smart_lists: true,
            smart_links: true,
            smart_tags: true,
            text_replacement: true,
            spelling: true,
            autocorrect: false,
            math: 1,
            data_detectors: true,
        }
    }
}

/// What a menu or toolbar button asks of the editor.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Style(ParaStyle),
    Bold,
    Italic,
    Underline,
    Strike,
    /// None: the last colour, or off when the selection is highlighted
    Highlight(Option<u8>),
    Superscript,
    Subscript,
    BaselineDefault,
    BlockQuote,
    Checklist,
    MarkChecked,
    CheckAll,
    UncheckAll,
    MoveCheckedToBottom,
    DeleteChecked,
    MoveItemUp,
    MoveItemDown,
    Indent,
    Outdent,
    Align(Align),
    InsertTable,
    InsertDivider,
    AddLink(String),
    RemoveLink,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PasteMatchStyle,
    SelectAll,
    Delete,
    CollapseSection,
    ExpandSection,
    CollapseAll,
    ExpandAll,
    ConvertTableToText,
    AddRowAbove,
    AddRowBelow,
    DeleteRow,
    AddColumnBefore,
    AddColumnAfter,
    DeleteColumn,
    /// a picture or file from disk (Edit > Attach File…)
    AttachFile(std::path::PathBuf),
    /// a link to another note (Add Link with a note chosen): its title and applenotes:// URL
    InsertNoteLink { title: String, url: String },
    CopyStyle,
    PasteStyle,
    RemoveStyle,
}

/// What the editor tells the shell from one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Response {
    pub changed: bool,
    pub open_link: Option<String>,
    /// a note link (applenotes://showNote?identifier=…&paragraphID=…) was clicked
    pub open_note: Option<String>,
    /// a tag was clicked ("#name")
    pub tag_clicked: Option<String>,
    /// ⌘K / the context menu's Add Link…: the shell's Add Link sheet should open
    pub add_link: bool,
    /// the editor was clicked (it has the keyboard now)
    pub focus: bool,
}

/// A note the >> autocomplete can link to.
#[derive(Clone, Debug, Default)]
pub struct NoteRef {
    pub id: String,
    pub title: String,
}

#[derive(Default)]
pub struct Editor {
    pub doc: Doc,
    note: Option<String>,
    pub anchor: Loc,
    pub head: Loc,
    pub table_sel: Option<TableSel>,
    /// format for the next typed text (Bold with nothing selected…)
    pub pending: Option<doc::Fmt>,
    pub goal_x: Option<f32>,
    pub undo: Vec<Snap>,
    pub redo: Vec<Snap>,
    pub last_kind: Option<Kind>,
    pub break_undo: bool,
    pub changed: bool,
    pub last_highlight: u8,
    pub settings: Settings,
    /// notes and tags for the >> and # autocomplete
    pub notes: Vec<NoteRef>,
    pub tags: Vec<String>,
    pub focused: bool,
    /// the date line above the text ("November 3, 2026 at 2:37 PM")
    pub date_line: String,
    /// Copy Style / Paste Style
    style_clip: Option<(doc::Fmt, ParaStyle)>,
    view: input::View,
    media: media::Media,
    menu: menu::MenuState,
    pub popover: popover::PopoverState,
    /// checklist items the last edit moved: (flow, item uuid, the paragraph it was) — the window
    /// slides them from there
    pub moved: Vec<(Option<Cell>, String, usize)>,
    /// how what moved slides (delay, seconds), when not the tick's
    pub slide_timing: Option<(f64, f64)>,
    /// an alert for the app to show (title, text): a click on a not-downloaded attachment
    pub alert: Option<(String, String)>,
    /// the toolbar strip over the editor's top (the note scrolls up under it, translucent):
    /// the content starts this far down, clicks there are the toolbar's
    pub top_inset: f32,
    /// Settings > "Use dark backgrounds for note content" off: Notes' light paper in dark mode
    pub light_paper: bool,
    /// the last write of this note (any editor's) this editor has taken in
    pub seen_write: u64,
    request_repaint: bool,
    pub find: find::FindState,
    /// a drawing (or a picture's Markup) being drawn on
    pub draw: Option<draw::DrawState>,
    /// the Markup palette sliding out after the drawing closed: its tools then, when it began
    pub markup_exit: Option<(draw::DrawState, f64)>,
    /// the time of this frame (egui's clock), for what starts outside a frame (Done's slide)
    pub now: f64,
    /// a sheet, Settings or Customize Toolbar is over the window: no keys or clicks for the note
    pub blocked: bool,
    /// the window's rounded corners where the editor column touches them (the shell's corners_for)
    pub corners: egui::CornerRadius,
    resp: Response,
}

thread_local! {
    static EDITOR: RefCell<Editor> = RefCell::new(Editor::new());
    /// Notes open in windows of their own (Window > Open Note in New Window), by window key
    static WINDOWS: RefCell<std::collections::HashMap<egui::Id, Editor>> = RefCell::new(std::collections::HashMap::new());
    /// Each note's last write by an editor: (a count, which editor), so another editor showing
    /// the same note takes it in again
    static WRITES: RefCell<std::collections::HashMap<String, (u64, egui::Id)>> = RefCell::new(std::collections::HashMap::new());
    static ADD_LINK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// ⌘K or Add Link… was chosen in the editor: the shell opens its Add Link sheet (once per ask),
/// then queues "Add Link:<url>" or "Note Link:<title>|<url>".
pub fn take_add_link() -> bool {
    ADD_LINK.replace(false)
}

/// Whether the editors still have pictures or PDF pages on the workers (scripted snapshots wait
/// for them, so a capture never shows a page still white).
pub fn busy() -> bool {
    EDITOR.with(|e| e.try_borrow().is_ok_and(|ed| ed.media.busy())) || WINDOWS.with(|w| w.try_borrow().is_ok_and(|w| w.values().any(|ed| ed.media.busy())))
}

/// The selected text (the Add Link sheet's Name field).
pub fn selected_text() -> String {
    EDITOR.with_borrow(|ed| ed.copy_frag().map(|(_, t)| t).unwrap_or_default())
}

impl Editor {
    pub fn new() -> Editor {
        layout::preload_faces();
        Editor { last_highlight: 3, ..Default::default() }
    }

    /// Opens a note (the caret at the end of its first line, undo cleared).
    pub fn load(&mut self, note: &model::Note) {
        self.load_doc(&note.id, doc::from_note(note));
        let end = self.doc.paras[0].len();
        self.anchor = Loc { cell: None, para: 0, off: end };
        self.head = self.anchor;
    }

    /// Opens a working copy (tests).
    pub fn load_doc(&mut self, note_id: &str, doc: Doc) {
        self.note = Some(note_id.to_string());
        self.doc = doc;
        if self.doc.paras.is_empty() {
            self.doc.paras.push(doc::Para::default());
        }
        self.anchor = Loc::default();
        self.head = Loc::default();
        self.undo.clear();
        self.redo.clear();
        self.table_sel = None;
        self.pending = None;
        self.draw = None;
        self.changed = false;
        self.view = input::View::default();
    }

    /// A menu label (Notes' wording) as a command; parameters after a colon:
    /// "Add Link:<url>", "Attach File:<path>", "Note Link:<title>|<url>", "Highlight:<1-5>".
    pub fn command_label(&mut self, ctx: Option<&egui::Context>, label: &str) {
        let label = label.trim_end_matches('…').trim_end_matches("...");
        let (name, arg) = label.split_once(':').map(|(a, b)| (a, Some(b))).unwrap_or((label, None));
        // the menus qualify submenu items ("Font > Bold", "Move Item > Up")
        let (menu, name) = name.rsplit_once(" > ").map(|(m, n)| (m, n)).unwrap_or(("", name));
        use Cmd::*;
        match (menu, name) {
            ("Math Results", n) => {
                self.settings.math = match n {
                    "Insert Results" => 2,
                    "Suggest Results" => 1,
                    _ => 0,
                };
                return;
            }
            ("Substitutions", n) => {
                let s = &mut self.settings;
                match n {
                    "Smart Quotes" => s.smart_quotes = !s.smart_quotes,
                    "Smart Dashes" => s.smart_dashes = !s.smart_dashes,
                    "Smart Links" => s.smart_links = !s.smart_links,
                    "Text Replacement" => s.text_replacement = !s.text_replacement,
                    "Data Detectors" => s.data_detectors = !s.data_detectors,
                    _ => {}
                }
                return;
            }
            ("Spelling and Grammar", "Check Spelling While Typing") => {
                self.settings.spelling = !self.settings.spelling;
                return;
            }
            ("Transformations", n) => {
                self.transform_case(match n {
                    "Make Upper Case" => 0,
                    "Make Lower Case" => 1,
                    _ => 2,
                });
                return;
            }
            ("Find", n) => {
                self.find_command(n);
                return;
            }
            ("Indentation", "Increase") => return self.command(ctx, Indent),
            ("Indentation", "Decrease") => return self.command(ctx, Outdent),
            _ => {}
        }
        if name == "Toggle Tag" {
            if let Some((_, tag)) = arg.and_then(|a| a.split_once('|')) {
                self.toggle_tag(tag);
            }
            return;
        }
        if menu == "Font" && name == "Show Fonts" {
            self.view.fonts_panel = !self.view.fonts_panel;
            return;
        }
        if menu == "Font" && name == "Show Colors" {
            self.view.colors_panel = !self.view.colors_panel;
            return;
        }
        if menu == "Spelling and Grammar" {
            match name {
                "Show Spelling and Grammar" => {
                    self.view.spell_panel = true;
                    self.check_document_now();
                }
                "Check Document Now" => {
                    self.check_document_now();
                }
                // (the ticks are settings the app flips; the editor reads them each frame)
                _ => {}
            }
            return;
        }
        if menu == "Writing Direction" {
            // Notes' paragraph field 3, zigzag: 1 natural (-1), 0 left to right, 2 right to left.
            // (Selection: egui has no bidi runs, so the paragraphs it touches)
            let d = if name.ends_with("Right to Left") {
                2
            } else if name.ends_with("Left to Right") {
                0
            } else {
                1
            };
            self.set_direction(d);
            return;
        }
        if menu == "Attachment View" {
            let v = if name == "Set All to Small" { model::ViewSize::Small } else { model::ViewSize::Large };
            self.set_all_views(v);
            return;
        }
        if menu == "View As" {
            let v = match name {
                "Small" => model::ViewSize::Small,
                "Medium" => model::ViewSize::Medium,
                _ => model::ViewSize::Large,
            };
            if let Some(id) = self.selected_block() {
                self.view_as(&id, v);
            }
            return;
        }
        if name == "Rename Attachment" {
            if let Some(id) = self.selected_block() {
                self.start_rename(&id);
            }
            return;
        }
        if name == "Paste as Markdown" {
            // (the clipboard arrives as a paste event next frame)
            if let Some(c) = ctx {
                self.view.paste_md = true;
                c.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
            #[cfg(test)]
            {
                self.view.paste_md = true;
            }
            return;
        }
        if name == "Copy as Markdown" {
            if let Some((frag, _)) = self.copy_frag() {
                let md = markdown::frag_to_markdown(&self.doc, &frag);
                if let Some(c) = ctx {
                    c.copy_text(md);
                }
            }
            return;
        }
        let cmd = match name {
            "Title" => Style(ParaStyle::Title),
            "Heading" => Style(ParaStyle::Heading),
            "Subheading" => Style(ParaStyle::Subheading),
            "Body" => Style(ParaStyle::Body),
            "Monostyled" => Style(ParaStyle::Monostyled),
            "Bulleted List" => Style(ParaStyle::Bulleted),
            "Dashed List" => Style(ParaStyle::Dashed),
            "Numbered List" => Style(ParaStyle::Numbered),
            "Block Quote" => BlockQuote,
            "Checklist" => Checklist,
            "Mark as Checked" | "Mark as Unchecked" => MarkChecked,
            "Check All" => CheckAll,
            "Uncheck All" => UncheckAll,
            "Move Checked to Bottom" => MoveCheckedToBottom,
            "Delete Checked" => DeleteChecked,
            "Up" | "Move Item Up" => MoveItemUp,
            "Down" | "Move Item Down" => MoveItemDown,
            "Increase" | "Increase Indentation" => Indent,
            "Decrease" | "Decrease Indentation" => Outdent,
            "Align Left" => Align(doc::Align::Left),
            "Center" => Align(doc::Align::Center),
            "Justify" => Align(doc::Align::Justified),
            "Align Right" => Align(doc::Align::Right),
            "Table" | "Add Table" => {
                if self.head.cell.is_some() || self.table_sel.is_some() {
                    ConvertTableToText
                } else {
                    InsertTable
                }
            }
            "Convert to Text" => ConvertTableToText,
            "Insert Divider Line" => InsertDivider,
            "Insert Drawing" | "Add Sketch" => {
                self.insert_drawing();
                self.changed = true;
                return;
            }
            "Markup" => {
                self.markup();
                self.changed = true;
                return;
            }
            "Bold" => Bold,
            "Italic" => Italic,
            "Underline" => Underline,
            "Strikethrough" => Strike,
            "Highlight" => Highlight(arg.and_then(|a| a.parse().ok())),
            "Purple" => Highlight(Some(1)),
            "Pink" => Highlight(Some(2)),
            "Orange" => Highlight(Some(3)),
            "Mint" => Highlight(Some(4)),
            "Blue" => Highlight(Some(5)),
            "Accent" => Highlight(Some(3)),
            "Superscript" => Superscript,
            "Subscript" => Subscript,
            "Default" | "Use Default" => BaselineDefault,
            "Undo" | "Undo Typing" => Undo,
            "Redo" => Redo,
            "Cut" => Cut,
            "Copy" => Copy,
            "Paste" | "Paste and Retain Style" => Paste,
            "Paste and Match Style" => PasteMatchStyle,
            "Select All" => SelectAll,
            "Delete" => Delete,
            "Collapse Section" => CollapseSection,
            "Expand Section" => ExpandSection,
            "Collapse All Sections" => CollapseAll,
            "Expand All Sections" => ExpandAll,
            "Bigger" => {
                self.settings.text_scale = (self.settings.text_scale * 1.1).min(3.0);
                return;
            }
            "Smaller" => {
                self.settings.text_scale = (self.settings.text_scale / 1.1).max(0.6);
                return;
            }
            "Add Row Above" => AddRowAbove,
            "Add Row Below" => AddRowBelow,
            "Delete Row" => DeleteRow,
            "Add Column Before" => AddColumnBefore,
            "Add Column After" => AddColumnAfter,
            "Delete Column" => DeleteColumn,
            "Copy Style" => CopyStyle,
            "Paste Style" => PasteStyle,
            "Remove Style" => RemoveStyle,
            "Remove Link" => RemoveLink,
            "Add Link" | "Edit Link" => match arg {
                Some(u) => AddLink(u.to_string()),
                None => {
                    self.resp.add_link = true;
                    return;
                }
            },
            "Attach File" => match arg {
                Some(p) => AttachFile(p.into()),
                None => return,
            },
            "Note Link" => match arg.and_then(|a| a.split_once('|')) {
                Some((t, u)) => InsertNoteLink { title: t.into(), url: u.into() },
                None => return,
            },
            _ => {
                #[cfg(test)]
                UNHANDLED.with_borrow_mut(|u| u.push(label.to_string()));
                return;
            }
        };
        self.command(ctx, cmd);
    }

    /// Runs a command.
    pub fn command(&mut self, ctx: Option<&egui::Context>, cmd: Cmd) {
        use Cmd::*;
        match cmd {
            Style(s) => self.set_style(s),
            Bold => self.toggle_bold(),
            Italic => self.toggle_italic(),
            Underline => self.toggle_underline(),
            Strike => self.toggle_strike(),
            Highlight(c) => self.highlight(c),
            Superscript => self.set_script(1),
            Subscript => self.set_script(-1),
            BaselineDefault => self.set_script(0),
            BlockQuote => self.toggle_quote(),
            Checklist => self.set_style(ParaStyle::Checklist),
            MarkChecked => self.mark_checked(),
            CheckAll => self.check_all(true),
            UncheckAll => self.check_all(false),
            MoveCheckedToBottom => self.move_checked_to_bottom(),
            DeleteChecked => self.delete_checked(),
            MoveItemUp => self.move_item(false),
            MoveItemDown => self.move_item(true),
            Indent => self.indent(1),
            Outdent => self.indent(-1),
            Align(a) => self.align(a),
            InsertTable => self.insert_table(),
            InsertDivider => self.insert_divider(),
            AddLink(u) => self.set_link(Some(u)),
            RemoveLink => self.set_link(None),
            Undo => self.undo(),
            Redo => self.redo(),
            Cut | Copy => {
                if let Some((frag, plain)) = self.copy_frag() {
                    input::set_clipboard(ctx, frag, plain);
                    if cmd == Cut {
                        if self.table_sel.is_some() {
                            self.clear_table_sel();
                        } else {
                            self.push_undo(Kind::Other);
                            self.delete_sel();
                        }
                    }
                }
            }
            Paste | PasteMatchStyle => {
                if let Some(ctx) = ctx {
                    self.view.paste_match = cmd == PasteMatchStyle;
                    // the clipboard read here (files and pictures with no text too); when it can't
                    // be read, egui's paste event next frame
                    let text = kn_ui::clipboard::text().filter(|t| !t.is_empty());
                    if !self.paste_system(text.as_deref(), None) {
                        self.view.paste_match = cmd == PasteMatchStyle;
                        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                    }
                }
            }
            SelectAll => self.select_all(),
            Delete => {
                if self.has_sel() || self.table_sel.is_some() {
                    self.backspace(false)
                }
            }
            CollapseSection => self.collapse_section(true),
            ExpandSection => self.collapse_section(false),
            CollapseAll => self.collapse_all(true),
            ExpandAll => self.collapse_all(false),
            ConvertTableToText => self.table_to_text(),
            AddRowAbove => self.table_add_row(false),
            AddRowBelow => self.table_add_row(true),
            DeleteRow => self.table_delete_rows(),
            AddColumnBefore => self.table_add_col(false),
            AddColumnAfter => self.table_add_col(true),
            DeleteColumn => self.table_delete_cols(),
            AttachFile(p) => self.attach_file(&p),
            InsertNoteLink { title, url } => {
                self.push_undo(Kind::Other);
                self.delete_sel();
                let off = self.head.off;
                self.insert_token(off..off, doc::AttKind::NoteLink { text: title, url }, model::UTI_NOTE_LINK);
            }
            CopyStyle => {
                let p = self.para(self.head).clone();
                self.style_clip = Some((self.typing_fmt(), p.style));
            }
            PasteStyle => {
                if let Some((f, s)) = self.style_clip.clone() {
                    self.set_style(s);
                    self.set_chars(|x| *x = doc::Fmt { att: x.att.clone(), link: x.link.clone(), ..f.clone() });
                }
            }
            RemoveStyle => self.set_chars(|x| *x = doc::Fmt { att: x.att.clone(), ..Default::default() }),
        }
        self.view.scroll_to_caret = true;
    }

    /// The menus' checkmarks and enabled states.
    pub fn state(&self) -> crate::app::EditorState {
        let p = self.para(self.head);
        let r = self.sel_paras();
        let flow = self.flow(self.head.cell);
        let paras = &flow[r];
        let style = paras.iter().all(|q| q.style == p.style).then_some(p.style);
        let link = p.fmt_of(self.head.off.saturating_sub(1)).and_then(|f| f.link.clone()).or_else(|| p.fmt_of(self.head.off).and_then(|f| f.link.clone()));
        crate::app::EditorState {
            focused: self.focused,
            has_selection: self.has_sel() || self.table_sel.is_some(),
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
            undo_label: (!self.undo.is_empty() && self.last_kind == Some(Kind::Typing)).then(|| "Typing".to_string()),
            style,
            // Title, Heading and Subheading are bold by style (the Aa popover's B is on)
            bold: self.chars_all(|f| f.bold) || paras.iter().all(|q| matches!(q.style, ParaStyle::Title | ParaStyle::Heading | ParaStyle::Subheading)),
            italic: self.chars_all(|f| f.italic),
            underline: self.chars_all(|f| f.underline),
            strike: self.chars_all(|f| f.strike),
            in_table: self.head.cell.is_some() || self.table_sel.is_some(),
            in_checklist: paras.iter().any(|q| q.style == ParaStyle::Checklist),
            on_link: link.is_some(),
            on_attachment: self.head.cell.is_none() && self.doc.block_of(p).is_some(),
            data_detectors: self.settings.data_detectors,
            direction: if self.head.cell.is_some() { None } else { paras.first().and_then(|f| paras.iter().all(|q| q.direction.unwrap_or(1) == f.direction.unwrap_or(1)).then(|| f.direction.unwrap_or(1))) },
        }
    }
}

/// What the main window's editor drew last frame inside `rect` (screen points), roughly: each
/// line of text's box in its colour, attachments' boxes in a middling colour, the date line.
/// For the toolbar's translucent band, which tints with what passes under it.
pub fn under(rect: Rect) -> Vec<(Rect, egui::Color32)> {
    EDITOR.with_borrow(|ed| {
        let mut out = Vec::new();
        let Some(flow) = ed.view.flow.as_ref() else { return out };
        if let Some(d) = ed.view.date_rect.filter(|d| d.intersects(rect)) {
            out.push((d, layout::dim_color()));
        }
        let o = ed.view.origin;
        for b in &flow.boxes {
            let top = o.y + b.top;
            if b.hidden || top > rect.max.y || top + b.height < rect.min.y {
                continue;
            }
            if let Some(g) = &b.galley {
                let base = egui::pos2(o.x + b.galley_x, top + b.gy);
                let color = g.job.sections.first().map_or(layout::text_color(), |s| s.format.color);
                for row in &g.rows {
                    let r = row.rect().translate(base.to_vec2());
                    if r.width() > 0.5 && r.intersects(rect) {
                        out.push((r, color));
                    }
                }
            } else if let Some(block) = &b.block {
                let size = match block {
                    layout::Block::Media(s) => *s,
                    _ => egui::vec2(b.width, b.height),
                };
                let r = Rect::from_min_size(egui::pos2(o.x + b.x, top), size);
                if r.intersects(rect) {
                    out.push((r, if layout::dark() { egui::Color32::from_gray(90) } else { egui::Color32::from_gray(170) }));
                }
            }
        }
        out
    })
}

/// The main window's editor's key.
fn main_key() -> egui::Id {
    egui::Id::unique("nt-main-editor")
}

/// Draws the selected note in the editor column (`ui.max_rect()`) and handles its input.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    EDITOR.with_borrow_mut(|ed| show_with(app, ui, ed, main_key(), None));
}

/// Draws note `note_id` in a window of its own (`ui.max_rect()`), with the editor kept for
/// `key` (the window's): its caret, selection and undo stay its own. The menus' commands and
/// state stay the main window's.
pub fn show_in(app: &mut App, ui: &mut egui::Ui, note_id: &str, key: egui::Id) {
    let mut ed = WINDOWS.with_borrow_mut(|w| w.remove(&key)).unwrap_or_else(Editor::new);
    show_with(app, ui, &mut ed, key, Some(note_id.to_string()));
    WINDOWS.with_borrow_mut(|w| w.insert(key, ed));
}

/// A window's editor is no longer needed (its window closed).
pub fn close_window(key: egui::Id) {
    WINDOWS.with_borrow_mut(|w| {
        if let Some(mut ed) = w.remove(&key) {
            // (a drawing still open: its picture is drawn and kept)
            ed.finish_drawing();
        }
    });
}

fn show_with(app: &mut App, ui: &mut egui::Ui, ed: &mut Editor, key: egui::Id, own: Option<String>) {
    let rect = ui.max_rect();
    let main = own.is_none();
    {
        ed.corners = app.corners_for(rect);
        ed.blocked = app.dialog.is_some() || app.show_settings || app.tb.customizing;
        let cmds = if main { std::mem::take(&mut app.editor_cmds) } else { Vec::new() };
        ed.focused = if main { app.focus == crate::app::Focus::Editor } else { ui.ctx().input(|i| i.viewport().focused.unwrap_or(true)) };
        ed.top_inset = if main { app.editor_top_inset } else { 0.0 };
        ed.light_paper = !app.settings.dark_backgrounds;
        // the default text size stop draws Body at 16 pt (measured), Notes' 13 pt stop
        ed.settings.text_scale = app.settings.body_size() / 13.0 * app.zoom.max(0.25);
        ed.settings.sort_checked = app.settings.auto_sort_checked;
        ed.settings.spelling = app.settings.spell_check;
        ed.settings.autocorrect = app.settings.autocorrect;
        ed.settings.smart_quotes = app.settings.smart_quotes;
        ed.settings.smart_dashes = app.settings.smart_dashes;
        ed.settings.text_replacement = app.settings.text_replacement;
        ed.settings.smart_links = app.settings.smart_links;
        ed.settings.smart_tags = app.settings.smart_tags;
        ed.settings.smart_lists = app.settings.smart_lists;
        // (the editor's own right-click menu flips these too: written back below)
        let toggles = |s: &Settings| [s.smart_quotes, s.smart_dashes, s.smart_links, s.smart_lists, s.text_replacement, s.spelling];
        let before = toggles(&ed.settings);
        if ed.notes.len() != app.library.notes.len() || ui.ctx().cumulative_frame_nr() % 120 == 0 {
            ed.notes = app.library.notes.iter().filter(|n| !n.deleted).map(|n| NoteRef { id: n.id.clone(), title: n.title.clone() }).collect();
            ed.tags = app.all_tags();
        }
        let want = own.clone().or_else(|| app.note.clone());
        // a drawing left open when another note is chosen: closed, its picture drawn in its note
        if ed.draw.is_some() && ed.note.is_some() && ed.note != want {
            ed.finish_drawing();
            ed.changed = false;
            let old = ed.note.clone().unwrap_or_default();
            // (never into a light copy: its body and attachments aren't there to write over)
            if let Some(n) = app.library.notes.iter_mut().find(|n| n.id == old && !n.light) {
                doc::write_note(&ed.doc, n);
                app.dirty.insert(old, crate::dates::now());
            }
        }
        // the library lists light notes (no body or attachments) until one is opened
        if main {
            app.ensure_full_selected();
        } else if let Some(id) = &own {
            if app.library.notes.iter().any(|n| n.id == *id && n.light) {
                if let Some(full) = app.db.as_ref().and_then(|st| st.note(id).ok()) {
                    if let Some(n) = app.library.notes.iter_mut().find(|n| n.id == *id) {
                        *n = full;
                    }
                }
            }
        }
        let found = match &own {
            Some(id) => app.library.notes.iter_mut().find(|n| n.id == *id),
            None => app.selected_note_mut(),
        };
        let Some(note) = found.filter(|n| !n.light) else {
            layout::set_paper(ed.light_paper);
            ui.painter().rect_filled(rect, ed.corners, layout::bg_color());
            ed.note = None;
            if main {
                app.editor_state = Default::default();
                app.editor_scroll_y = 0.0;
            }
            return;
        };
        // (another editor wrote this note since: taken in again, scrolled where it was)
        let theirs = WRITES.with_borrow(|w| w.get(&note.id).copied()).filter(|(n, by)| *by != key && *n > ed.seen_write);
        // (another note: what was playing stops)
        if main && ed.note.is_some() && ed.note.as_deref() != Some(note.id.as_str()) {
            player::stop();
        }
        if ed.note.as_deref() != Some(note.id.as_str()) || theirs.is_some() {
            let (scroll, head) = (ed.view.scroll.offset, ed.head);
            let same = ed.note.as_deref() == Some(note.id.as_str());
            ed.load(note);
            if same {
                ed.view.scroll.offset = scroll;
                if head.cell.is_none() && head.para < ed.doc.paras.len() && head.off <= ed.doc.paras[head.para].len() && ed.doc.paras[head.para].text.is_char_boundary(head.off) {
                    ed.head = head;
                    ed.anchor = head;
                }
            }
            ed.seen_write = WRITES.with_borrow(|w| w.get(&note.id).map_or(0, |x| x.0));
        }
        ed.date_line = crate::dates::long(note.modified);
        let nid = note.id.clone();
        let resp = frame(ed, ui, rect, note, &cmds);
        if resp.changed {
            let n = WRITES.with_borrow_mut(|w| {
                let next = w.values().map(|x| x.0).max().unwrap_or(0) + 1;
                w.insert(nid.clone(), (next, key));
                next
            });
            ed.seen_write = n;
            if main {
                app.note_changed();
            } else {
                note_changed_by_id(app, &nid);
            }
        }
        if main {
            // (the toolbar's hairline shows once the note is scrolled)
            app.editor_scroll_y = ed.view.scroll.offset;
        }
        // drawings' pictures drawn on the workers: into their attachments (this note's or one
        // left since), saved with them
        for pic in draw::take_pictures() {
            let note = app.library.notes.iter_mut().find(|n| !n.light && n.attachments.iter().any(|a| a.id == pic.id));
            let took = note.and_then(|n| {
                let a = n.attachments.iter_mut().find(|a| a.id == pic.id)?;
                draw::apply_picture(&pic, a).then(|| n.id.clone())
            });
            match took {
                Some(id) => {
                    app.dirty.insert(id, crate::dates::now());
                }
                None => draw::drop_picture(pic),
            }
        }
        if let Some((title, text)) = ed.alert.take() {
            app.dialog = Some(crate::app::Dialog::Alert(title, text));
        }
        if toggles(&ed.settings) != before {
            let s = &ed.settings;
            app.settings.smart_quotes = s.smart_quotes;
            app.settings.smart_dashes = s.smart_dashes;
            app.settings.smart_links = s.smart_links;
            app.settings.smart_lists = s.smart_lists;
            app.settings.text_replacement = s.text_replacement;
            app.settings.spell_check = s.spelling;
            app.settings.save();
        }
        if resp.add_link {
            ADD_LINK.set(true);
        }
        if let Some(u) = resp.open_link {
            kn_ui::open_url(&u);
        }
        if let Some(u) = resp.open_note {
            let tk = model::Token { uti: model::UTI_NOTE_LINK.into(), content: u, ..Default::default() };
            if let model::TokenKind::NoteLink { note, .. } = tk.kind() {
                if app.note_by_id(&note).is_some() {
                    app.select_note(&note);
                }
            }
        }
        if let Some(t) = resp.tag_clicked {
            app.go(crate::app::Place::Tag(t.trim_start_matches('#').to_string()));
        }
        if main {
            app.editor_state = ed.state();
            if resp.focus {
                app.focus = crate::app::Focus::Editor;
            }
        }
    }
}

/// A note edited in its own window: its edit date, title and snippet follow (as the app's
/// note_changed does for the selected one).
fn note_changed_by_id(app: &mut App, id: &str) {
    let now = crate::dates::now();
    app.dirty.insert(id.to_string(), now);
    if let Some(n) = app.library.notes.iter_mut().find(|n| n.id == id) {
        n.modified = now;
        let lines: Vec<String> = n.body.paragraphs.iter().map(|p| p.text().replace(model::OBJ, "")).filter(|t| !t.trim().is_empty()).collect();
        n.title = lines.first().map(|t| t.trim().to_string()).unwrap_or_default();
        n.snippet = lines.get(1).map(|t| t.trim().to_string()).unwrap_or_default();
    }
}

/// One frame of the editor on a note: commands, input, drawing, and writing back.
fn frame(ed: &mut Editor, ui: &mut egui::Ui, rect: Rect, note: &mut model::Note, cmds: &[String]) -> Response {
    ed.resp = Response::default();
    for c in cmds {
        ed.command_label(Some(ui.ctx()), c);
    }
    input::frame(ed, ui, rect);
    let mut r = std::mem::take(&mut ed.resp);
    if ed.changed {
        ed.refresh_math();
        ed.changed = false;
        doc::write_note_drawing(&ed.doc, note, ed.draw.as_ref().map(|d| d.att.as_str()));
        r.changed = true;
    }
    r
}

#[cfg(test)]
thread_local! {
    /// Menu labels command_label didn't know (tests: every menu item the shell sends works).
    pub static UNHANDLED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Opens a file in the system's app for it (Open Attachment); never from tests or snapshots,
/// whose clicks land anywhere.
pub(crate) fn open_external(path: &std::path::Path) {
    if cfg!(test) || std::env::var_os("NT_EDITOR_SNAP").is_some() || std::env::var_os("NT_SNAP").is_some() {
        return;
    }
    let _ = kn_ui::open_command(path).spawn();
}

/// The Aa toolbar button's popover contents (the shell opens the popup and calls this inside it).
pub fn format_popover(app: &mut App, ui: &mut egui::Ui) {
    EDITOR.with_borrow_mut(|ed| {
        if ed.note.is_none() {
            return;
        }
        popover::show(ed, ui);
        if ed.changed {
            ed.changed = false;
            if let Some(note) = app.selected_note_mut() {
                if ed.note.as_deref() == Some(note.id.as_str()) && !note.light {
                    doc::write_note(&ed.doc, note);
                    app.note_changed();
                }
            }
        }
        app.editor_state = ed.state();
    });
}
