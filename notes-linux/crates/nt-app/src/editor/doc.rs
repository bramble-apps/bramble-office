//! The editor's working copy of a note body: paragraphs of text with character runs, the
//! attachments the body's U+FFFC characters stand for (tables, images, inline tokens), and the
//! text operations everything else is built from. Converted from and to `nt_core::model::Body`.

use nt_core::model::{self, ParaStyle};

/// The object replacement character: one attachment or inline token in the text.
pub const OBJ: char = '\u{FFFC}';

pub use nt_core::model::Align;

/// A run's character format.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fmt {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// 1 superscript, -1 subscript
    pub script: i8,
    pub link: Option<String>,
    /// Notes' highlight colours 1..=5 (purple, pink, orange, mint, blue)
    pub highlight: Option<u8>,
    /// Format > Font > Show Colors (r, g, b, a)
    pub color: Option<[f32; 4]>,
    /// a font from imported text
    pub font: Option<model::Font>,
    /// the attachment a U+FFFC stands for
    pub att: Option<String>,
}

impl Fmt {
    /// What text typed after this run gets (attachments and links don't carry on).
    pub fn typing(&self) -> Fmt {
        Fmt { att: None, link: None, ..self.clone() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// bytes
    pub len: usize,
    pub fmt: Fmt,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Para {
    pub text: String,
    /// covers `text` exactly, no empty runs, neighbours differ
    pub runs: Vec<Run>,
    pub style: ParaStyle,
    pub indent: u8,
    pub quote: bool,
    pub checked: bool,
    pub start: Option<u32>,
    pub align: Align,
    /// Notes' paragraph uuid (note-to-section links, collapsed headings); "" until saved
    pub uuid: String,
    /// checklist item uuid
    pub check_id: String,
    /// a heading collapsed in the editor
    pub collapsed: bool,
    /// Notes' paragraph-style field 3, kept as read
    pub direction: Option<u32>,
}

impl Para {
    pub fn new(text: &str, style: ParaStyle) -> Para {
        let mut p = Para { style, ..Default::default() };
        p.insert(0, text, &Fmt::default());
        p
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn is_list(&self) -> bool {
        matches!(self.style, ParaStyle::Bulleted | ParaStyle::Dashed | ParaStyle::Numbered | ParaStyle::Checklist)
    }

    pub fn is_heading(&self) -> bool {
        matches!(self.style, ParaStyle::Title | ParaStyle::Heading | ParaStyle::Subheading)
    }

    /// The runs with their byte ranges.
    pub fn spans(&self) -> impl Iterator<Item = (std::ops::Range<usize>, &Fmt)> {
        let mut at = 0;
        self.runs.iter().map(move |r| {
            let s = at;
            at += r.len;
            (s..at, &r.fmt)
        })
    }

    /// The format of the character at `off`.
    pub fn fmt_of(&self, off: usize) -> Option<&Fmt> {
        self.spans().find(|(r, _)| r.contains(&off)).map(|(_, f)| f)
    }

    /// What typing at `off` gets: the character before's format (the first one's at the start).
    pub fn fmt_at(&self, off: usize) -> Fmt {
        let before = if off == 0 { None } else { self.fmt_of(off - 1) };
        before.or_else(|| self.fmt_of(off)).map(Fmt::typing).unwrap_or_default()
    }

    /// The attachment id at `off` (a U+FFFC).
    pub fn att_at(&self, off: usize) -> Option<&str> {
        if self.text[off..].starts_with(OBJ) { self.fmt_of(off).and_then(|f| f.att.as_deref()) } else { None }
    }

    /// The only thing in the paragraph is one attachment.
    pub fn sole_att(&self) -> Option<&str> {
        if self.text == "\u{FFFC}" { self.att_at(0) } else { None }
    }

    /// Splits the runs so that `off` is a boundary; returns the index of the run starting there.
    fn cut(&mut self, off: usize) -> usize {
        let mut at = 0;
        for i in 0..self.runs.len() {
            if at == off {
                return i;
            }
            let len = self.runs[i].len;
            if off < at + len {
                let fmt = self.runs[i].fmt.clone();
                self.runs[i].len = off - at;
                self.runs.insert(i + 1, Run { len: at + len - off, fmt });
                return i + 1;
            }
            at += len;
        }
        self.runs.len()
    }

    fn normalize(&mut self) {
        self.runs.retain(|r| r.len > 0);
        let mut i = 1;
        while i < self.runs.len() {
            if self.runs[i].fmt == self.runs[i - 1].fmt {
                self.runs[i - 1].len += self.runs[i].len;
                self.runs.remove(i);
            } else {
                i += 1;
            }
        }
    }

    pub fn insert(&mut self, off: usize, s: &str, fmt: &Fmt) {
        if s.is_empty() {
            return;
        }
        let i = self.cut(off);
        self.text.insert_str(off, s);
        self.runs.insert(i, Run { len: s.len(), fmt: fmt.clone() });
        self.normalize();
    }

    /// Inserts another paragraph's text with its runs.
    pub fn insert_para(&mut self, off: usize, other: &Para) {
        let mut i = self.cut(off);
        self.text.insert_str(off, &other.text);
        for r in &other.runs {
            self.runs.insert(i, r.clone());
            i += 1;
        }
        self.normalize();
    }

    pub fn remove(&mut self, r: std::ops::Range<usize>) {
        if r.is_empty() {
            return;
        }
        let a = self.cut(r.start);
        let b = self.cut(r.end);
        self.runs.drain(a..b);
        self.text.replace_range(r, "");
        self.normalize();
    }

    /// The part `r` as its own paragraph (same paragraph attributes).
    pub fn slice(&self, r: std::ops::Range<usize>) -> Para {
        let mut p = self.clone();
        p.remove(r.end..p.len());
        p.remove(0..r.start);
        p
    }

    /// Cuts the paragraph at `off`; the rest becomes a new paragraph with the same attributes.
    pub fn split_off(&mut self, off: usize) -> Para {
        let rest = self.slice(off..self.len());
        self.remove(off..self.len());
        Para { uuid: String::new(), check_id: String::new(), collapsed: false, start: None, ..rest }
    }

    pub fn append(&mut self, other: &Para) {
        let n = self.len();
        self.insert_para(n, other);
    }

    /// Changes the format of the characters in `r`.
    pub fn set(&mut self, r: std::ops::Range<usize>, f: &impl Fn(&mut Fmt)) {
        if r.is_empty() {
            return;
        }
        let a = self.cut(r.start);
        let b = self.cut(r.end);
        for run in &mut self.runs[a..b] {
            f(&mut run.fmt);
        }
        self.normalize();
    }

    /// Whether every character in `r` passes `f` (an empty range: the typing format).
    pub fn all(&self, r: std::ops::Range<usize>, f: &impl Fn(&Fmt) -> bool) -> bool {
        if r.is_empty() {
            return f(&self.fmt_at(r.start));
        }
        self.spans().filter(|(s, _)| s.start < r.end && r.start < s.end).all(|(_, x)| f(x))
    }
}

/// A table: rows of cells, each cell a small flow of paragraphs (Notes' cells are full texts).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub cells: Vec<Vec<Vec<Para>>>,
    /// column widths in points the user dragged (None: share the width)
    pub widths: Vec<Option<f32>>,
    pub rtl: bool,
}

impl Table {
    pub fn new(rows: usize, cols: usize) -> Table {
        Table { cells: vec![vec![vec![Para::default()]; cols]; rows], widths: vec![None; cols], rtl: false }
    }

    pub fn rows(&self) -> usize {
        self.cells.len()
    }

    pub fn cols(&self) -> usize {
        self.cells.first().map_or(0, |r| r.len())
    }

    pub fn cell_text(&self, r: usize, c: usize) -> String {
        self.cells[r][c].iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AttKind {
    Table(Table),
    /// a picture: the file (or its preview), its size in points, View As (Small: a card) and
    /// its name (Rename Attachment…)
    Image { path: Option<std::path::PathBuf>, width: f32, height: f32, view: model::ViewSize, name: String },
    /// a PDF shown inline (first page), with its preview picture when there is one
    /// a PDF: its pages' sizes (points), View As, and whether its thumbnails show (Notes 4.13)
    Pdf { path: Option<std::path::PathBuf>, preview: Option<std::path::PathBuf>, width: f32, height: f32, name: String, pages: Vec<[f32; 2]>, view: model::ViewSize, thumbs: bool },
    /// any other file: a card with its kind, size and an icon
    File { name: String, kind: String, size: u64, path: Option<std::path::PathBuf> },
    /// a drawing (PaperKit): its fallback picture
    Drawing { path: Option<std::path::PathBuf>, width: f32, height: f32, ink: Option<std::sync::Arc<super::ink::Drawing>> },
    /// a rich link card
    Url { url: String, title: String },
    /// an attachment iCloud never sent to the Mac: a "Not Downloaded" card with its kind's icon,
    /// at its size when known (its library row is kept as read)
    Missing { what: Missing, name: String, width: f32, height: f32 },
    /// inline tokens
    Tag(String),
    NoteLink { text: String, url: String },
    Divider,
    Math(String),
    Unknown(String),
}

/// What a not-downloaded attachment was, for its icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    Scan,
    Table,
    Drawing,
    File,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Att {
    pub id: String,
    pub uti: String,
    pub kind: AttKind,
    /// inline tokens: the token as read (alt text, content), written back as is
    pub token: Option<model::Token>,
    /// block attachments: the library's attachment as read (written back with the table's cells)
    pub model: Option<model::Attachment>,
}

impl Att {
    pub fn new(uti: &str, kind: AttKind) -> Att {
        Att { id: new_uuid(), uti: uti.into(), kind, token: None, model: None }
    }
}

impl Att {
    /// Drawn as its own block (a line of its own), not inside the text.
    pub fn is_block(&self) -> bool {
        !matches!(self.kind, AttKind::Tag(_) | AttKind::NoteLink { .. } | AttKind::Math(_) | AttKind::Unknown(_))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Doc {
    pub paras: Vec<Para>,
    pub atts: Vec<Att>,
}

impl Doc {
    pub fn att(&self, id: &str) -> Option<&Att> {
        self.atts.iter().find(|a| a.id == id)
    }

    pub fn att_mut(&mut self, id: &str) -> Option<&mut Att> {
        self.atts.iter_mut().find(|a| a.id == id)
    }

    pub fn table(&self, id: &str) -> Option<&Table> {
        match self.att(id).map(|a| &a.kind) {
            Some(AttKind::Table(t)) => Some(t),
            _ => None,
        }
    }

    pub fn table_mut(&mut self, id: &str) -> Option<&mut Table> {
        match self.att_mut(id).map(|a| &mut a.kind) {
            Some(AttKind::Table(t)) => Some(t),
            _ => None,
        }
    }

    /// The attachment a paragraph is (when it is one block attachment alone).
    pub fn block_of(&self, p: &Para) -> Option<&Att> {
        p.sole_att().and_then(|id| self.att(id)).filter(|a| a.is_block())
    }

    /// Plain text of part of a paragraph: tokens as their text, tables as tab-separated rows.
    pub fn para_plain(&self, p: &Para, r: std::ops::Range<usize>) -> String {
        let mut out = String::new();
        for (s, f) in p.spans() {
            let (a, b) = (s.start.max(r.start), s.end.min(r.end));
            if a >= b {
                continue;
            }
            match f.att.as_deref().and_then(|id| self.att(id)) {
                Some(att) => out.push_str(&att_plain(att)),
                None => out.push_str(&p.text[a..b]),
            }
        }
        out
    }

}

/// An attachment as text (copy as plain text, Markdown-ish like Notes' export).
pub fn att_plain(a: &Att) -> String {
    match &a.kind {
        AttKind::Tag(t) => t.clone(),
        AttKind::NoteLink { text, .. } => text.clone(),
        AttKind::Math(t) => t.clone(),
        AttKind::Divider => "---".into(),
        AttKind::Url { url, .. } => url.clone(),
        AttKind::Table(t) => (0..t.rows()).map(|r| (0..t.cols()).map(|c| t.cell_text(r, c)).collect::<Vec<_>>().join("\t")).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// A random upper-case UUID string (Notes' identifiers).
pub fn new_uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut bytes = [0u8; 16];
    for (i, chunk) in bytes.chunks_mut(8).enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_usize(i);
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

// ---- the library's note <-> Doc

fn para_from(mp: &model::Paragraph, note: Option<&model::Note>, atts: &mut Vec<Att>) -> Para {
    let mut p = Para {
        style: mp.style,
        indent: mp.indent,
        quote: mp.block_quote,
        checked: mp.checked,
        start: mp.start_number,
        align: mp.align,
        uuid: mp.id.clone(),
        check_id: mp.check_id.clone(),
        collapsed: mp.collapsed,
        direction: mp.direction,
        ..Default::default()
    };
    for s in &mp.spans {
        let att = s.token.as_ref().map(|t| {
            if !atts.iter().any(|a| a.id == t.id) {
                atts.push(att_from(t, note));
            }
            t.id.clone()
        });
        let fmt = Fmt {
            bold: s.bold,
            italic: s.italic,
            underline: s.underline,
            strike: s.strike,
            script: s.script,
            link: s.link.clone(),
            highlight: s.highlight,
            color: s.color,
            font: s.font.clone(),
            att,
        };
        let n = p.len();
        p.insert(n, &s.text, &fmt);
    }
    p
}

fn att_from(t: &model::Token, note: Option<&model::Note>) -> Att {
    use model::TokenKind;
    let kind = match t.kind() {
        TokenKind::Tag => AttKind::Tag(if t.alt.is_empty() { format!("#{}", t.content.to_lowercase()) } else { t.alt.clone() }),
        TokenKind::NoteLink { .. } => AttKind::NoteLink { text: t.alt.clone(), url: t.content.clone() },
        TokenKind::Divider => AttKind::Divider,
        TokenKind::Math => AttKind::Math(t.alt.clone()),
        TokenKind::Mention | TokenKind::OtherInline => AttKind::Unknown(t.alt.clone()),
        TokenKind::Attachment => {
            let a = note.and_then(|n| n.attachments.iter().find(|a| a.id == t.id));
            let model = a.cloned();
            let kind = match a {
                Some(a) => block_kind(a),
                // (no row for it at all: never downloaded)
                None => AttKind::Missing { what: missing_kind(model::AttachmentKind::from_uti(&t.uti)), name: String::new(), width: 0.0, height: 0.0 },
            };
            return Att { id: t.id.clone(), uti: t.uti.clone(), kind, token: Some(t.clone()), model };
        }
    };
    Att { id: t.id.clone(), uti: t.uti.clone(), kind, token: Some(t.clone()), model: None }
}

/// The kind name a file card shows ("Text Document", "PDF Document", …).
pub fn kind_name(uti: &str) -> String {
    match uti {
        "public.plain-text" | "public.utf8-plain-text" | "public.text" => "Text Document",
        "com.adobe.pdf" => "PDF Document",
        "public.rtf" => "Rich Text Document",
        "public.html" => "HTML Document",
        "net.daringfireball.markdown" | "public.markdown" => "Markdown Document",
        "public.zip-archive" => "ZIP Archive",
        "public.comma-separated-values-text" => "CSV Document",
        u if super::media::is_audio(u) => "Audio",
        u if u.contains("movie") || u.contains("video") || u == "public.mpeg-4" => "Movie",
        u if u.contains("spreadsheet") || u.contains("excel") => "Spreadsheet",
        u if u.contains("word") || u.contains("document") => "Document",
        _ => "Document",
    }
    .into()
}

/// The icon a not-downloaded attachment of this kind shows.
fn missing_kind(k: model::AttachmentKind) -> Missing {
    use model::AttachmentKind as K;
    match k {
        K::Scan => Missing::Scan,
        K::Table => Missing::Table,
        K::Drawing => Missing::Drawing,
        _ => Missing::File,
    }
}

/// iCloud never sent it to the Mac: flagged by the library, or nothing of it is here at all.
pub fn not_downloaded(a: &model::Attachment) -> bool {
    use model::AttachmentKind as K;
    a.not_downloaded
        || match a.kind {
            K::Table => a.table.is_none(),
            K::Drawing => a.drawing.is_none() && a.fallback.is_none() && a.preview.is_none(),
            K::Scan => a.display_file().is_none() && a.parts.iter().all(|p| p.display_file().is_none()),
            K::Image => a.display_file().is_none(),
            K::Pdf => a.display_file().is_none() && a.pages.is_empty(),
            K::Audio | K::File => a.file.is_none() && a.preview.is_none(),
            K::Url => false,
        }
}

pub fn block_kind(a: &model::Attachment) -> AttKind {
    use model::AttachmentKind as K;
    let (w, h) = (a.width as f32, a.height as f32);
    if not_downloaded(a) {
        return AttKind::Missing { what: missing_kind(a.kind), name: a.name.clone(), width: w, height: h };
    }
    match a.kind {
        K::Table => AttKind::Table(table_from(a.table.as_ref().cloned().unwrap_or_default())),
        K::Image => AttKind::Image { path: a.display_file().cloned(), width: w, height: h, view: a.view_size, name: a.name.clone() },
        K::Scan => AttKind::Image { path: a.parts.first().and_then(|p| p.display_file().cloned()).or(a.display_file().cloned()), width: w, height: h, view: a.view_size, name: a.name.clone() },
        K::Pdf => {
            // the pages nt-core knows, else (an older PDF attachment) the file's, read by the
            // editor off the UI thread
            let pages: Vec<[f32; 2]> = a.pages.iter().map(|p| [p.width, p.height]).collect();
            AttKind::Pdf { path: a.file.clone(), preview: a.preview.clone(), width: w, height: h, name: a.name.clone(), pages, view: a.view_size, thumbs: false }
        }
        // drawings made here come with their strokes; Apple's show their fallback picture until
        // nt-core reads PaperKit's
        K::Drawing => AttKind::Drawing { path: a.fallback.clone().or(a.preview.clone()), width: w, height: h, ink: a.drawing.clone().map(std::sync::Arc::new) },
        K::Url => AttKind::Url { url: a.url.clone().unwrap_or_default(), title: a.name.clone() },
        K::Audio | K::File => {
            let size = a.file.as_ref().and_then(|f| std::fs::metadata(f).ok()).map_or(0, |m| m.len());
            AttKind::File { name: a.name.clone(), kind: kind_name(&a.uti), size, path: a.file.clone() }
        }
    }
}

fn table_from(t: model::Table) -> Table {
    let cols = t.cols().max(1);
    let mut atts = Vec::new();
    let cells = t
        .rows
        .iter()
        .map(|r| {
            (0..cols)
                .map(|c| {
                    let b = r.get(c).cloned().unwrap_or_default();
                    let mut paras: Vec<Para> = b.paragraphs.iter().map(|p| para_from(p, None, &mut atts)).collect();
                    if paras.is_empty() {
                        paras.push(Para::default());
                    }
                    paras
                })
                .collect()
        })
        .collect();
    Table { cells, widths: vec![None; cols], rtl: t.rtl }
}

/// The working copy of a note.
pub fn from_note(note: &model::Note) -> Doc {
    let mut doc = from_body_with(&note.body, Some(note));
    if doc.paras.is_empty() {
        doc.paras.push(Para::default());
    }
    doc
}

#[allow(dead_code)]
pub fn from_body(body: &model::Body) -> Doc {
    from_body_with(body, None)
}

fn from_body_with(body: &model::Body, note: Option<&model::Note>) -> Doc {
    let mut doc = Doc::default();
    for mp in &body.paragraphs {
        let p = para_from(mp, note, &mut doc.atts);
        doc.paras.push(p);
    }
    if doc.paras.is_empty() {
        doc.paras.push(Para::default());
    }
    doc
}

fn para_to(doc: &Doc, p: &Para) -> model::Paragraph {
    model::Paragraph {
        style: p.style,
        indent: p.indent,
        align: p.align,
        block_quote: p.quote,
        checked: p.checked,
        check_id: p.check_id.clone(),
        start_number: p.start,
        id: p.uuid.clone(),
        collapsed: p.collapsed,
        direction: p.direction,
        spans: p
            .spans()
            .flat_map(|(r, f)| {
                let mk = |text: &str, token: Option<model::Token>| model::Span {
                    text: text.to_string(),
                    bold: f.bold,
                    italic: f.italic,
                    underline: f.underline,
                    strike: f.strike,
                    script: f.script,
                    link: f.link.clone(),
                    highlight: f.highlight,
                    color: f.color,
                    font: f.font.clone(),
                    token,
                };
                match f.att.as_deref().and_then(|id| doc.att(id)) {
                    // one span per token
                    Some(a) => p.text[r].chars().map(|_| mk("\u{FFFC}", Some(token_of(a)))).collect::<Vec<_>>(),
                    None => vec![mk(&p.text[r], None)],
                }
            })
            .collect(),
    }
}

/// The token a span carries for an attachment (the one read, or one for a new attachment).
fn token_of(a: &Att) -> model::Token {
    let mut t = a.token.clone().unwrap_or(model::Token { id: a.id.clone(), uti: a.uti.clone(), ..Default::default() });
    match &a.kind {
        AttKind::Tag(s) => {
            t.alt = s.clone();
            t.content = s.trim_start_matches('#').to_uppercase();
        }
        AttKind::NoteLink { text, url } => {
            t.alt = text.clone();
            t.content = url.clone();
        }
        AttKind::Divider => {
            t.alt = "---".into();
            t.content = a.uti.clone();
        }
        AttKind::Math(s) => {
            t.alt = s.clone();
            if t.content.is_empty() {
                t.content = "ICInlineAttachmentCalculateStateValidLTR".into();
            }
        }
        _ => {}
    }
    t
}

pub fn to_body(doc: &Doc) -> model::Body {
    model::Body { paragraphs: doc.paras.iter().map(|p| para_to(doc, p)).collect() }
}

fn table_to(t: &Table) -> model::Table {
    let cell = |paras: &Vec<Para>| {
        let d = Doc { paras: paras.clone(), atts: Vec::new() };
        let b = to_body(&d);
        if b.paragraphs.len() == 1 && b.paragraphs[0].spans.is_empty() { model::Body::default() } else { b }
    };
    model::Table { rows: t.cells.iter().map(|r| r.iter().map(cell).collect()).collect(), rtl: t.rtl }
}

/// Writes the working copy into the note: its body, its tables' cells, and attachments the editor
/// added (tables, attached files).
pub fn write_note(doc: &Doc, note: &mut model::Note) {
    write_note_drawing(doc, note, None);
}

/// Strokes the user is drawing whose fallback picture hasn't been drawn yet.
static STALE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
/// Each drawing as last written, held (so a change to it copies it, and a drawing still the same
/// one is unchanged: its strokes aren't compared again).
static WRITTEN: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<super::ink::Drawing>>>> = std::sync::LazyLock::new(Default::default);

/// As write_note; the drawing `open` (being drawn on) keeps its strokes but its fallback picture
/// is only drawn once it is closed, not on every stroke.
pub fn write_note_drawing(doc: &Doc, note: &mut model::Note, open: Option<&str>) {
    note.body = to_body(doc);
    // attachments the note no longer shows anywhere (its text or its tables' cells) go with it
    fn add<'a>(paras: &'a [Para], shown: &mut Vec<&'a str>) {
        for p in paras {
            for r in &p.runs {
                if let Some(id) = r.fmt.att.as_deref() {
                    shown.push(id);
                }
            }
        }
    }
    let mut shown: Vec<&str> = Vec::new();
    add(&doc.paras, &mut shown);
    for a in &doc.atts {
        if let AttKind::Table(t) = &a.kind {
            for row in &t.cells {
                for cell in row {
                    add(cell, &mut shown);
                }
            }
        }
    }
    let shown: Vec<String> = shown.into_iter().map(str::to_string).collect();
    note.attachments.retain(|a| shown.contains(&a.id));
    let used: Vec<&str> = doc.paras.iter().flat_map(|p| p.runs.iter().filter_map(|r| r.fmt.att.as_deref())).collect();
    for a in &doc.atts {
        if !used.contains(&a.id.as_str()) || a.uti.starts_with("com.apple.notes.inlinetextattachment") {
            continue;
        }
        // (one with no row at all is left so: there's nothing to write)
        if matches!(a.kind, AttKind::Missing { .. }) && a.model.is_none() {
            continue;
        }
        // (from what the note has now: what was last written, else as read)
        let mut m = note.attachments.iter().find(|x| x.id == a.id).cloned().or_else(|| a.model.clone()).unwrap_or_else(|| model::Attachment {
            id: a.id.clone(),
            uti: a.uti.clone(),
            kind: model::AttachmentKind::from_uti(&a.uti),
            ..Default::default()
        });
        match &a.kind {
            AttKind::Table(t) => {
                let mt = table_to(t);
                m.summary = mt.summary();
                m.table = Some(mt);
            }
            // a drawing drawn here: its fallback picture (what Apple and the list thumbnail show)
            AttKind::Drawing { ink: Some(d), .. } => {
                m.width = d.width as f64;
                m.height = d.height as f64;
                // (the fallback picture is drawn again only when the strokes changed)
                let mut stale = STALE.lock().unwrap_or_else(|e| e.into_inner());
                // (also when its picture is gone: a drawing brought back by Undo after the library
                // deleted its files)
                let gone = m.fallback.as_ref().is_none_or(|p| !p.exists());
                // (the same drawing as last written, by identity: no need to compare its strokes)
                let mut written = WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
                let same_as_written = written.get(&a.id).is_some_and(|w| std::sync::Arc::ptr_eq(w, d)) && m.drawing.is_some();
                if !same_as_written {
                    written.insert(a.id.clone(), d.clone());
                }
                drop(written);
                if !same_as_written && m.drawing.as_ref() != Some(&**d) || gone || stale.contains(&a.id) {
                    m.drawing = Some((**d).clone());
                    stale.retain(|x| *x != a.id);
                    if open == Some(a.id.as_str()) {
                        stale.push(a.id.clone());
                    } else if super::draw::PICTURES_AT_ONCE.load(std::sync::atomic::Ordering::Relaxed) {
                        if let Some(p) = super::draw::fallback_png(&a.id, d, m.fallback.as_deref()) {
                            m.fallback = Some(p);
                        }
                    } else {
                        // (drawn on a worker; the old picture stays until the new one is in)
                        super::draw::fallback_later(super::shapes::CTX.with_borrow(|c| c.clone()), &a.id, d);
                    }
                }
            }
            AttKind::Image { path, width, height, view, name } if a.model.is_none() => {
                m.file = path.clone();
                m.width = *width as f64;
                m.height = *height as f64;
                m.view_size = *view;
                m.name = if name.is_empty() { path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { name.clone() };
            }
            // (View As and a rename are the editor's; the rest is the library's)
            AttKind::Image { view, name, .. } => {
                m.view_size = *view;
                if !name.is_empty() {
                    m.name = name.clone();
                }
            }
            AttKind::Drawing { path, width, height, .. } if a.model.is_none() => {
                m.file = path.clone();
                m.width = *width as f64;
                m.height = *height as f64;
                m.name = path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            }
            AttKind::Pdf { path, width, height, name, view, .. } => {
                // (View As and a rename are the editor's; the rest is the library's once saved)
                m.view_size = *view;
                m.name = name.clone();
                if a.model.is_none() {
                    m.file = path.clone();
                    m.width = *width as f64;
                    m.height = *height as f64;
                }
            }
            AttKind::File { name, path, .. } if a.model.is_none() => {
                m.file = path.clone();
                m.name = name.clone();
            }
            // (a rename)
            AttKind::File { name, .. } => m.name = name.clone(),
            _ => {}
        }
        match note.attachments.iter_mut().find(|x| x.id == a.id) {
            Some(x) => {
                if *x != m {
                    *x = m;
                }
            }
            None => note.attachments.push(m),
        }
    }
}
