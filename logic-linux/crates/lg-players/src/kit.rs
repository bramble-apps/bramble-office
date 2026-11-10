//! Kit pieces and the MIDI notes that play them on Logic's two drum instruments.
//!
//! - Drum Kit Designer (the acoustic drummers' kits: SoCal, Bluebird, Brooklyn …) follows the
//!   General MIDI drum map and adds hi-hat articulations below 35 (the research measured note 34,
//!   a half-open / edge hat stroke, played soft between the closed hats).
//! - Drum Machine Designer (the electronic drummers): 48 pads over three pages, each on one note
//!   from C1 (36) up, with GM-style slot names (KIT-NOTES-DrumMachineDesigner in the research).
//!
//! Note numbers: MIDI, 60 = C3 in Logic's naming, so C1 = 36.

/// A kit piece as the drummers think of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Piece {
    Kick,
    Snare,
    /// A snare rimshot / the second snare (accents in fills)
    SnareRim,
    SideStick,
    Clap,
    HatClosed,
    /// The half-open / edge stroke (Drum Kit Designer's note below 35)
    HatHalf,
    HatPedal,
    HatOpen,
    Ride,
    RideBell,
    Crash,
    Crash2,
    Splash,
    China,
    TomHigh,
    TomMid,
    TomLow,
    TomFloor,
    Tambourine,
    Shaker,
    Cowbell,
    Woodblock,
    Bongo,
    Conga,
    Cabasa,
    Triangle,
    Timbale,
    FootStomp,
    Cajon,
    Fx,
}

impl Piece {
    /// The family a piece belongs to (for statistics and for the kit-piece sections).
    pub fn family(self) -> Family {
        use Piece::*;
        match self {
            Kick => Family::Kick,
            Snare | SnareRim | SideStick | Clap => Family::Snare,
            HatClosed | HatHalf | HatPedal | HatOpen => Family::HiHat,
            Ride | RideBell | Crash | Crash2 | Splash | China => Family::Cymbal,
            TomHigh | TomMid | TomLow | TomFloor => Family::Tom,
            _ => Family::Percussion,
        }
    }

    pub fn name(self) -> &'static str {
        use Piece::*;
        match self {
            Kick => "Kick",
            Snare => "Snare",
            SnareRim => "Snare Rim",
            SideStick => "Side Stick",
            Clap => "Clap",
            HatClosed => "Hi-Hat Closed",
            HatHalf => "Hi-Hat Half-Open",
            HatPedal => "Hi-Hat Pedal",
            HatOpen => "Hi-Hat Open",
            Ride => "Ride",
            RideBell => "Ride Bell",
            Crash => "Crash",
            Crash2 => "Crash 2",
            Splash => "Splash",
            China => "China",
            TomHigh => "Tom High",
            TomMid => "Tom Mid",
            TomLow => "Tom Low",
            TomFloor => "Floor Tom",
            Tambourine => "Tambourine",
            Shaker => "Shaker",
            Cowbell => "Cowbell",
            Woodblock => "Woodblock",
            Bongo => "Bongos",
            Conga => "Congas",
            Cabasa => "Cabasa",
            Triangle => "Triangle",
            Timbale => "Timbales",
            FootStomp => "Foot Stomp",
            Cajon => "Cajon",
            Fx => "FX",
        }
    }
}

/// Kit-piece families: the rows of the drummer's statistics, and (Kick+Snare, Hi-Hat+Cymbal,
/// Percussion) the sections of the Session Player's kit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    Kick,
    Snare,
    HiHat,
    Tom,
    Cymbal,
    Percussion,
}

impl Family {
    pub const ALL: [Family; 6] = [Family::Kick, Family::Snare, Family::HiHat, Family::Tom, Family::Cymbal, Family::Percussion];

    pub fn name(self) -> &'static str {
        match self {
            Family::Kick => "Kick",
            Family::Snare => "Snare",
            Family::HiHat => "Hi-Hat",
            Family::Tom => "Toms",
            Family::Cymbal => "Cymbals",
            Family::Percussion => "Percussion",
        }
    }
}

/// Which instrument a kit is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KitMap {
    /// Drum Kit Designer (acoustic kits; GM drum map + hi-hat articulations)
    DrumKitDesigner,
    /// Drum Machine Designer (electronic kits; its pad slots)
    DrumMachineDesigner,
}

impl KitMap {
    /// The note that plays a piece on this instrument.
    pub fn note(self, p: Piece) -> u8 {
        use Piece::*;
        match self {
            KitMap::DrumKitDesigner => match p {
                Kick => 36,
                SideStick => 37,
                Snare => 38,
                Clap => 39,
                SnareRim => 40,
                TomFloor => 41,
                HatClosed => 42,
                TomLow => 43,
                HatPedal => 44,
                TomMid => 47,
                HatOpen => 46,
                TomHigh => 48,
                Crash => 49,
                Ride => 51,
                China => 52,
                RideBell => 53,
                Tambourine => 54,
                Splash => 55,
                Cowbell => 56,
                Crash2 => 57,
                Bongo => 60,
                Conga => 63,
                Timbale => 65,
                Cabasa => 69,
                Shaker => 70,
                Woodblock => 76,
                Triangle => 81,
                HatHalf => 34,
                FootStomp => 35,
                Cajon => 66,
                Fx => 58,
            },
            // the pads of the default Drum Machine Designer kit, by slot (page 1: 36-53, page 2:
            // 48-56 and 60-68, page 3: 69 and up)
            KitMap::DrumMachineDesigner => match p {
                Kick => 36,
                SideStick => 37,
                Snare => 38,
                Clap => 39,
                SnareRim => 40,
                TomLow => 41,
                HatClosed => 42,
                TomMid => 43,
                HatPedal => 44,
                TomHigh => 45,
                HatOpen => 46,
                TomFloor => 41,
                HatHalf => 44,
                Crash | Crash2 | Splash | China => 49,
                Ride | RideBell => 51,
                Shaker => 62,
                Cowbell => 57,
                Tambourine => 54,
                Woodblock => 76,
                Bongo => 60,
                Conga => 63,
                Cabasa => 69,
                Triangle => 81,
                Timbale => 65,
                FootStomp => 47,
                Cajon => 66,
                Fx => 55,
            },
        }
    }

    /// The piece a note plays (the first piece mapped to it).
    pub fn piece(self, note: u8) -> Option<Piece> {
        ALL_PIECES.iter().copied().find(|p| self.note(*p) == note)
    }
}

pub const ALL_PIECES: [Piece; 31] = {
    use Piece::*;
    [
        Kick, Snare, SnareRim, SideStick, Clap, HatClosed, HatHalf, HatPedal, HatOpen, Ride, RideBell, Crash, Crash2, Splash, China, TomHigh, TomMid,
        TomLow, TomFloor, Tambourine, Shaker, Cowbell, Woodblock, Bongo, Conga, Cabasa, Triangle, Timbale, FootStomp, Cajon, Fx,
    ]
};

/// The family a General-MIDI-style drum note belongs to (for measuring any drum part).
pub fn family_of_note(note: u8) -> Family {
    match note {
        35 | 36 => Family::Kick,
        37..=40 => Family::Snare,
        33 | 34 | 42 | 44 | 46 => Family::HiHat,
        41 | 43 | 45 | 47 | 48 | 50 => Family::Tom,
        49 | 51 | 52 | 53 | 55 | 57 | 59 => Family::Cymbal,
        _ => Family::Percussion,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_match_the_research() {
        let d = KitMap::DrumKitDesigner;
        assert_eq!((d.note(Piece::Kick), d.note(Piece::SideStick), d.note(Piece::Snare), d.note(Piece::HatClosed)), (36, 37, 38, 42));
        assert_eq!((d.note(Piece::HatHalf), d.note(Piece::TomMid), d.note(Piece::Crash2)), (34, 47, 57));
        let m = KitMap::DrumMachineDesigner;
        assert_eq!((m.note(Piece::Clap), m.note(Piece::HatPedal), m.note(Piece::HatOpen), m.note(Piece::Ride)), (39, 44, 46, 51));
        for p in ALL_PIECES {
            assert!(d.note(p) >= 33 && m.note(p) >= 36);
            if d.note(p) != 34 && d.note(p) != 35 && p.family() != Family::Percussion {
                assert_eq!(family_of_note(d.note(p)), p.family(), "{p:?}");
            }
        }
        assert_eq!(d.piece(38), Some(Piece::Snare));
    }
}
