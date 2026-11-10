//! The Session Player characters: Logic's drummers by genre (names, personalities, kits and
//! tempos as Logic lists them, from the research's characters table), each with a [`Style`] of
//! our own that sets how the generator plays for them.

use crate::kit::KitMap;

/// The genres of Logic's drummer browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Genre {
    Rock,
    Songwriter,
    Alternative,
    RnB,
    Electronic,
    HipHop,
    ElectronicAlternative,
    Percussion,
}

impl Genre {
    pub fn name(self) -> &'static str {
        match self {
            Genre::Rock => "Rock",
            Genre::Songwriter => "Songwriter",
            Genre::Alternative => "Alternative",
            Genre::RnB => "R&B",
            Genre::Electronic => "Electronic",
            Genre::HipHop => "Hip Hop",
            Genre::ElectronicAlternative => "Alternative",
            Genre::Percussion => "Percussion",
        }
    }

    /// The drummer type the genre is listed under.
    pub fn electronic(self) -> bool {
        matches!(self, Genre::Electronic | Genre::HipHop | Genre::ElectronicAlternative)
    }
}

/// What the groove is built on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Groove {
    /// Kick on the downbeat, snare on the backbeats (2 and 4)
    Backbeat,
    /// Driving eighths, kick and snare busy (punk, garage)
    Driving,
    /// Kick on every beat, snare / clap on the backbeats, open hats on the off-beats
    FourOnFloor,
    /// Half-time: the snare on beat 3 (dubstep, trap)
    HalfTime,
    /// Swung sixteenths, a lazy kick and hard backbeat (boom bap)
    BoomBap,
    /// Trap: half-time snare, hats rolling in 16ths, 32nds and triplets, a syncopated kick
    Trap,
    /// Brushes: swept, soft, the snare carries the time
    Brush,
    /// Hand percussion only
    Percussion,
}

/// How a character plays: our own settings for the generator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub groove: Groove,
    /// 0 eighth-note hats .. 1 sixteenth-note hats (at full complexity)
    pub sixteenths: f32,
    /// default swing (0..1) and whether it swings sixteenths
    pub swing: f32,
    pub swing_sixteenths: bool,
    /// how much the kick wanders off the beat (0 plain .. 1 busy, syncopated)
    pub kick_syncopation: f32,
    /// toms in fills (0 snare-only fills .. 1 around the kit)
    pub toms: f32,
    /// the ride instead of the hats in loud sections (0 never .. 1 always)
    pub ride: f32,
    /// open hats now and then (0..1)
    pub open_hats: f32,
    /// ghost-note habit (scales the Ghost Notes control)
    pub ghosts: f32,
    /// velocity range (how far soft and loud strokes sit apart, 1 = the Pop Rock measurement)
    pub dynamics: f32,
    /// timing looseness (scales Humanize)
    pub looseness: f32,
    /// default feel (-1 pull .. 1 push)
    pub feel: f32,
    /// clap layered on the backbeat (electronic)
    pub clap: bool,
}

impl Style {
    const fn base(groove: Groove) -> Style {
        Style {
            groove,
            sixteenths: 0.3,
            swing: 0.0,
            swing_sixteenths: true,
            kick_syncopation: 0.4,
            toms: 0.6,
            ride: 0.3,
            open_hats: 0.3,
            ghosts: 1.0,
            dynamics: 1.0,
            looseness: 1.0,
            feel: 0.0,
            clap: false,
        }
    }
}

/// One Session Player drummer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Character {
    /// The CharacterIdentifier Logic stores in a region ("Acoustic Drummer - Pop Rock").
    pub id: &'static str,
    /// The name in the browser ("Pop Rock").
    pub name: &'static str,
    /// The drummer's personality name ("Kyle").
    pub personality: &'static str,
    pub genre: Genre,
    /// The default kit patch (Drum Kit / Drum Machine library patch name).
    pub kit: &'static str,
    pub default_tempo: u32,
    pub style: Style,
}

impl Character {
    pub fn kit_map(&self) -> KitMap {
        if self.genre.electronic() { KitMap::DrumMachineDesigner } else { KitMap::DrumKitDesigner }
    }

    pub fn electronic(&self) -> bool {
        self.genre.electronic()
    }

    /// Finds a character by its identifier, name or personality (case and spacing ignored).
    pub fn find(key: &str) -> Option<&'static Character> {
        let k = key.trim().to_ascii_lowercase();
        DRUMMERS.iter().find(|c| c.id.trim().eq_ignore_ascii_case(&k) || c.name.eq_ignore_ascii_case(&k) || c.personality.eq_ignore_ascii_case(&k))
    }

    /// The default drummer (Pop Rock, Kyle, SoCal kit).
    pub fn default_drummer() -> &'static Character {
        &DRUMMERS[0]
    }
}

macro_rules! ch {
    ($id:expr, $name:expr, $who:expr, $genre:ident, $kit:expr, $tempo:expr, $style:expr) => {
        Character { id: $id, name: $name, personality: $who, genre: Genre::$genre, kit: $kit, default_tempo: $tempo, style: $style }
    };
}

const fn with(mut s: Style, sixteenths: f32, swing: f32, kick: f32, toms: f32, ride: f32, open: f32, ghosts: f32, feel: f32) -> Style {
    s.sixteenths = sixteenths;
    s.swing = swing;
    s.kick_syncopation = kick;
    s.toms = toms;
    s.ride = ride;
    s.open_hats = open;
    s.ghosts = ghosts;
    s.feel = feel;
    s
}

const fn clap(mut s: Style) -> Style {
    s.clap = true;
    s
}

const fn eighth_swing(mut s: Style) -> Style {
    s.swing_sixteenths = false;
    s
}

const fn loose(mut s: Style, looseness: f32, dynamics: f32) -> Style {
    s.looseness = looseness;
    s.dynamics = dynamics;
    s
}

use Groove::*;

/// The drummers, Logic's list (identifiers as Logic stores them in regions; note "Modern Motown "
/// keeps Logic's trailing space).
pub static DRUMMERS: [Character; 33] = [
    // Rock
    ch!("Acoustic Drummer - Pop Rock", "Pop Rock", "Kyle", Rock, "SoCal", 110, with(Style::base(Backbeat), 0.25, 0.0, 0.35, 0.55, 0.35, 0.2, 1.0, 0.0)),
    ch!("Acoustic Drummer - Retro Rock", "Retro Rock", "Logan", Rock, "Retro Rock", 110, with(Style::base(Backbeat), 0.15, 0.08, 0.45, 0.8, 0.5, 0.35, 0.8, -0.05)),
    ch!("Acoustic Drummer - Hard Rock", "Hard Rock", "Anders", Rock, "Heavy", 80, loose(with(Style::base(Backbeat), 0.2, 0.0, 0.5, 0.9, 0.6, 0.3, 0.5, -0.1), 0.8, 0.8)),
    ch!("Acoustic Drummer - Punk Rock", "Punk Rock", "Max", Rock, "East Bay", 140, loose(with(Style::base(Driving), 0.05, 0.0, 0.3, 0.7, 0.4, 0.4, 0.3, 0.25), 1.1, 0.8)),
    ch!("Acoustic Drummer - Funk Rock", "Funk Rock", "Jesse", Rock, "Smash", 95, with(Style::base(Backbeat), 0.8, 0.12, 0.7, 0.6, 0.2, 0.45, 1.5, 0.0)),
    ch!("Acoustic Drummer - Britpop", "Britpop", "Ian", Rock, "Manchester", 80, with(Style::base(Backbeat), 0.5, 0.05, 0.4, 0.6, 0.4, 0.35, 1.0, -0.1)),
    // Songwriter
    ch!("Acoustic Drummer - Funky Songwriter", "Funky Songwriter", "Levi", Songwriter, "Sunset", 90, with(Style::base(Backbeat), 0.7, 0.15, 0.55, 0.4, 0.2, 0.3, 1.4, -0.1)),
    ch!("Acoustic Drummer - Pop Songwriter", "Pop Songwriter", "Darcy", Songwriter, "Bluebird", 100, with(Style::base(Backbeat), 0.4, 0.0, 0.35, 0.45, 0.25, 0.2, 1.0, 0.0)),
    ch!("Acoustic Drummer - Experimental Songwriter", "Experimental Songwriter", "Graham", Songwriter, "Scientific Method", 100, with(Style::base(Backbeat), 0.6, 0.0, 0.8, 0.7, 0.3, 0.3, 1.0, 0.0)),
    ch!("Acoustic Drummer - Roots Songwriter", "Roots Songwriter", "Mason", Songwriter, "Roots", 85, loose(with(Style::base(Backbeat), 0.1, 0.2, 0.3, 0.5, 0.4, 0.2, 0.8, -0.15), 1.3, 0.9)),
    ch!("Acoustic Drummer - 60s Songwriter", "60's Songwriter", "Parker", Songwriter, "Liverpool", 110, with(eighth_swing(Style::base(Backbeat)), 0.1, 0.1, 0.35, 0.7, 0.5, 0.3, 0.6, 0.0)),
    ch!("Acoustic Drummer - Roots Brush", "Roots Brush", "Austin", Songwriter, "Blue Ridge", 85, loose(with(eighth_swing(Style::base(Brush)), 0.0, 0.3, 0.25, 0.2, 0.1, 0.0, 0.8, -0.1), 1.3, 0.7)),
    ch!("Acoustic Drummer - Pop Brush", "Pop Brush", "Tyrell", Songwriter, "Speakeasy", 75, loose(with(eighth_swing(Style::base(Brush)), 0.0, 0.2, 0.25, 0.2, 0.1, 0.0, 0.8, -0.1), 1.2, 0.7)),
    // Alternative
    ch!("Acoustic Drummer - Indie Pop", "Indie Pop", "Aidan", Alternative, "Portland", 100, with(Style::base(Backbeat), 0.5, 0.0, 0.45, 0.6, 0.3, 0.25, 0.9, 0.05)),
    ch!("Acoustic Drummer - Indie Disco", "Indie Disco", "Nikki", Alternative, "Four on the Floor", 120, with(Style::base(FourOnFloor), 0.6, 0.0, 0.2, 0.4, 0.1, 0.8, 0.7, 0.05)),
    ch!("Acoustic Drummer - Indie Rock", "Indie Rock", "Gavin", Alternative, "Brooklyn", 100, with(Style::base(Backbeat), 0.35, 0.0, 0.45, 0.75, 0.5, 0.35, 0.8, 0.05)),
    ch!("Acoustic Drummer - Garage Rock", "Garage Rock", "Zak", Alternative, "Detroit Garage", 130, loose(with(Style::base(Driving), 0.1, 0.0, 0.35, 0.7, 0.5, 0.45, 0.4, 0.15), 1.2, 0.85)),
    // R&B
    ch!("Acoustic Drummer - Modern R&B", "Modern R&B", "Rose", RnB, "Slow Jam", 75, with(Style::base(Backbeat), 0.8, 0.25, 0.75, 0.35, 0.1, 0.2, 1.4, -0.2)),
    ch!("Acoustic Drummer - Neo Soul", "Neo Soul", "Curtis", RnB, "Neo Soul", 90, loose(with(Style::base(Backbeat), 0.7, 0.35, 0.7, 0.4, 0.2, 0.25, 1.6, -0.3), 1.3, 1.1)),
    ch!("Acoustic Drummer - Modern Motown ", "Modern Motown", "Benny", RnB, "Motown Revisited", 100, with(eighth_swing(Style::base(Backbeat)), 0.2, 0.15, 0.4, 0.5, 0.2, 0.3, 1.0, 0.0)),
    // Electronic
    ch!("Electronic Drummer - EDM", "Big Room EDM", "Magnus", Electronic, "Big Room", 128, clap(with(Style::base(FourOnFloor), 0.5, 0.0, 0.05, 0.2, 0.1, 0.9, 0.0, 0.0))),
    ch!("Electronic Drummer - Tech House", "Tech House", "Leah", Electronic, "Deep Tech", 125, clap(with(Style::base(FourOnFloor), 0.8, 0.1, 0.15, 0.2, 0.1, 0.9, 0.3, 0.0))),
    ch!("Electronic Drummer - Dubstep", "Dubstep", "Jasper", Electronic, "Pile Driver", 140, with(Style::base(HalfTime), 0.6, 0.0, 0.7, 0.3, 0.1, 0.2, 0.2, 0.0)),
    ch!("Electronic Drummer - Modern House", "Modern House", "Julian", Electronic, "After Party", 120, clap(with(Style::base(FourOnFloor), 0.7, 0.15, 0.1, 0.2, 0.2, 0.9, 0.3, 0.0))),
    // Hip Hop
    ch!("Electronic Drummer - Trap", "Trap", "Dez", HipHop, "Trap Door", 70, clap(with(Style::base(Trap), 1.0, 0.0, 0.85, 0.1, 0.0, 0.05, 0.3, 0.0))),
    ch!("Electronic Drummer - Modern Hip Hop", "Modern Hip Hop", "Anton", HipHop, "Beat Machine", 85, clap(with(Style::base(BoomBap), 0.8, 0.15, 0.75, 0.2, 0.0, 0.1, 0.5, 0.0))),
    ch!("Electronic Drummer - Boom Bap", "Boom Bap", "Maurice", HipHop, "Crate Digger", 85, with(Style::base(BoomBap), 0.4, 0.45, 0.7, 0.2, 0.0, 0.15, 0.8, -0.1)),
    ch!("Electronic Drummer - Electro Trap", "Electro Trap", "Ronnie", HipHop, "Seismic", 70, clap(with(Style::base(Trap), 1.0, 0.0, 0.8, 0.1, 0.0, 0.05, 0.2, 0.0))),
    // Electronic alternative
    ch!("Electronic Drummer - Modern 80s", "Modern 80's", "Maya", ElectronicAlternative, "Neon", 100, clap(with(Style::base(Backbeat), 0.6, 0.0, 0.35, 0.5, 0.0, 0.3, 0.2, 0.0))),
    ch!("Electronic Drummer - Synthpop", "Synthpop", "Duncan", ElectronicAlternative, "Silverlake", 120, clap(with(Style::base(Backbeat), 0.7, 0.0, 0.3, 0.4, 0.0, 0.4, 0.2, 0.0))),
    // Percussion
    ch!("Percussion v2 - Latin", "Latin Percussion", "Isabela", Percussion, "Latin", 95, with(Style::base(Percussion), 0.8, 0.1, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0)),
    ch!("Percussion v2 - Pop Rock", "Pop Percussion", "Quincy", Percussion, "Studio", 110, with(Style::base(Percussion), 0.7, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0)),
    ch!("Percussion v2 - Songwriter", "Songwriter Percussion", "Finn", Percussion, "Coffee Shop", 90, with(Style::base(Percussion), 0.5, 0.1, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0)),
];

/// The drummers of one genre, in Logic's order.
pub fn drummers_in(genre: Genre) -> impl Iterator<Item = &'static Character> {
    DRUMMERS.iter().filter(move |c| c.genre == genre)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster() {
        assert_eq!(Character::find("Kyle").unwrap().kit, "SoCal");
        assert_eq!(Character::find("Acoustic Drummer - Pop Rock").unwrap().name, "Pop Rock");
        assert_eq!(Character::find("Acoustic Drummer - Modern Motown").unwrap().personality, "Benny");
        assert_eq!(drummers_in(Genre::Rock).count(), 6);
        assert!(Character::find("Trap").unwrap().electronic());
        assert_eq!(Character::find("Trap").unwrap().kit_map(), KitMap::DrumMachineDesigner);
    }
}
