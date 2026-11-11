//! Reading Logic Pro projects (.logicx packages and project folders), from the Bramble Logic
//! research: the container (chunks), event lists (events), the song as tracks / regions / notes
//! (project) and plug-in settings (plugin).

pub mod arrange;
pub mod automation;
pub mod chunks;
pub mod events;
pub mod liveloops;
pub mod midi;
pub mod plugin;
pub mod project;
pub mod save;

pub use project::{open, Project, Region, Track, TrackKind};
