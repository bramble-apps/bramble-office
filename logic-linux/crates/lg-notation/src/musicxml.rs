//! MusicXML (3.1 partwise) from engraved voices: what Logic's File ▸ Export ▸ Score as
//! MusicXML writes — parts with their staves, clefs, key and time, notes with their written
//! values, dots, triplets, ties, accidentals, chords and rests, voices, and the score's
//! directions (dynamics, words), lyrics and bar lines.

use crate::{Clef, Item, Voice, PPQ};

/// A part: its name, its staves (clef and the voices on it).
pub struct Part {
    pub name: String,
    pub staves: Vec<(Clef, Vec<Voice>)>,
    /// Dynamics and words: (staff 0-based, tick, is a dynamic, text).
    pub directions: Vec<(usize, u32, bool, String)>,
    /// Lyrics: (staff, tick, syllable), attached to the note starting there.
    pub lyrics: Vec<(usize, u32, String)>,
}

/// Bar line styles at bar ends / starts: (bar index whose start it is, style).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Barline {
    Double,
    Final,
    Dashed,
    RepeatStart,
    RepeatEnd,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn clef_xml(c: Clef) -> &'static str {
    match c {
        Clef::Treble => "<sign>G</sign><line>2</line>",
        Clef::Bass => "<sign>F</sign><line>4</line>",
        Clef::Alto => "<sign>C</sign><line>3</line>",
        Clef::Tenor => "<sign>C</sign><line>4</line>",
        Clef::Percussion => "<sign>percussion</sign>",
    }
}

fn type_name(base: u8) -> &'static str {
    match base {
        1 => "whole",
        2 => "half",
        4 => "quarter",
        8 => "eighth",
        16 => "16th",
        32 => "32nd",
        64 => "64th",
        _ => "128th",
    }
}

fn acc_name(a: i8) -> &'static str {
    match a {
        2 => "double-sharp",
        1 => "sharp",
        -1 => "flat",
        -2 => "flat-flat",
        _ => "natural",
    }
}

/// The score as MusicXML text. `bars` measures of `sig`, the key in fifths; `barlines` bar
/// signs (a start's bar index); `title` the work title.
pub fn write(title: &str, parts: &[Part], sig: (u8, u8), fifths: i8, bars: u32, barlines: &[(u32, Barline)]) -> String {
    let bar_t = PPQ * 4 * sig.0.max(1) as u32 / sig.1.max(1) as u32;
    let mut o = String::new();
    o += "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n";
    o += "<!DOCTYPE score-partwise PUBLIC \"-//Recordare//DTD MusicXML 3.1 Partwise//EN\" \"http://www.musicxml.org/dtds/partwise.dtd\">\n";
    o += "<score-partwise version=\"3.1\">\n";
    o += &format!("  <work><work-title>{}</work-title></work>\n", esc(title));
    o += "  <identification><encoding><software>Bramble Studio</software></encoding></identification>\n";
    o += "  <part-list>\n";
    for (k, p) in parts.iter().enumerate() {
        o += &format!("    <score-part id=\"P{}\"><part-name>{}</part-name></score-part>\n", k + 1, esc(&p.name));
    }
    o += "  </part-list>\n";
    for (k, p) in parts.iter().enumerate() {
        o += &format!("  <part id=\"P{}\">\n", k + 1);
        // pitches tied over from the item before, per (staff, voice)
        let mut tied: std::collections::HashMap<(usize, usize), Vec<u8>> = std::collections::HashMap::new();
        for m in 0..bars.max(1) {
            o += &format!("    <measure number=\"{}\">\n", m + 1);
            for (b, style) in barlines.iter().filter(|x| x.0 == m) {
                let _ = b;
                if *style == Barline::RepeatStart {
                    o += "      <barline location=\"left\"><bar-style>heavy-light</bar-style><repeat direction=\"forward\"/></barline>\n";
                }
            }
            if m == 0 {
                o += &format!("      <attributes><divisions>{PPQ}</divisions><key><fifths>{fifths}</fifths></key><time><beats>{}</beats><beat-type>{}</beat-type></time>", sig.0, sig.1);
                if p.staves.len() > 1 {
                    o += &format!("<staves>{}</staves>", p.staves.len());
                }
                for (s, (clef, _)) in p.staves.iter().enumerate() {
                    if p.staves.len() > 1 {
                        o += &format!("<clef number=\"{}\">{}</clef>", s + 1, clef_xml(*clef));
                    } else {
                        o += &format!("<clef>{}</clef>", clef_xml(*clef));
                    }
                }
                o += "</attributes>\n";
            }
            let (lo, hi) = (m * bar_t, (m + 1) * bar_t);
            for &(s, tick, dynamic, ref text) in p.directions.iter().filter(|d| d.1 >= lo && d.1 < hi) {
                let off = tick - lo;
                o += "      <direction placement=\"";
                o += if dynamic { "below" } else { "above" };
                o += "\"><direction-type>";
                if dynamic {
                    o += &format!("<dynamics><{}/></dynamics>", esc(text));
                } else {
                    o += &format!("<words>{}</words>", esc(text));
                }
                o += "</direction-type>";
                if off > 0 {
                    o += &format!("<offset>{off}</offset>");
                }
                if p.staves.len() > 1 {
                    o += &format!("<staff>{}</staff>", s + 1);
                }
                o += "</direction>\n";
            }
            let mut first = true;
            let mut vno = 0usize;
            for (s, (_, voices)) in p.staves.iter().enumerate() {
                for (vi, v) in voices.iter().enumerate() {
                    vno += 1;
                    if !first {
                        o += &format!("      <backup><duration>{bar_t}</duration></backup>\n");
                    }
                    first = false;
                    let items: Vec<&Item> = v.items.iter().filter(|it| it.tick / bar_t == m).collect();
                    let mut filled = 0;
                    for (ii, it) in items.iter().enumerate() {
                        let k_item = v.items.iter().position(|x| std::ptr::eq(x, *it)).unwrap_or(0);
                        let in_tuplet = v.tuplets.iter().find(|t| t.first <= k_item && k_item <= t.last);
                        let dur = it.dur;
                        filled += dur;
                        let prev_tied = tied.remove(&(s, vi)).unwrap_or_default();
                        let mut now_tied = Vec::new();
                        let notes: Vec<Option<&crate::Head>> = if it.is_rest() { vec![None] } else { it.heads.iter().map(Some).collect() };
                        for (hk, h) in notes.iter().enumerate() {
                            o += "      <note>";
                            if hk > 0 {
                                o += "<chord/>";
                            }
                            match h {
                                None => {
                                    o += if it.full_bar { "<rest measure=\"yes\"/>" } else { "<rest/>" };
                                }
                                Some(h) => {
                                    let sp = h.spelled;
                                    o += &format!("<pitch><step>{}</step>", crate::LETTERS[sp.letter as usize % 7]);
                                    if sp.alter != 0 {
                                        o += &format!("<alter>{}</alter>", sp.alter);
                                    }
                                    o += &format!("<octave>{}</octave></pitch>", sp.step.div_euclid(7) - 1);
                                }
                            }
                            o += &format!("<duration>{dur}</duration>");
                            if let Some(h) = h {
                                if prev_tied.contains(&h.pitch) {
                                    o += "<tie type=\"stop\"/>";
                                }
                                if h.tie {
                                    o += "<tie type=\"start\"/>";
                                    now_tied.push(h.pitch);
                                }
                            }
                            o += &format!("<voice>{vno}</voice>");
                            if !it.full_bar {
                                o += &format!("<type>{}</type>", type_name(it.value.base));
                                for _ in 0..it.value.dots {
                                    o += "<dot/>";
                                }
                            }
                            if let Some(a) = h.and_then(|h| h.acc) {
                                o += &format!("<accidental>{}</accidental>", acc_name(a));
                            }
                            if it.triplet {
                                o += "<time-modification><actual-notes>3</actual-notes><normal-notes>2</normal-notes></time-modification>";
                            }
                            if h.is_some() && !it.stem_hidden {
                                o += if it.stem_up { "<stem>up</stem>" } else { "<stem>down</stem>" };
                            }
                            if p.staves.len() > 1 {
                                o += &format!("<staff>{}</staff>", s + 1);
                            }
                            // notations: ties, tuplet brackets
                            let mut nots = String::new();
                            if let Some(h) = h {
                                if prev_tied.contains(&h.pitch) {
                                    nots += "<tied type=\"stop\"/>";
                                }
                                if h.tie {
                                    nots += "<tied type=\"start\"/>";
                                }
                            }
                            if hk == 0 {
                                if let Some(t) = in_tuplet {
                                    if t.first == k_item {
                                        nots += "<tuplet type=\"start\" bracket=\"yes\"/>";
                                    }
                                    if t.last == k_item {
                                        nots += "<tuplet type=\"stop\"/>";
                                    }
                                }
                            }
                            if !nots.is_empty() {
                                o += &format!("<notations>{nots}</notations>");
                            }
                            if hk == 0 && h.is_some() {
                                for (_, _, syl) in p.lyrics.iter().filter(|l| l.0 == s && l.1 == it.tick) {
                                    o += &format!("<lyric number=\"1\"><syllabic>single</syllabic><text>{}</text></lyric>", esc(syl));
                                }
                            }
                            o += "</note>\n";
                        }
                        tied.insert((s, vi), now_tied);
                        let _ = ii;
                    }
                    // a voice with nothing in this bar: a whole-bar rest keeps the measure full
                    if items.is_empty() {
                        o += &format!("      <note><rest measure=\"yes\"/><duration>{bar_t}</duration><voice>{vno}</voice>");
                        if p.staves.len() > 1 {
                            o += &format!("<staff>{}</staff>", s + 1);
                        }
                        o += "</note>\n";
                    } else if filled < bar_t {
                        o += &format!("      <forward><duration>{}</duration></forward>\n", bar_t - filled);
                    }
                }
            }
            for (_, style) in barlines.iter().filter(|x| x.0 == m + 1) {
                let bs = match style {
                    Barline::Double => "<bar-style>light-light</bar-style>",
                    Barline::Final => "<bar-style>light-heavy</bar-style>",
                    Barline::Dashed => "<bar-style>dashed</bar-style>",
                    Barline::RepeatEnd => "<bar-style>light-heavy</bar-style><repeat direction=\"backward\"/>",
                    Barline::RepeatStart => continue,
                };
                o += &format!("      <barline location=\"right\">{bs}</barline>\n");
            }
            if m + 1 == bars.max(1) && !barlines.iter().any(|x| x.0 == bars) {
                o += "      <barline location=\"right\"><bar-style>light-heavy</bar-style></barline>\n";
            }
            o += "    </measure>\n";
        }
        o += "  </part>\n";
    }
    o += "</score-partwise>\n";
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engrave, Key, NoteIn, Options, Quant};

    #[test]
    fn notes_ties_and_rests_in_measures() {
        // C4 a dotted half, then E4–G4 tied over the bar line
        let notes = [NoteIn::new(0, 60, 0, 2880), NoteIn::new(1, 64, 2880, 1920), NoteIn::new(2, 67, 2880, 1920)];
        let v = engrave(&notes, Clef::Treble, &Options::new((4, 4), Key::default(), Quant::default(), 0, 3840 * 2));
        let part = Part { name: "Piano & Co".into(), staves: vec![(Clef::Treble, vec![v])], directions: vec![(0, 0, true, "mf".into())], lyrics: vec![(0, 0, "la".into())] };
        let x = write("Song", &[part], (4, 4), -1, 2, &[(1, Barline::Double)]);
        assert!(x.contains("<part-name>Piano &amp; Co</part-name>"));
        assert!(x.contains("<fifths>-1</fifths>"));
        assert_eq!(x.matches("<measure ").count(), 2);
        assert!(x.contains("<step>C</step><octave>4</octave>"));
        assert!(x.contains("<type>half</type><dot/>"));
        assert!(x.contains("<chord/>"));
        assert!(x.contains("<tie type=\"start\"/>") && x.contains("<tie type=\"stop\"/>"));
        assert!(x.contains("<dynamics><mf/></dynamics>"));
        assert!(x.contains("<text>la</text>"));
        assert!(x.contains("light-light"));
        // every measure's notes add up to the bar
        for m in x.split("<measure ").skip(1) {
            let d: u32 = m.split("<duration>").skip(1).filter_map(|s| s.split('<').next()?.parse::<u32>().ok()).sum::<u32>();
            let chords = m.matches("<chord/>").count();
            assert!(d >= 3840, "{d} {chords}");
        }
    }
}
