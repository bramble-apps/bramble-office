//! Reading and writing Apple Keynote `.key` files (Keynote 6+ IWA format), losslessly.
//!
//! Layering: [`wire`] (varints, Snappy/IWA framing) -> [`msg::Msg`] (schema-aware lossless
//! protobuf) -> [`iwa::IwaFile`] (archives of objects) -> [`keyfile::KeyFile`] (the package).
//! Structural edits (clone/delete slides, copy objects) live in [`edit`].

pub mod edit;
pub mod iwa;
pub mod keyfile;
pub mod msg;
pub mod crypto;
pub mod objects;
pub mod progress;
pub mod schema;
pub mod tables;
pub mod wire;

pub use iwa::{Archive, IwaFile};
pub use keyfile::{Entry, EntryData, KeyFile};
pub use objects::Extract;
pub use msg::{reference, Msg, ObjId, Val};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    Corrupt(&'static str),
    InFile(String, Box<Error>),
    Invalid(String),
    /// The presentation is password protected (the hint, possibly empty).
    NeedsPassword(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Zip(e) => write!(f, "ZIP error: {e}"),
            Error::Corrupt(s) => write!(f, "corrupt data: {s}"),
            Error::InFile(n, e) => write!(f, "{n}: {e}"),
            Error::Invalid(s) => write!(f, "{s}"),
            Error::NeedsPassword(_) => write!(f, "this presentation is password protected"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Self {
        Error::Zip(e)
    }
}
