//! The Notes engine: the library (accounts, folders, notes, attachments) in Apple's NoteStore
//! format, note bodies as Apple's protobuf note documents, and the document model the editor works on.
//! Spec: ~/CascadeProjects/notes-research-src/ (Notes-Research, round NO1).
//!
//! - [`model`]: what the app works on
//! - [`body`]: note bodies (gzip protobuf topotext) <-> [`model::Body`]
//! - [`table`]: table attachments (CRDT mergeable data) <-> [`model::Table`]
//! - [`import`]: read a Mac's Notes library (group.com.apple.notes or a NoteStore.sqlite), read-only
//! - [`store`]: the app's own library, a NoteStore-format SQLite
//! - [`export`]: Markdown, plain text, HTML
//! - [`lock`]: locked notes (Apple's password encryption)
//! - [`paper`]: drawings as strokes
//! - [`paperkit`]: Apple's drawings (PaperKit bundles) read and written
//! - [`native`]: Bramble Notes' own library format (notes as files, a database as their index)
//! - [`sync`]: the library kept in step with other devices through a WebDAV service

pub mod body;
pub mod export;
pub mod import;
pub mod lock;
pub mod model;
pub mod paper;
pub mod native;
pub mod paperkit;
pub mod sync;
pub mod pb;
pub mod store;
pub mod table;
pub mod upnote;

/// nt-core's result type.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    /// a protobuf / gzip / store layout we can't read
    Format(String),
    /// no such note, folder, …
    NotFound(String),
    /// syncing: the server couldn't be reached, refused us, or answered with an error (the
    /// message is for the user as it is)
    Network(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Sql(e) => write!(f, "database error: {e}"),
            Error::Format(s) => write!(f, "unreadable notes data: {s}"),
            Error::NotFound(s) => write!(f, "not found: {s}"),
            Error::Network(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sql(e)
    }
}
