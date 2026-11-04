//! What the app works on, independent of how Apple stores it (nt-core's readers and writers map
//! NoteStore rows and note protobufs to and from these).

use std::path::PathBuf;

/// The object replacement character that stands for an attachment or inline token in a note's text.
pub const OBJ: char = '\u{FFFC}';

/// A paragraph's style, as Notes' Format menu names them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParaStyle {
    Title,
    Heading,
    Subheading,
    #[default]
    Body,
    Monostyled,
    Bulleted,
    Dashed,
    Numbered,
    Checklist,
}

impl ParaStyle {
    pub fn is_list(self) -> bool {
        matches!(self, ParaStyle::Bulleted | ParaStyle::Dashed | ParaStyle::Numbered | ParaStyle::Checklist)
    }
}

/// Paragraph alignment (Format > Text). `Natural` = no alignment stored (follows the writing direction).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Natural,
    Left,
    Center,
    Right,
    Justified,
}

/// A font set on imported (HTML / AppleScript) text; Notes' own styles never set one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Font {
    pub name: Option<String>,
    pub size: Option<f32>,
    pub hints: Option<u32>,
}

/// What an inline token or attachment placeholder (one U+FFFC in the text) is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Token {
    /// the attachment's / inline attachment's identifier (upper-case UUID)
    pub id: String,
    /// its type UTI, e.g. `public.png`, `com.apple.notes.table`, `com.apple.notes.inlinetextattachment.hashtag`
    pub uti: String,
    /// inline tokens: the text Notes shows (`#tag`, `Note → Section`, `---`, ` = 14`); filled from the
    /// library's ICInlineAttachment row, empty when only the body was decoded
    pub alt: String,
    /// inline tokens: ZTOKENCONTENTIDENTIFIER (tag key, `applenotes://showNote?…` link, math state)
    pub content: String,
}

/// The kinds of [`Token`], by UTI.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Tag,
    /// a link to another note (and optionally one of its paragraphs, by paragraph uuid)
    NoteLink { note: String, paragraph: Option<String> },
    Divider,
    Math,
    Mention,
    /// any other inline token
    OtherInline,
    /// a block attachment (image, PDF, file, table, drawing, URL card, …): see [`Attachment`]
    Attachment,
}

pub const UTI_HASHTAG: &str = "com.apple.notes.inlinetextattachment.hashtag";
pub const UTI_NOTE_LINK: &str = "com.apple.notes.inlinetextattachment.link";
pub const UTI_DIVIDER: &str = "com.apple.notes.inlinetextattachment.dividerline";
pub const UTI_MATH: &str = "com.apple.notes.inlinetextattachment.calculateresult";
pub const UTI_MENTION: &str = "com.apple.notes.inlinetextattachment.mention";
pub const UTI_TABLE: &str = "com.apple.notes.table";
pub const UTI_PAPER: &str = "com.apple.paper";
/// a PDF attached in Notes 4.13+: a PaperKit document of its pages ([`crate::paperkit`])
pub const UTI_PAPER_PDF: &str = "com.apple.paper.doc.pdf";

impl Token {
    pub fn is_inline(&self) -> bool {
        self.uti.starts_with("com.apple.notes.inlinetextattachment")
    }

    pub fn kind(&self) -> TokenKind {
        match self.uti.as_str() {
            UTI_HASHTAG => TokenKind::Tag,
            UTI_DIVIDER => TokenKind::Divider,
            UTI_MATH => TokenKind::Math,
            UTI_MENTION => TokenKind::Mention,
            UTI_NOTE_LINK => {
                let q = self.content.split_once('?').map(|(_, q)| q).unwrap_or("");
                let get = |k: &str| {
                    q.split('&').find_map(|kv| kv.strip_prefix(k).and_then(|v| v.strip_prefix('='))).map(str::to_string)
                };
                TokenKind::NoteLink { note: get("identifier").unwrap_or_default(), paragraph: get("paragraphID") }
            }
            _ if self.is_inline() => TokenKind::OtherInline,
            _ => TokenKind::Attachment,
        }
    }

    /// A note-link token's content URL.
    pub fn note_link_url(note: &str, paragraph: Option<&str>) -> String {
        match paragraph {
            Some(p) => format!("applenotes://showNote?identifier={note}&paragraphID={p}"),
            None => format!("applenotes://showNote?identifier={note}"),
        }
    }
}

/// One run of text with the same character style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    /// the text; a token span holds exactly one U+FFFC
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// 1 superscript, -1 subscript
    pub script: i8,
    pub link: Option<String>,
    /// Notes' highlight colours 1..=5 (purple, pink, orange, mint, blue)
    pub highlight: Option<u8>,
    /// Format > Font > Show Colors, as r, g, b, a in 0..=1
    pub color: Option<[f32; 4]>,
    pub font: Option<Font>,
    /// an attachment or inline token (then `text` is "\u{FFFC}")
    pub token: Option<Token>,
}

impl Span {
    pub fn plain(text: impl Into<String>) -> Span {
        Span { text: text.into(), ..Span::default() }
    }

    /// A span holding one attachment / inline token.
    pub fn token(id: impl Into<String>, uti: impl Into<String>) -> Span {
        Span {
            text: OBJ.to_string(),
            token: Some(Token { id: id.into(), uti: uti.into(), ..Token::default() }),
            ..Span::default()
        }
    }

    /// Same character style (everything but the text); token spans never merge.
    pub fn same_style(&self, o: &Span) -> bool {
        self.bold == o.bold
            && self.italic == o.italic
            && self.underline == o.underline
            && self.strike == o.strike
            && self.script == o.script
            && self.link == o.link
            && self.highlight == o.highlight
            && self.color == o.color
            && self.font == o.font
            && self.token.is_none()
            && o.token.is_none()
    }
}

/// One paragraph of a note.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paragraph {
    pub style: ParaStyle,
    /// list nesting / Format > Indentation level
    pub indent: u8,
    pub align: Align,
    pub block_quote: bool,
    /// checklist items: done or not
    pub checked: bool,
    /// checklist items: the item's uuid (upper-case, dashed); the encoder makes one when empty
    pub check_id: String,
    /// a numbered list's start number as Notes stores it (Notes writes 1 when the list was typed "1. ")
    pub start_number: Option<u32>,
    /// the paragraph's uuid (upper-case, dashed): target of note-to-section links and collapse state;
    /// `body::assign_ids` makes one for new paragraphs
    pub id: String,
    /// headings: collapsed in the editor (stored outside the body, in ICNote.outlineStateData)
    pub collapsed: bool,
    /// Notes' paragraph-style field 3 (1 on most paragraphs, absent on some); kept as read
    pub direction: Option<u32>,
    pub spans: Vec<Span>,
}

impl Paragraph {
    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    pub fn plain(style: ParaStyle, text: &str) -> Paragraph {
        let spans = if text.is_empty() { Vec::new() } else { vec![Span::plain(text)] };
        Paragraph { style, spans, ..Paragraph::default() }
    }
}

/// A note's body.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Body {
    pub paragraphs: Vec<Paragraph>,
}

impl Body {
    /// The note's text with U+FFFC for each attachment/token, paragraphs joined by '\n'.
    pub fn text(&self) -> String {
        self.paragraphs.iter().map(Paragraph::text).collect::<Vec<_>>().join("\n")
    }

    /// Plain lines, one per paragraph (handy for building bodies in code and tests).
    pub fn from_lines(title: &str, lines: &[&str]) -> Body {
        let mut paragraphs = vec![Paragraph::plain(ParaStyle::Title, title)];
        paragraphs.extend(lines.iter().map(|l| Paragraph::plain(ParaStyle::Body, l)));
        Body { paragraphs }
    }

    /// Every token in reading order.
    pub fn tokens(&self) -> impl Iterator<Item = &Token> {
        self.paragraphs.iter().flat_map(|p| p.spans.iter()).filter_map(|s| s.token.as_ref())
    }

    pub fn tokens_mut(&mut self) -> impl Iterator<Item = &mut Token> {
        self.paragraphs.iter_mut().flat_map(|p| p.spans.iter_mut()).filter_map(|s| s.token.as_mut())
    }
}

/// A table attachment's grid (rows of cells); cells are full rich text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub rows: Vec<Vec<Body>>,
    /// CRTableColumnDirectionRightToLeft
    pub rtl: bool,
}

impl Table {
    pub fn cols(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    pub fn from_text(rows: &[&[&str]]) -> Table {
        let cell = |s: &&str| {
            if s.is_empty() {
                Body::default()
            } else {
                Body { paragraphs: s.split('\n').map(|l| Paragraph::plain(ParaStyle::Body, l)).collect() }
            }
        };
        Table { rows: rows.iter().map(|r| r.iter().map(cell).collect()).collect(), rtl: false }
    }

    pub fn text_grid(&self) -> Vec<Vec<String>> {
        self.rows.iter().map(|r| r.iter().map(Body::text).collect()).collect()
    }

    /// Notes' ICAttachment.summary for a table: every cell's text, row by row, joined by '\n'.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        for r in &self.rows {
            for c in r {
                s.push_str(&c.text());
                s.push('\n');
            }
        }
        s
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    Pdf,
    /// a drawing (com.apple.paper / legacy com.apple.drawing): shown via its fallback image/PDF
    Drawing,
    Table,
    /// a URL card (public.url)
    Url,
    Audio,
    /// a document scan (com.apple.notes.gallery)
    Scan,
    #[default]
    File,
}

impl AttachmentKind {
    pub fn from_uti(uti: &str) -> AttachmentKind {
        match uti {
            UTI_TABLE => AttachmentKind::Table,
            UTI_PAPER | "com.apple.drawing" | "com.apple.drawing.2" => AttachmentKind::Drawing,
            // PaperKit documents: attached PDFs, scans and other documents, all shown as their pages
            "com.adobe.pdf" | UTI_PAPER_PDF | "com.apple.paper.doc" | "com.apple.paper.doc.scan" => AttachmentKind::Pdf,
            "public.url" => AttachmentKind::Url,
            "com.apple.notes.gallery" => AttachmentKind::Scan,
            "public.png" | "public.jpeg" | "public.heic" | "public.heif" | "public.avif" | "public.tiff" | "public.image"
            | "com.compuserve.gif" | "org.webmproject.webp" | "com.microsoft.bmp" | "public.svg-image" => AttachmentKind::Image,
            u if u.contains("audio") || u == "public.mpeg-4-audio" => AttachmentKind::Audio,
            _ => AttachmentKind::File,
        }
    }
}

/// One block attachment of a note (the body's U+FFFC with the same id points at it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attachment {
    pub id: String,
    pub uti: String,
    pub kind: AttachmentKind,
    /// the original file name (ICMedia.filename), or the URL card's title
    pub name: String,
    /// the original file (Media/<id>/<generation>/<name>), when it exists on disk
    pub file: Option<PathBuf>,
    /// drawings and scans: FallbackImages/…/FallbackImage.png or FallbackPDFs/…/FallbackPDF.pdf
    pub fallback: Option<PathBuf>,
    /// the largest light-mode preview image found on disk (Previews/<id>/<gen>/Preview.png)
    pub preview: Option<PathBuf>,
    pub width: f64,
    pub height: f64,
    /// URL cards: the link
    pub url: Option<String>,
    /// Notes' indexable text (table cells joined by '\n', OCR, …)
    pub summary: String,
    pub table: Option<Table>,
    /// drawings: the strokes, when they could be read (else only `fallback`, the picture, is known)
    pub drawing: Option<Drawing>,
    /// scans (com.apple.notes.gallery): the scanned pages, each an image attachment
    pub parts: Vec<Attachment>,
    /// seconds since 1970
    pub created: f64,
    pub modified: f64,
    /// View As (ZMERGEABLEPREFERREDVIEWSIZE)
    pub view_size: ViewSize,
    /// iCloud never sent it to the Mac (ZNEEDSINITIALFETCHFROMCLOUD, no file or data), or a drawing
    /// whose bundle never arrived: there is nothing to show
    pub not_downloaded: bool,
    /// PDFs kept as PaperKit documents: their pages in order (empty for other attachments; a PDF
    /// added here without pages gets them from the file)
    pub pages: Vec<PdfPage>,
}

/// An encrypted row's crypto columns (ZCRYPTO*, ZENCRYPTEDVALUESJSON), copied as they are.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Crypto {
    pub salt: Option<Vec<u8>>,
    pub iterations: i64,
    pub wrapped_key: Option<Vec<u8>>,
    pub verifier: Option<Vec<u8>>,
    pub iv: Option<Vec<u8>>,
    pub tag: Option<Vec<u8>>,
    pub values: Option<Vec<u8>>,
}

/// A locked note kept encrypted ([`Note::sealed`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sealed {
    pub crypto: Crypto,
    pub hint: Option<String>,
    /// the note body (ZICNOTEDATA.ZDATA), its IV and tag
    pub data: Vec<u8>,
    pub data_iv: Option<Vec<u8>>,
    pub data_tag: Option<Vec<u8>>,
    /// parents before their scan pages
    pub attachments: Vec<SealedAttachment>,
    pub inline: Vec<SealedInline>,
    /// encrypted files: (where they are, path under the account folder)
    pub files: Vec<(PathBuf, PathBuf)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SealedAttachment {
    pub id: String,
    pub uti: String,
    /// the gallery a scan page belongs to
    pub parent: Option<String>,
    pub width: f64,
    pub height: f64,
    pub created: Option<f64>,
    pub modified: Option<f64>,
    pub fallback_image: Option<String>,
    pub fallback_pdf: Option<String>,
    pub fallback_image_iv: Option<Vec<u8>>,
    pub fallback_image_tag: Option<Vec<u8>>,
    pub fallback_pdf_iv: Option<Vec<u8>>,
    pub fallback_pdf_tag: Option<Vec<u8>>,
    pub crypto: Crypto,
    pub media: Option<SealedMedia>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SealedMedia {
    pub id: String,
    pub generation: Option<String>,
    pub asset_iv: Option<Vec<u8>>,
    pub asset_tag: Option<Vec<u8>>,
    pub crypto: Crypto,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SealedInline {
    pub id: String,
    pub uti: String,
    pub crypto: Crypto,
}

/// How big an attachment is shown in the note (View As).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewSize {
    #[default]
    Large,
    Small,
    Medium,
}

/// One page of a PDF document.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PdfPage {
    /// the page's index in the PDF file (0-based)
    pub index: u32,
    /// its size in points
    pub width: f32,
    pub height: f32,
    /// the crop box: x y w h
    pub crop: [f32; 4],
}

impl Attachment {
    /// The file to show or export: the original, else the drawing's fallback, else the preview.
    pub fn display_file(&self) -> Option<&PathBuf> {
        self.file.as_ref().or(self.fallback.as_ref()).or(self.preview.as_ref())
    }
}

/// PencilKit's inks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum InkKind {
    #[default]
    Pen,
    Pencil,
    Marker,
    Monoline,
    Fountain,
    Watercolor,
    Crayon,
    Reed,
}

/// One sample of a stroke (PencilKit's PKStrokePoint).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InkPoint {
    pub x: f32,
    pub y: f32,
    /// seconds from the stroke's start
    pub t: f32,
    /// the width here (points)
    pub size: f32,
    pub force: f32,
    pub azimuth: f32,
    pub altitude: f32,
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub id: String,
    pub ink: InkKind,
    /// r, g, b, a in 0..=1
    pub color: [f32; 4],
    /// the ink's nominal width
    pub width: f32,
    pub points: Vec<InkPoint>,
    /// a b c d tx ty
    pub transform: [f32; 6],
    /// the pixel eraser's mask path over this stroke, if any (stroke coordinates)
    pub mask: Option<Vec<[f32; 2]>>,
}

/// What a Markup object is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    #[default]
    Rect,
    RoundRect,
    Oval,
    Line,
    Arrow,
    Bubble,
    Star,
    Polygon,
    Loupe,
    Text,
    Signature,
}

/// A Markup object over a drawing or picture (PaperKit's shapes, text boxes, signatures).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub id: String,
    pub kind: ShapeKind,
    /// x y w h, drawing points, before rotation (about the centre)
    pub frame: [f32; 4],
    /// radians
    pub rotation: f32,
    /// rgba, a = 0: no border
    pub stroke: [f32; 4],
    /// rgba, a = 0: no fill
    pub fill: [f32; 4],
    pub line_width: f32,
    /// 0 solid, 1 dashed, 2 dotted
    pub dash: u8,
    pub shadow: bool,
    /// Line / Arrow: the two ends; Bubble: the tail tip (drawing points)
    pub points: Vec<[f32; 2]>,
    /// Star points / Polygon sides
    pub sides: u32,
    /// bit 0 start, bit 1 end
    pub arrows: u8,
    pub text: String,
    pub font: String,
    pub font_size: f32,
    pub text_color: [f32; 4],
    /// 0 left, 1 centre, 2 right, 3 justified
    pub align: u8,
    /// Loupe
    pub magnify: f32,
    /// Signature: its strokes, in a box of 0..1 x 0..1 (scaled to the frame)
    pub ink: Vec<Stroke>,
}

/// A drawing (com.apple.paper): its canvas size and strokes, bottom first, then Markup shapes over
/// them, bottom first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    pub width: f32,
    pub height: f32,
    pub strokes: Vec<Stroke>,
    pub shapes: Vec<Shape>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Note {
    pub id: String,
    pub folder: String,
    pub title: String,
    pub snippet: String,
    /// seconds since 1970
    pub created: f64,
    pub modified: f64,
    pub pinned: bool,
    /// password protected. Its body and attachments are only read when the note was opened with its
    /// password (then `lock` is set); otherwise they are left empty.
    pub locked: bool,
    /// how a locked note opened with its password is encrypted, so saving keeps it locked
    pub lock: Option<Lock>,
    /// in Recently Deleted
    pub deleted: bool,
    /// when it went to Recently Deleted (seconds since 1970; ZFOLDERMODIFICATIONDATE)
    pub trashed_at: Option<f64>,
    /// an iCloud note whose body the Mac hadn't downloaded yet (ZNEEDSINITIALFETCHFROMCLOUD, no
    /// data): only its title and snippet are known
    pub not_downloaded: bool,
    /// a locked note no password opened when its library was read: its encrypted rows and files as
    /// Notes stored them, so importing keeps it locked and its own password opens it later
    pub sealed: Option<Box<Sealed>>,
    /// read for the notes list only ([`crate::store::Store::library_meta`]): no body or
    /// attachments; `list_thumbnail` and `list_tags` stand in for them
    pub light: bool,
    pub list_thumbnail: Option<PathBuf>,
    pub list_tags: Vec<String>,
    /// (light notes) its attachments' kinds, and whether it has a checklist and one with unchecked
    /// items (Notes' ZHASCHECKLIST / ZHASCHECKLISTINPROGRESS), for Smart Folders
    pub list_kinds: Vec<AttachmentKind>,
    pub list_checklist: (bool, bool),
    pub body: Body,
    pub attachments: Vec<Attachment>,
}

impl Note {
    pub fn attachment(&self, id: &str) -> Option<&Attachment> {
        self.attachments.iter().find(|a| a.id == id)
    }

    /// The picture for the note list: the first image / scan / drawing / PDF in the body (its preview
    /// when there is one, else its file). PDFs give a PDF path (`.pdf`) when they have no preview.
    pub fn thumbnail(&self) -> Option<&std::path::Path> {
        if self.light {
            return self.list_thumbnail.as_deref();
        }
        // the first picture, scan (its first page), drawing or PDF in the body
        let first = self.body.tokens().filter_map(|t| self.attachment(&t.id)).find(|a| {
            matches!(a.kind, AttachmentKind::Image | AttachmentKind::Scan | AttachmentKind::Drawing | AttachmentKind::Pdf)
        })?;
        match first.kind {
            AttachmentKind::Scan => first.parts.first().and_then(|p| p.display_file()).or(first.preview.as_ref()).map(|p| p.as_path()),
            _ => first.preview.as_deref().or(first.fallback.as_deref()).or(first.file.as_deref()),
        }
    }

    /// The kinds of its attachments (for Smart Folders; light notes too).
    pub fn attachment_kinds(&self) -> Vec<AttachmentKind> {
        if self.light {
            return self.list_kinds.clone();
        }
        self.attachments.iter().map(|a| a.kind).collect()
    }

    /// Its checklist items: (how many, how many checked). A light note says only whether it has
    /// one and whether one is unchecked: (1, 0) undone, (1, 1) all done, (0, 0) none.
    pub fn checklist_counts(&self) -> (u32, u32) {
        if self.light {
            return match self.list_checklist {
                (false, _) => (0, 0),
                (true, true) => (1, 0),
                (true, false) => (1, 1),
            };
        }
        self.body.paragraphs.iter().filter(|p| p.style == ParaStyle::Checklist).fold((0, 0), |(all, done), p| (all + 1, done + u32::from(p.checked)))
    }

    /// Tags in the note (the `#tag` tokens' display text without '#').
    pub fn tags(&self) -> Vec<String> {
        if self.light {
            return self.list_tags.clone();
        }
        self.body.tokens().filter(|t| t.uti == UTI_HASHTAG).map(|t| t.alt.trim_start_matches('#').to_string()).collect()
    }
}

/// What kind of folder a folder is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FolderKind {
    #[default]
    Normal,
    /// Recently Deleted
    Trash,
    Smart,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Folder {
    pub id: String,
    pub account: String,
    pub name: String,
    pub parent: Option<String>,
    pub kind: FolderKind,
    /// a Smart Folder's query (Notes' JSON), None for a plain folder
    pub smart: Option<String>,
    /// the account's default "Notes" folder
    pub default: bool,
    /// manual order among siblings (ICFolder.sortOrder)
    pub sort_order: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub id: String,
    pub name: String,
    /// ICAccount.accountType: 1 iCloud, 3 On My Mac
    pub kind: i64,
}

/// The key a locked note is encrypted with (Apple's scheme: PBKDF2-SHA256 of the password and this
/// salt gives the key that wraps each object's own AES key). Made by [`crate::lock`] from the password.
#[derive(Clone, Default, PartialEq)]
pub struct Lock {
    pub hint: Option<String>,
    pub salt: Vec<u8>,
    pub iterations: u32,
    /// the password-derived key-encrypting key
    pub(crate) kek: [u8; 16],
}

impl std::fmt::Debug for Lock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lock").field("hint", &self.hint).field("iterations", &self.iterations).finish_non_exhaustive()
    }
}

/// A tag known to an account (ICHashtag).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tag {
    pub account: String,
    pub name: String,
}

/// Everything in one library.
#[derive(Clone, Debug, Default)]
pub struct Library {
    pub accounts: Vec<Account>,
    pub folders: Vec<Folder>,
    pub notes: Vec<Note>,
    pub tags: Vec<Tag>,
}

impl Library {
    pub fn note(&self, id: &str) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    pub fn folder(&self, id: &str) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == id)
    }

    pub fn notes_in<'a>(&'a self, folder: &'a str) -> impl Iterator<Item = &'a Note> + 'a {
        self.notes.iter().filter(move |n| n.folder == folder)
    }

    /// Notes directly in a folder (Smart Folders: 0, they hold no notes of their own).
    pub fn note_count(&self, folder: &str) -> usize {
        self.notes.iter().filter(|n| n.folder == folder).count()
    }

    pub fn subfolders<'a>(&'a self, parent: Option<&'a str>) -> impl Iterator<Item = &'a Folder> + 'a {
        self.folders.iter().filter(move |f| f.parent.as_deref() == parent)
    }
}
