//! Reading Logic Pro projects (.logicx packages and project folders), from the Bramble Logic
//! research: the container (chunks), event lists (events), the song as tracks / regions / notes
//! (project) and plug-in settings (plugin).

pub mod arrange;
pub mod automation;
pub mod bandsave;
pub mod chunks;
pub mod displaystate;
pub mod events;
pub mod functions;
pub mod liveloops;
pub mod midi;
pub mod package;
pub mod mixer;
pub mod plugin;
pub mod project;
pub mod regions;
pub mod save;
pub mod score;
pub mod takes;
pub mod timeline;
pub mod transform;
pub mod writeback;

pub use project::{open, Project, Region, Track, TrackKind};
