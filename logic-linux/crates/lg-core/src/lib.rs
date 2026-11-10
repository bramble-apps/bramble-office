//! Reading Logic Pro projects (.logicx packages and project folders), from the Bramble Logic
//! research: the container (chunks), event lists (events), the song as tracks / regions / notes
//! (project) and plug-in settings (plugin).

pub mod chunks;
pub mod events;
pub mod midi;
pub mod plugin;
pub mod project;

pub use project::{open, Project, Region, Track, TrackKind};
