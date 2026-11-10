//! Logic's plug-ins → ours (filled in as each stand-in is written). Keys are the plug-in name as
//! the project's AuCU chunk gives it (and later its manufacturer|subtype code).

use crate::{Effect, Instrument};

pub fn effect(_logic_name: &str, _rate: f32) -> Option<Box<dyn Effect>> {
    None
}

pub fn instrument(_logic_name: &str, _rate: f32) -> Option<Box<dyn Instrument>> {
    None
}
