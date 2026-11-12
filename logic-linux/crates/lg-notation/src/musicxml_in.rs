//! Reading MusicXML (partwise, uncompressed) for File ▸ Import ▸ MusicXML: each part's notes in
//! ticks (ties joined, chords, backup / forward, grace notes left out), its staves' clefs, and
//! the notation that maps to score symbols — dynamics, words, wedges, slurs, chord symbols
//! (harmony) and lyrics — plus the title, the first key and time signature.

use crate::{Clef, PPQ};

/// A part read: its name, notes (tick, length, MIDI pitch, staff 0-based, velocity), clefs per
/// staff, and its symbols.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InPart {
    pub name: String,
    pub notes: Vec<(u32, u32, u8, u8, u8)>,
    pub clefs: Vec<Clef>,
    pub symbols: Vec<InSymbol>,
}

/// A score symbol from the file.
#[derive(Clone, Debug, PartialEq)]
pub struct InSymbol {
    pub tick: u32,
    pub len: u32,
    pub staff: u8,
    /// "dynamic", "text", "lyric", "chord", "crescendo", "decrescendo", "slur"
    pub kind: &'static str,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Imported {
    pub title: String,
    pub fifths: i8,
    pub time: Option<(u8, u8)>,
    pub parts: Vec<InPart>,
}

// ---- a small XML tree

#[derive(Clone, Debug, Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    kids: Vec<Node>,
    text: String,
}

impl Node {
    fn kid(&self, n: &str) -> Option<&Node> {
        self.kids.iter().find(|k| k.name == n)
    }
    fn all<'a>(&'a self, n: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.kids.iter().filter(move |k| k.name == n)
    }
    fn text_of(&self, n: &str) -> Option<&str> {
        self.kid(n).map(|k| k.text.trim())
    }
    fn num(&self, n: &str) -> Option<f64> {
        self.text_of(n)?.parse().ok()
    }
    fn attr(&self, n: &str) -> Option<&str> {
        self.attrs.iter().find(|a| a.0 == n).map(|a| a.1.as_str())
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
        let body = &after[1..];
        let Some(end) = body.find(q) else { break };
        out.push((name, unescape(&body[..end])));
        rest = &body[end + 1..];
    }
    out
}

fn parse_tree(xml: &str) -> Option<Node> {
    let mut stack: Vec<Node> = vec![Node::default()];
    let mut i = 0;
    let b = xml.as_bytes();
    while i < b.len() {
        if b[i] == b'<' {
            let rest = &xml[i..];
            if rest.starts_with("<!--") {
                i += rest.find("-->").map_or(rest.len(), |e| e + 3);
                continue;
            }
            if rest.starts_with("<?") || rest.starts_with("<!") {
                i += rest.find('>').map_or(rest.len(), |e| e + 1);
                continue;
            }
            let end = rest.find('>')?;
            let tag = &rest[1..end];
            i += end + 1;
            if let Some(name) = tag.strip_prefix('/') {
                let n = stack.pop()?;
                if n.name != name.trim() {
                    return None;
                }
                stack.last_mut()?.kids.push(n);
            } else {
                let selfclose = tag.ends_with('/');
                let tag = tag.trim_end_matches('/');
                let (name, attrs) = tag.split_once(char::is_whitespace).map_or((tag, ""), |(a, b)| (a, b));
                let n = Node { name: name.to_string(), attrs: parse_attrs(attrs), ..Default::default() };
                if selfclose {
                    stack.last_mut()?.kids.push(n);
                } else {
                    stack.push(n);
                }
            }
        } else {
            let end = xml[i..].find('<').map_or(xml.len(), |e| i + e);
            stack.last_mut()?.text += &unescape(&xml[i..end]);
            i = end;
        }
    }
    let mut root = stack.pop()?;
    if !stack.is_empty() {
        return None;
    }
    root.kids.pop()
}

fn clef_of(sign: &str, line: u8) -> Clef {
    match (sign, line) {
        ("F", _) => Clef::Bass,
        ("C", 4) => Clef::Tenor,
        ("C", _) => Clef::Alto,
        ("percussion", _) => Clef::Percussion,
        _ => Clef::Treble,
    }
}

const DYNAMICS: [&str; 14] = ["ppp", "pp", "p", "mp", "mf", "f", "ff", "fff", "sf", "sfz", "sffz", "fp", "rfz", "fz"];

/// Reads a MusicXML score-partwise document.
pub fn read(xml: &str) -> Option<Imported> {
    let root = parse_tree(xml)?;
    if root.name != "score-partwise" {
        return None;
    }
    let mut out = Imported::default();
    out.title = root.kid("work").and_then(|w| w.text_of("work-title")).or_else(|| root.text_of("movement-title")).unwrap_or("").to_string();
    let names: Vec<(String, String)> = root.kid("part-list").map(|pl| pl.all("score-part").map(|sp| (sp.attr("id").unwrap_or("").to_string(), sp.text_of("part-name").unwrap_or("").to_string())).collect()).unwrap_or_default();
    let mut first_attrs = true;
    for part in root.all("part") {
        let id = part.attr("id").unwrap_or("");
        let mut ip = InPart { name: names.iter().find(|n| n.0 == id).map(|n| n.1.clone()).unwrap_or_else(|| id.to_string()), clefs: vec![Clef::Treble], ..Default::default() };
        let mut divisions = 1.0f64;
        let mut cursor = 0.0f64; // in divisions
        let mut last_start = 0.0f64;
        let mut velocity = 80u8;
        // open ties: pitch → index into notes; open slurs / wedges: number → (tick, staff)
        let mut ties: Vec<(u8, usize)> = Vec::new();
        let mut slurs: Vec<(String, u32, u8)> = Vec::new();
        let mut wedges: Vec<(String, u32, u8, bool)> = Vec::new();
        for m in part.all("measure") {
            for el in &m.kids {
                let dv = divisions;
                let tick = move |d: f64| ((d / dv) * PPQ as f64).round().max(0.0) as u32;
                match el.name.as_str() {
                    "attributes" => {
                        if let Some(d) = el.num("divisions").filter(|d| *d > 0.0) {
                            divisions = d;
                        }
                        if first_attrs {
                            if let Some(f) = el.kid("key").and_then(|k| k.num("fifths")) {
                                out.fifths = f as i8;
                            }
                            if let Some(t) = el.kid("time") {
                                if let (Some(b), Some(bt)) = (t.num("beats"), t.num("beat-type")) {
                                    out.time = Some((b as u8, bt as u8));
                                }
                            }
                        }
                        if let Some(s) = el.num("staves") {
                            ip.clefs.resize((s as usize).max(1), Clef::Treble);
                        }
                        for c in el.all("clef") {
                            let n = c.attr("number").and_then(|x| x.parse::<usize>().ok()).unwrap_or(1).max(1) - 1;
                            if ip.clefs.len() <= n {
                                ip.clefs.resize(n + 1, Clef::Treble);
                            }
                            // (only the first clef of each staff: later ones are changes)
                            if cursor == 0.0 {
                                ip.clefs[n] = clef_of(c.text_of("sign").unwrap_or("G"), c.num("line").unwrap_or(2.0) as u8);
                            }
                        }
                    }
                    "backup" => cursor = (cursor - el.num("duration").unwrap_or(0.0)).max(0.0),
                    "forward" => cursor += el.num("duration").unwrap_or(0.0),
                    "direction" => {
                        let staff = el.num("staff").map_or(0, |s| (s as u8).max(1) - 1);
                        let at = tick(cursor + el.num("offset").unwrap_or(0.0));
                        if let Some(v) = el.kid("sound").and_then(|s| s.attr("dynamics")).and_then(|d| d.parse::<f64>().ok()) {
                            velocity = (v * 0.9).round().clamp(1.0, 127.0) as u8;
                        }
                        for dt in el.all("direction-type") {
                            if let Some(d) = dt.kid("dynamics") {
                                if let Some(k) = d.kids.iter().find(|k| DYNAMICS.contains(&k.name.as_str())) {
                                    ip.symbols.push(InSymbol { tick: at, len: 0, staff, kind: "dynamic", text: k.name.clone() });
                                    velocity = match k.name.as_str() { "ppp" => 16, "pp" => 32, "p" => 48, "mp" => 64, "mf" => 80, "f" => 96, "ff" => 112, "fff" => 127, _ => velocity };
                                }
                            }
                            for w in dt.all("words") {
                                if !w.text.trim().is_empty() {
                                    ip.symbols.push(InSymbol { tick: at, len: 0, staff, kind: "text", text: w.text.trim().to_string() });
                                }
                            }
                            if let Some(w) = dt.kid("wedge") {
                                let num = w.attr("number").unwrap_or("1").to_string();
                                match w.attr("type") {
                                    Some("crescendo") => wedges.push((num, at, staff, true)),
                                    Some("diminuendo") => wedges.push((num, at, staff, false)),
                                    Some("stop") => {
                                        if let Some(k) = wedges.iter().position(|x| x.0 == num) {
                                            let (_, a, s, up) = wedges.remove(k);
                                            ip.symbols.push(InSymbol { tick: a, len: at.saturating_sub(a), staff: s, kind: if up { "crescendo" } else { "decrescendo" }, text: String::new() });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    "harmony" => {
                        let root_step = el.kid("root").and_then(|r| r.text_of("root-step")).unwrap_or("C");
                        let alter = el.kid("root").and_then(|r| r.num("root-alter")).unwrap_or(0.0);
                        let kind = el.kid("kind");
                        let kt = kind.and_then(|k| k.attr("text")).map(str::to_string).unwrap_or_else(|| match kind.map(|k| k.text.trim()) {
                            Some("minor") => "m".into(),
                            Some("dominant") => "7".into(),
                            Some("major-seventh") => "maj7".into(),
                            Some("minor-seventh") => "m7".into(),
                            Some("diminished") => "dim".into(),
                            Some("augmented") => "+".into(),
                            Some("suspended-fourth") => "sus4".into(),
                            _ => String::new(),
                        });
                        let acc = if alter > 0.0 { "#" } else if alter < 0.0 { "b" } else { "" };
                        let mut text = format!("{root_step}{acc}{kt}");
                        if let Some(bs) = el.kid("bass").and_then(|b| b.text_of("bass-step")) {
                            let ba = el.kid("bass").and_then(|b| b.num("bass-alter")).unwrap_or(0.0);
                            text += &format!("/{bs}{}", if ba > 0.0 { "#" } else if ba < 0.0 { "b" } else { "" });
                        }
                        ip.symbols.push(InSymbol { tick: tick(cursor + el.num("offset").unwrap_or(0.0)), len: 0, staff: 0, kind: "chord", text });
                    }
                    "note" => {
                        let chord = el.kid("chord").is_some();
                        let grace = el.kid("grace").is_some();
                        let dur = el.num("duration").unwrap_or(0.0);
                        let start = if chord { last_start } else { cursor };
                        let staff = el.num("staff").map_or(0, |s| (s as u8).max(1) - 1);
                        if !chord {
                            last_start = cursor;
                            cursor += dur;
                        }
                        let at = tick(start);
                        for l in el.all("lyric") {
                            if let Some(t) = l.text_of("text").filter(|t| !t.is_empty()) {
                                let dash = matches!(l.text_of("syllabic"), Some("begin" | "middle"));
                                ip.symbols.push(InSymbol { tick: at, len: 0, staff, kind: "lyric", text: if dash { format!("{t}-") } else { t.to_string() } });
                            }
                        }
                        if let Some(nt) = el.kid("notations") {
                            for s in nt.all("slur") {
                                let num = s.attr("number").unwrap_or("1").to_string();
                                match s.attr("type") {
                                    Some("start") => slurs.push((num, at, staff)),
                                    Some("stop") => {
                                        if let Some(k) = slurs.iter().position(|x| x.0 == num) {
                                            let (_, a, st) = slurs.remove(k);
                                            ip.symbols.push(InSymbol { tick: a, len: at.saturating_sub(a), staff: st, kind: "slur", text: String::new() });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        if grace || el.kid("rest").is_some() {
                            continue;
                        }
                        let Some(p) = el.kid("pitch") else { continue };
                        let step = match p.text_of("step").unwrap_or("C") {
                            "D" => 2,
                            "E" => 4,
                            "F" => 5,
                            "G" => 7,
                            "A" => 9,
                            "B" => 11,
                            _ => 0,
                        };
                        let midi = ((p.num("octave").unwrap_or(4.0) as i32 + 1) * 12 + step + p.num("alter").unwrap_or(0.0).round() as i32).clamp(0, 127) as u8;
                        let len = tick(start + dur) - at;
                        let tie_stop = el.all("tie").any(|t| t.attr("type") == Some("stop"));
                        let tie_start = el.all("tie").any(|t| t.attr("type") == Some("start"));
                        let k = if let (true, Some(open)) = (tie_stop, ties.iter().position(|x| x.0 == midi)) {
                            // a tied continuation: the note it's tied from grows
                            let (_, idx) = ties.remove(open);
                            let n = &mut ip.notes[idx];
                            n.1 = (at + len).saturating_sub(n.0);
                            idx
                        } else {
                            ip.notes.push((at, len.max(1), midi, staff, velocity));
                            ip.notes.len() - 1
                        };
                        if tie_start {
                            ties.push((midi, k));
                        }
                    }
                    _ => {}
                }
            }
            if m.kids.iter().any(|k| k.name == "attributes") {
                first_attrs = false;
            }
        }
        ip.notes.sort_by_key(|n| (n.0, n.2));
        out.parts.push(ip);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 3.1 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="3.1">
  <work><work-title>Test &amp; Song</work-title></work>
  <part-list>
    <score-part id="P1"><part-name>Flute</part-name></score-part>
    <score-part id="P2"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes><divisions>2</divisions><key><fifths>-2</fifths></key><time><beats>3</beats><beat-type>4</beat-type></time><clef><sign>G</sign><line>2</line></clef></attributes>
      <direction placement="below"><direction-type><dynamics><mf/></dynamics></direction-type></direction>
      <harmony><root><root-step>B</root-step><root-alter>-1</root-alter></root><kind>major</kind></harmony>
      <note><pitch><step>B</step><alter>-1</alter><octave>4</octave></pitch><duration>2</duration><tie type="start"/><notations><slur type="start" number="1"/></notations><lyric><syllabic>begin</syllabic><text>Hal</text></lyric></note>
      <note><pitch><step>B</step><alter>-1</alter><octave>4</octave></pitch><duration>2</duration><tie type="stop"/></note>
      <note><rest/><duration>1</duration></note>
      <note><pitch><step>D</step><octave>5</octave></pitch><duration>1</duration><notations><slur type="stop" number="1"/></notations></note>
    </measure>
  </part>
  <part id="P2">
    <measure number="1">
      <attributes><divisions>1</divisions><staves>2</staves><clef number="1"><sign>G</sign><line>2</line></clef><clef number="2"><sign>F</sign><line>4</line></clef></attributes>
      <note><pitch><step>C</step><octave>4</octave></pitch><duration>3</duration><staff>1</staff></note>
      <note><chord/><pitch><step>E</step><octave>4</octave></pitch><duration>3</duration><staff>1</staff></note>
      <backup><duration>3</duration></backup>
      <note><pitch><step>C</step><octave>3</octave></pitch><duration>3</duration><staff>2</staff></note>
    </measure>
  </part>
</score-partwise>"#;

    #[test]
    fn reads_parts_notes_and_symbols() {
        let s = read(DOC).expect("reads");
        assert_eq!(s.title, "Test & Song");
        assert_eq!((s.fifths, s.time), (-2, Some((3, 4))));
        assert_eq!(s.parts.len(), 2);
        let fl = &s.parts[0];
        assert_eq!(fl.name, "Flute");
        // the tied B♭ is one note of two beats; the rest moves the D to beat 2½
        assert_eq!(fl.notes, vec![(0, 1920, 70, 0, 80), (2400, 480, 74, 0, 80)]);
        let kinds: Vec<(&str, u32, &str)> = fl.symbols.iter().map(|x| (x.kind, x.tick, x.text.as_str())).collect();
        assert!(kinds.contains(&("dynamic", 0, "mf")));
        assert!(kinds.contains(&("chord", 0, "Bb")));
        assert!(kinds.contains(&("lyric", 0, "Hal-")));
        assert!(fl.symbols.iter().any(|x| x.kind == "slur" && x.len == 2400));
        let pn = &s.parts[1];
        assert_eq!(pn.clefs, vec![Clef::Treble, Clef::Bass]);
        assert_eq!(pn.notes, vec![(0, 2880, 48, 1, 80), (0, 2880, 60, 0, 80), (0, 2880, 64, 0, 80)]);
        assert!(read("<score-timewise/>").is_none());
    }
}
