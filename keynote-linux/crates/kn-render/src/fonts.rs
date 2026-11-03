//! Fonts: Apple PostScript font names -> installed families (with metric-compatible substitutes),
//! glyph outlines via swash.

use std::collections::HashMap;

use cosmic_text::{fontdb, CacheKey, CacheKeyFlags, FontSystem, SwashCache};

#[derive(Debug, Clone, PartialEq)]
pub struct FontChoice {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
}

pub const OUTLINE_SIZE: f32 = 256.0;

pub struct Fonts {
    pub fs: FontSystem,
    pub cache: SwashCache,
    families: Vec<String>,
    resolved: HashMap<String, FontChoice>,
    outlines: HashMap<(fontdb::ID, u16, u16), Option<Vec<swash::zeno::Command>>>,
    /// Lay text out as Word (and OnlyOffice) do, for a Word document: OnlyOffice's stand-ins
    /// for fonts that aren't installed, lines from the font's own (OS/2 win) height, Word's line
    /// spacing. Off: as Keynote / Pages do. Set by the app for the document being laid out.
    pub word: bool,
    win: HashMap<fontdb::ID, Option<(f32, f32, f32)>>,
    /// Installed metric-matched stand-ins (read on the first missing font).
    stand_ins: Option<Vec<StandIn>>,
}

/// An installed font made to stand in for another (an Apple font) with the same spacing: its
/// name table's record MATCH_NAME_ID reads "<original family>|<original PostScript name>"
/// (made by ~/CascadeProjects/matched-fonts' build_matched.py).
#[derive(Debug, Clone)]
struct StandIn {
    /// the original's family and PostScript name, as `key` makes them
    family: String,
    ps: String,
    /// the style part of the original's PostScript name ("Bold" of "Futura-Bold"), as `key`
    style: String,
    choice: FontChoice,
}

const MATCH_NAME_ID: u16 = 32000;

/// A name for comparing: lowercase, no spaces, dashes or leading dot (".SFNS-Regular").
fn key(s: &str) -> String {
    s.trim_start_matches('.').to_ascii_lowercase().replace([' ', '-', '_'], "")
}

/// Families documents name that stand-ins carry under another name: Apple's system font
/// (SF Pro's text cut) and New York (its text size).
fn stand_in_family(family: &str) -> String {
    let k = key(family);
    match k.as_str() {
        "sfpro" | "sfns" | "sfnstext" | "systemfont" | "applesystemuifont" | "sfui" | "sfuitext" => "sfprotext".into(),
        "sfnsdisplay" | "sfuidisplay" => "sfprodisplay".into(),
        "newyork" => "newyorksmall".into(),
        _ => k,
    }
}

/// The MATCH_NAME_ID record of a font file's face (Windows Unicode or Mac Roman), if any.
fn stand_in_name(data: &[u8], index: u32) -> Option<String> {
    let be16 = |o: usize| data.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let be32 = |o: usize| data.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let dir = if data.get(0..4) == Some(b"ttcf") { be32(12 + 4 * index as usize)? as usize } else { 0 };
    let n = be16(dir + 4)? as usize;
    let name = (0..n).find_map(|i| {
        let rec = dir + 12 + 16 * i;
        (data.get(rec..rec + 4)? == b"name").then(|| be32(rec + 8).map(|o| o as usize)).flatten()
    })?;
    let (count, strings) = (be16(name + 2)? as usize, name + be16(name + 4)? as usize);
    (0..count).find_map(|i| {
        let r = name + 6 + 12 * i;
        let (platform, id) = (be16(r)?, be16(r + 6)?);
        if id != MATCH_NAME_ID {
            return None;
        }
        let (len, off) = (be16(r + 8)? as usize, be16(r + 10)? as usize);
        let bytes = data.get(strings + off..strings + off + len)?;
        Some(if platform == 3 || platform == 0 {
            String::from_utf16_lossy(&bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>())
        } else {
            bytes.iter().map(|&b| b as char).collect()
        })
    })
}

/// Fonts built into every suite app: (PostScript name, file). (The matched stand-ins for
/// Apple fonts are in fonts_matched.rs, generated.)
const BUNDLED: [(&str, &[u8]); 20] = [
    ("Carlito", include_bytes!("../fonts/carlito/Carlito-Regular.ttf")),
    ("Carlito-Bold", include_bytes!("../fonts/carlito/Carlito-Bold.ttf")),
    ("Carlito-Italic", include_bytes!("../fonts/carlito/Carlito-Italic.ttf")),
    ("Carlito-BoldItalic", include_bytes!("../fonts/carlito/Carlito-BoldItalic.ttf")),
    ("Caladea-Regular", include_bytes!("../fonts/caladea/caladea-regular.ttf")),
    ("Caladea-Bold", include_bytes!("../fonts/caladea/caladea-bold.ttf")),
    ("Caladea-Italic", include_bytes!("../fonts/caladea/caladea-italic.ttf")),
    ("Caladea-BoldItalic", include_bytes!("../fonts/caladea/caladea-bolditalic.ttf")),
    ("LiberationSans", include_bytes!("../fonts/liberation/LiberationSans-Regular.ttf")),
    ("LiberationSans-Bold", include_bytes!("../fonts/liberation/LiberationSans-Bold.ttf")),
    ("LiberationSans-Italic", include_bytes!("../fonts/liberation/LiberationSans-Italic.ttf")),
    ("LiberationSans-BoldItalic", include_bytes!("../fonts/liberation/LiberationSans-BoldItalic.ttf")),
    ("LiberationSerif", include_bytes!("../fonts/liberation/LiberationSerif-Regular.ttf")),
    ("LiberationSerif-Bold", include_bytes!("../fonts/liberation/LiberationSerif-Bold.ttf")),
    ("LiberationSerif-Italic", include_bytes!("../fonts/liberation/LiberationSerif-Italic.ttf")),
    ("LiberationSerif-BoldItalic", include_bytes!("../fonts/liberation/LiberationSerif-BoldItalic.ttf")),
    ("LiberationMono", include_bytes!("../fonts/liberation/LiberationMono-Regular.ttf")),
    ("LiberationMono-Bold", include_bytes!("../fonts/liberation/LiberationMono-Bold.ttf")),
    ("LiberationMono-Italic", include_bytes!("../fonts/liberation/LiberationMono-Italic.ttf")),
    ("LiberationMono-BoldItalic", include_bytes!("../fonts/liberation/LiberationMono-BoldItalic.ttf")),
];

/// The Microsoft fonts people expect in a font menu, with the bundled metric copies that draw
/// them when they aren't installed: (family, PostScript name stored in documents, stand-in).
const EXPECTED: [(&str, &str, &str); 5] = [
    ("Arial", "ArialMT", "Liberation Sans"),
    ("Times New Roman", "TimesNewRomanPSMT", "Liberation Serif"),
    ("Courier New", "CourierNewPSMT", "Liberation Mono"),
    ("Calibri", "Calibri", "Carlito"),
    ("Cambria", "Cambria", "Caladea"),
];

/// OnlyOffice's stand-ins for fonts that aren't installed (its own metric copies first: what a
/// Word document is laid out with there).
fn word_substitutes(base: &str) -> &'static [&'static str] {
    word_stand_ins(base).unwrap_or(&["Liberation Sans", "Arimo"])
}

/// The stand-ins for a family Word documents name, if it is one OnlyOffice knows.
fn word_stand_ins(base: &str) -> Option<&'static [&'static str]> {
    let b = base.to_ascii_lowercase().replace([' ', '-'], "");
    Some(match b.as_str() {
        "helvetica" | "helveticaneue" | "arial" | "arialmt" | "sfprotext" | "sfprodisplay" | "systemfont" | "segoeui" | "tahoma" | "verdana" => &["Liberation Sans", "Arimo"],
        "timesnewroman" | "timesnewromanps" | "timesnewromanpsmt" | "times" | "timesroman" => &["Liberation Serif", "Tinos"],
        "couriernew" | "courier" | "couriernewps" | "couriernewpsmt" => &["Liberation Mono", "Cousine"],
        "calibri" => &["Carlito"],
        "cambria" => &["Caladea"],
        "arialnarrow" => &["Liberation Sans Narrow"],
        // serif and typewriter faces it doesn't have: its serif / mono; anything else its sans
        // (Century Gothic, Avenir, Helvetica Neue … all come out as Liberation Sans there)
        "georgia" | "baskerville" | "garamond" | "palatino" | "palatinolinotype" | "bookantiqua" | "didot" | "bodoni" | "hoefertext" | "bigcaslon"
        | "centuryschoolbook" | "constantia" | "bookmanoldstyle" | "minionpro" | "charter" | "iowanoldstyle" | "newyork" => &["Liberation Serif", "Tinos"],
        "menlo" | "monaco" | "consolas" | "lucidaconsole" | "sfmono" | "andalemono" => &["Liberation Mono", "Cousine"],
        _ => return None,
    })
}

/// A font file's OS/2 usWinAscent (plus Word's external leading) / usWinDescent per em: what
/// Word measures a line with.
fn win_metrics(data: &[u8], index: u32) -> Option<(f32, f32, f32)> {
    let be16 = |o: usize| data.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let be32 = |o: usize| data.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    // a collection: the face's own table directory
    let dir = if data.get(0..4) == Some(b"ttcf") { be32(12 + 4 * index as usize)? as usize } else { 0 };
    let n = be16(dir + 4)? as usize;
    let table = |tag: &[u8; 4]| (0..n).find_map(|i| {
        let rec = dir + 12 + 16 * i;
        (data.get(rec..rec + 4)? == tag).then(|| be32(rec + 8).map(|o| o as usize)).flatten()
    });
    let upem = be16(table(b"head")? + 18)? as f32;
    let os2 = table(b"OS/2")?;
    let (asc, desc) = (be16(os2 + 74)? as f32, be16(os2 + 76)? as f32);
    // the line gap left over once the win height covers hhea's ascender + descender goes on
    // top (Word: "external leading"; Arial / Liberation Sans 11 pt single-spaced = 12.65 pt)
    let hhea = table(b"hhea").and_then(|h| {
        let i16at = |o: usize| be16(o).map(|v| v as i16 as f32);
        Some((i16at(h + 4)?, i16at(h + 6)?, i16at(h + 8)?))
    });
    let lead = hhea.map(|(a, d, g)| (g - ((asc + desc) - (a - d))).max(0.0));
    if upem <= 0.0 {
        return None;
    }
    // a font whose win height is taller than hhea's ascender + descender + line gap (Poppins:
    // 1.76 em against 1.5): OnlyOffice measures the line by hhea (FreeType's), the gap on top
    if let Some((a, d, g)) = hhea.filter(|(a, d, g)| *a > 0.0 && asc + desc > a - d + g.max(0.0) + 1.0) {
        let g = g.max(0.0);
        return Some(((a + g) / upem, -d / upem, g / upem));
    }
    Some(((asc + lead.unwrap_or(0.0)) / upem, desc / upem, lead.unwrap_or(0.0) / upem))
}

/// Substitutes for Apple fonts, best first (metric-compatible ones where they exist).
fn substitutes(base: &str) -> &'static [&'static str] {
    let b = base.to_ascii_lowercase().replace([' ', '-'], "");
    match b.as_str() {
"arial" | "arialmt" => &["Liberation Sans", "Arimo", "Helvetica Neue", "Nimbus Sans", "DejaVu Sans"],
        "timesnewroman" | "timesnewromanps" | "timesnewromanpsmt" => &["Liberation Serif", "Tinos", "Nimbus Roman", "Georgia", "DejaVu Serif"],
        "couriernew" | "couriernewps" | "couriernewpsmt" => &["Liberation Mono", "Cousine", "Nimbus Mono PS", "DejaVu Sans Mono"],
        "helveticaneue" | "helvetica" | "sfprodisplay" | "sfprotext" | "sfpro" | ".sfnsdisplay" | ".sfnstext"
        | "systemfont" | "applesystemuifont" | "lucidagrande" | "geneva" | "avenir"
        | "avenirnext" | "gillsans" | "futura" | "verdana" | "tahoma" | "trebuchetms" | "optima" => {
            &["Helvetica Neue", "Helvetica", "Inter", "Nimbus Sans", "Liberation Sans", "Arimo", "DejaVu Sans", "Noto Sans"]
        }
"times" | "timesroman" | "georgia" | "baskerville"
        | "hoeferText" | "didot" | "garamond" | "bigcaslon" | "charter" | "iowanoldstyle" | "newyork" => {
            &["Georgia", "Gelasio", "Nimbus Roman", "Liberation Serif", "Tinos", "DejaVu Serif", "Noto Serif"]
        }
"courier" | "menlo" | "monaco" | "sfmono" | "andalemono" => {
            &["Menlo", "Nimbus Mono PS", "Liberation Mono", "Cousine", "DejaVu Sans Mono", "Noto Sans Mono"]
        }
        "markerfelt" | "chalkboard" | "chalkboardse" | "noteworthy" | "chalkduster" | "comicsansms" => {
            &["Comic Neue", "Comic Sans MS", "Noto Sans", "DejaVu Sans"]
        }
        // high-contrast and book serifs Apple ships (Canela, New York, Palatino, Century…)
        "canela" | "canelatext" | "canelacondensed" | "domaine" | "domainedisplay" => &["P052", "C059", "Georgia", "Nimbus Roman", "Liberation Serif", "DejaVu Serif", "Noto Serif"],
        "palatino" | "palatinolinotype" | "bookantiqua" => &["P052", "Georgia", "Nimbus Roman", "Liberation Serif", "Noto Serif"],
        "centuryschoolbook" | "century" | "newcenturyschlbk" => &["C059", "P052", "Nimbus Roman", "Liberation Serif"],
        // geometric sans (Century Gothic, Avant Garde, Futura-like)
        "centurygothic" | "itcavantgardegothic" | "avantgarde" | "avantgardegothic" => &["URW Gothic", "Poppins", "Montserrat", "Nimbus Sans", "Liberation Sans"],
        // condensed grotesques (DIN Condensed, Helvetica Condensed…)
        "dincondensed" | "helveticaneuecondensed" | "arialnarrow" | "dinalternate" => &["Nimbus Sans Narrow", "Liberation Sans Narrow", "Nimbus Sans", "Liberation Sans"],
        // Microsoft's Calibri / Cambria (Carlito / Caladea are metric copies)
        "calibri" => &["Carlito", "Lato", "Liberation Sans", "Nimbus Sans"],
        "cambria" => &["Caladea", "Georgia", "Liberation Serif", "Nimbus Roman"],
        "americantypewriter" | "courierprime" => &["Courier Prime", "Nimbus Mono PS", "Liberation Mono", "DejaVu Sans Mono"],
        // macOS's Graphik (Apple-only, a licence can't come to Linux): Lato is the closest free
        // stand-in (measured against Keynote's own slide pictures of a Graphik deck)
        "graphik" | "graphikcompact" => &["Lato", "Helvetica Neue", "Nimbus Sans", "Liberation Sans", "DejaVu Sans"],
        _ => &["Helvetica Neue", "Nimbus Sans", "Liberation Sans", "DejaVu Sans", "Noto Sans"],
    }
}

/// "HelveticaNeue-MediumItalic" -> ("Helvetica Neue", 500, true)
pub fn parse_ps_name(ps: &str) -> (String, u16, bool) {
    let (base, style) = ps.split_once('-').unwrap_or((ps, ""));
    let mut base = base.to_string();
    for suffix in ["PSMT", "MT", "PS"] {
        if base.len() > suffix.len() + 2 && base.ends_with(suffix) {
            base.truncate(base.len() - suffix.len());
            break;
        }
    }
    // CamelCase -> spaced ("HelveticaNeue" -> "Helvetica Neue")
    let mut family = String::new();
    let chars: Vec<char> = base.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() && chars[i - 1].is_lowercase() {
            family.push(' ');
        }
        family.push(*c);
    }
    let s = style.to_ascii_lowercase();
    let weight = if s.contains("ultralight") || s.contains("extralight") {
        200
    } else if s.contains("thin") || s.contains("hairline") {
        100
    } else if s.contains("light") {
        300
    } else if s.contains("medium") {
        500
    } else if s.contains("semibold") || s.contains("demibold") || s.contains("demi") {
        600
    } else if s.contains("extrabold") || s.contains("ultrabold") || s.contains("heavy") {
        800
    } else if s.contains("black") {
        900
    } else if s.contains("bold") {
        700
    } else {
        400
    };
    let italic = s.contains("italic") || s.contains("oblique") || base.ends_with("Italic");
    (family, weight, italic)
}

impl Fonts {
    pub fn new() -> Fonts {
        let mut fs = FontSystem::new();
        // the suite's bundled fonts (fonts/README.md): Word's Calibri / Cambria stand-ins, as
        // OnlyOffice lays Word documents out with them (an installed copy is used as well)
        for (name, data) in BUNDLED {
            if !fs.db().faces().any(|f| f.post_script_name == *name) {
                fs.db_mut().load_font_data(data.to_vec());
            }
        }
        // and the metric-matched stand-ins for Apple fonts (fonts/matched/README.md), read in
        // place from the program (no copy: they are 50 MB), unless installed already
        let installed: std::collections::HashSet<String> = fs.db().faces().map(|f| f.post_script_name.clone()).collect();
        for (name, data) in crate::fonts_matched::MATCHED {
            if !installed.contains(*name) {
                fs.db_mut().load_font_source(fontdb::Source::Binary(std::sync::Arc::new(*data)));
            }
        }
        let mut families: Vec<String> = fs
            .db()
            .faces()
            .flat_map(|f| f.families.iter().map(|(n, _)| n.clone()))
            .collect();
        families.sort();
        families.dedup();
        Fonts { fs, cache: SwashCache::new(), families, resolved: HashMap::new(), outlines: HashMap::new(), word: false, win: HashMap::new(), stand_ins: None }
    }

    pub fn families(&self) -> &[String] {
        &self.families
    }

    /// The families for a font menu: the installed ones, and Arial, Times New Roman, Courier
    /// New, Calibri and Cambria when they aren't (drawn with the bundled metric copies).
    pub fn menu_families(&self) -> Vec<String> {
        let mut out = self.families.clone();
        for (fam, _, _) in EXPECTED {
            if !self.has_family(fam) {
                out.push(fam.to_string());
            }
        }
        out.sort_by_key(|f| f.to_lowercase());
        out
    }

    /// PostScript name of a family's regular face (what Keynote stores as `font_name`).
    pub fn ps_name_for_family(&self, family: &str) -> Option<String> {
        // (Arial and the others when only their stand-ins are here: their own names, so the
        // document keeps asking for Arial)
        if !self.has_family(family) {
            if let Some((_, ps, _)) = EXPECTED.iter().find(|(f, _, _)| f.eq_ignore_ascii_case(family)) {
                return Some(ps.to_string());
            }
        }
        let faces: Vec<_> = self.fs.db().faces().filter(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(family))).collect();
        faces
            .iter()
            .min_by_key(|f| ((i32::from(f.weight.0) - 400).abs(), f.style != fontdb::Style::Normal))
            .map(|f| f.post_script_name.clone())
            .filter(|n| !n.is_empty())
    }

    /// Whether a Keynote font name (PostScript name) is installed, as itself or by family.
    pub fn is_installed(&self, ps_name: &str) -> bool {
        self.fs.db().faces().any(|f| f.post_script_name.eq_ignore_ascii_case(ps_name)) || self.has_family(&parse_ps_name(ps_name).0)
    }

    fn has_family(&self, name: &str) -> bool {
        self.families.iter().any(|f| f.eq_ignore_ascii_case(name))
    }

    fn installed_name(&self, name: &str) -> Option<String> {
        self.families.iter().find(|f| f.eq_ignore_ascii_case(name)).cloned()
    }

    /// Word mode: an installed family named like `name` with more after it (the shortest such),
    /// for a family Word's stand-ins don't cover.
    fn word_name_match(&self, name: &str) -> Option<String> {
        let n = name.to_ascii_lowercase().replace([' ', '-'], "");
        if n.len() < 4 || self.has_family(name) || word_stand_ins(name).is_some() {
            return None;
        }
        self.families.iter().filter(|f| f.to_ascii_lowercase().replace([' ', '-'], "").starts_with(&n)).min_by_key(|f| f.len()).cloned()
    }

    /// Resolve a Keynote font (PostScript name) plus bold/italic flags to an installed face.
    /// The installed face for a family / weight / style.
    fn face_id(&self, family: &str, weight: u16, italic: bool) -> Option<fontdb::ID> {
        let q = fontdb::Query { families: &[fontdb::Family::Name(family)], weight: fontdb::Weight(weight), style: if italic { fontdb::Style::Italic } else { fontdb::Style::Normal }, ..Default::default() };
        self.fs.db().query(&q)
    }

    /// The font file the page would draw `ps_name` (bold, italic) with, for a UI text field to
    /// type in the same face: a key naming the face, its bytes, the face index in the file, and
    /// whether the face itself is italic.
    pub fn face_data(&mut self, ps_name: &str, bold: bool, italic: bool) -> Option<(String, Vec<u8>, u32, bool)> {
        let c = self.choose(ps_name, bold, italic);
        let id = self.face_id(&c.family, c.weight, c.italic)?;
        let face_italic = self.fs.db().face(id).is_some_and(|f| f.style != fontdb::Style::Normal);
        let (bytes, index) = self.fs.db().with_face_data(id, |d, i| (d.to_vec(), i))?;
        Some((format!("{}|{}|{}", c.family, c.weight, face_italic), bytes, index, face_italic))
    }

    /// Whether a family has a glyph for `ch`.
    pub fn covers(&mut self, family: &str, weight: u16, italic: bool, ch: char) -> bool {
        let Some(id) = self.face_id(family, weight, italic) else { return false };
        self.fs.get_font(id, fontdb::Weight(weight)).is_some_and(|f| f.as_swash().charmap().map(ch) != 0)
    }

    /// A family's advance per em for `ch` (None: its missing glyph, .notdef).
    pub fn advance_em(&mut self, family: &str, weight: u16, italic: bool, ch: Option<char>) -> f32 {
        let Some(id) = self.face_id(family, weight, italic) else { return 0.5 };
        self.fs.get_font(id, fontdb::Weight(weight)).map(|f| {
            let s = f.as_swash();
            let upem = s.metrics(&[]).units_per_em.max(1) as f32;
            let g = ch.map(|c| s.charmap().map(c)).unwrap_or(0);
            s.glyph_metrics(&[]).advance_width(g) / upem
        }).unwrap_or(0.5)
    }

    /// A face's OS/2 win ascent and descent per em (Word's line height), when it has them.
    pub fn win_metrics(&mut self, id: fontdb::ID) -> Option<(f32, f32)> {
        self.win_metrics_lead(id).map(|(a, d, _)| (a, d))
    }

    /// The same with the external leading on its own (ascent includes it, per em).
    pub fn win_metrics_lead(&mut self, id: fontdb::ID) -> Option<(f32, f32, f32)> {
        if let Some(m) = self.win.get(&id) {
            return *m;
        }
        let m = self.fs.db().with_face_data(id, |data, index| win_metrics(data, index)).flatten();
        self.win.insert(id, m);
        m
    }

    /// A font's single line (ascent + descent per point of size, Word mode's win / hhea
    /// metrics) and its face, as a Word document draws `ps_name`.
    pub fn word_line(&mut self, ps_name: &str, bold: bool, italic: bool) -> Option<(f32, f32, fontdb::ID)> {
        let c = self.choose(ps_name, bold, italic);
        let id = self.face_id(&c.family, c.weight, c.italic)?;
        let (a, d) = self.win_metrics(id)?;
        Some((a, d, id))
    }

    pub fn choose(&mut self, ps_name: &str, bold: bool, italic: bool) -> FontChoice {
        let key = format!("{ps_name}|{bold}|{italic}|{}", self.word);
        if let Some(c) = self.resolved.get(&key) {
            return c.clone();
        }
        let (family, mut weight, it) = parse_ps_name(ps_name);
        if bold {
            weight = weight.max(700);
        }
        // KN_FONT_SUB="Graphik=Lato:800,…" (tests): try another stand-in for a missing font
        if let Some((sub, w)) = std::env::var("KN_FONT_SUB").ok().and_then(|v| {
            v.split(',').find_map(|e| {
                let (f, rest) = e.split_once('=')?;
                f.trim().eq_ignore_ascii_case(&family).then(|| {
                    let (n, w) = rest.split_once(':').unwrap_or((rest, ""));
                    (n.trim().to_string(), w.trim().parse::<u16>().ok())
                })
            })
        }) {
            if !self.is_installed(ps_name) && self.has_family(&sub) {
                let c = self.snap_to_faces(FontChoice { family: self.installed_name(&sub).unwrap_or(sub), weight: w.map(|w| if bold || weight >= 700 { w } else { weight }).unwrap_or(weight), italic: it || italic });
                self.resolved.insert(key, c.clone());
                return c;
            }
        }
        // an Apple font that isn't installed, in a Keynote / Pages document: its metric-matched
        // stand-in when one is installed (the same spacing, so lines and pages break as on a Mac)
        if !self.word && !self.is_installed(ps_name) {
            if let Some(c) = self.stand_in(ps_name, &family, weight, it || italic) {
                let c = self.snap_to_faces(c);
                self.resolved.insert(key, c.clone());
                return c;
            }
        }
        // an exact PostScript-name match wins (the user may have the real font installed)
        let exact = self
            .fs
            .db()
            .faces()
            .find(|f| f.post_script_name.eq_ignore_ascii_case(ps_name))
            .map(|f| (f.families[0].0.clone(), f.weight.0));
        let chosen = if let Some((fam, w)) = exact {
            FontChoice { family: fam, weight: if bold { w.max(700) } else { w }, italic: it || italic }
        } else {
            // (a Word document: a family it has no stand-in for drawn with an installed family
            // whose name starts with it, as OnlyOffice picks by name: "Futura" with "Futura
            // Display")
            let word = if self.word { self.word_name_match(&family).or_else(|| word_substitutes(&family).iter().find(|f| self.has_family(f)).and_then(|f| self.installed_name(f))) } else { None };
            let fam = if self.has_family(&family) {
                self.installed_name(&family).unwrap()
            } else if let Some(w) = word {
                w
            } else {
                substitutes(&family)
                    .iter()
                    .find(|f| self.has_family(f))
                    .and_then(|f| self.installed_name(f))
                    .unwrap_or_else(|| "sans-serif".into())
            };
            // (Graphik's bold is heavier than Lato's: its Heavy face matches it better)
            let weight = if fam == "Lato" && weight >= 700 && family.to_ascii_lowercase().starts_with("graphik") { 800 } else { weight };
            FontChoice { family: fam, weight, italic: it || italic }
        };
        let chosen = self.snap_to_faces(chosen);
        self.resolved.insert(key, chosen.clone());
        chosen
    }

    /// The installed stand-in for `ps_name` (of `family`, at `weight` / `italic` as asked):
    /// the one made from that very face, else the closest face standing in for its family
    /// (the condensed family for a condensed style).
    fn stand_in(&mut self, ps_name: &str, family: &str, weight: u16, italic: bool) -> Option<FontChoice> {
        if self.stand_ins.is_none() {
            let db = self.fs.db();
            let list = db
                .faces()
                .filter_map(|f| {
                    let name = db.with_face_data(f.id, |d, i| stand_in_name(d, i)).flatten()?;
                    let (fam, ps) = name.split_once('|')?;
                    Some(StandIn {
                        family: stand_in_family(fam),
                        ps: key(ps),
                        style: key(ps.split_once('-').map_or("", |s| s.1)),
                        choice: FontChoice { family: f.families.first()?.0.clone(), weight: f.weight.0, italic: f.style != fontdb::Style::Normal },
                    })
                })
                .collect();
            self.stand_ins = Some(list);
        }
        let list = self.stand_ins.as_ref()?;
        // (a bold request on a lighter face looks for a bold one in the family instead)
        let heavy_enough = |s: &StandIn| weight < 700 || s.choice.weight >= 600;
        let ps = key(ps_name);
        if let Some(s) = list.iter().find(|s| s.ps == ps && heavy_enough(s)) {
            return Some(FontChoice { italic: s.choice.italic || italic, ..s.choice.clone() });
        }
        let style = key(ps_name.split_once('-').map_or("", |s| s.1));
        let fam = stand_in_family(family);
        let condensed = format!("{fam}condensed");
        let pick = |want: &str| -> Option<&StandIn> {
            let in_family: Vec<&StandIn> = list.iter().filter(|s| s.family == want).collect();
            in_family
                .iter()
                .find(|s| !style.is_empty() && s.style == style && heavy_enough(s))
                .or_else(|| in_family.iter().min_by_key(|s| (s.choice.italic != italic, (i32::from(s.choice.weight) - i32::from(weight)).abs())))
                .copied()
        };
        let s = if style.contains("condensed") || style.starts_with("cn") { pick(&condensed).or_else(|| pick(&fam)) } else { pick(&fam) }?;
        Some(FontChoice { family: s.choice.family.clone(), weight: s.choice.weight, italic: s.choice.italic || italic })
    }

    /// Clamp weight/style to what the family actually has: asking for a bold face of a
    /// single-weight font would otherwise make the shaper fall back to a different family.
    fn snap_to_faces(&self, c: FontChoice) -> FontChoice {
        // a weight the family has only as a family of its own ("Lato Heavy", "Montserrat Black"
        // …): use that family
        let named = match c.weight {
            850.. => &["Black", "Heavy", "ExtraBold"][..],
            750..=849 => &["Heavy", "ExtraBold", "Black"][..],
            550..=649 => &["Semibold", "SemiBold", "DemiBold"][..],
            450..=549 => &["Medium"][..],
            250..=349 => &["Light"][..],
            ..=249 => &["Thin", "Hairline", "ExtraLight"][..],
            _ => &[][..],
        };
        let has_weight = self.fs.db().faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(&c.family)) && (i32::from(f.weight.0) - i32::from(c.weight)).abs() <= 50);
        if !has_weight {
            for w in named {
                let fam = format!("{} {w}", c.family);
                if let Some(name) = self.installed_name(&fam) {
                    return self.snap_weight(FontChoice { family: name, ..c });
                }
            }
        }
        self.snap_weight(c)
    }

    fn snap_weight(&self, mut c: FontChoice) -> FontChoice {
        let faces: Vec<(u16, bool)> = self
            .fs
            .db()
            .faces()
            .filter(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(&c.family)))
            .map(|f| (f.weight.0, f.style != fontdb::Style::Normal))
            .collect();
        if faces.is_empty() {
            return c;
        }
        if !faces.iter().any(|f| f.1 == c.italic) {
            c.italic = faces[0].1;
        }
        let best = faces
            .iter()
            .filter(|f| f.1 == c.italic)
            .min_by_key(|f| (i32::from(f.0) - i32::from(c.weight)).abs())
            .map(|f| f.0);
        if let Some(w) = best {
            c.weight = w;
        }
        c
    }

    /// Unhinted outline of a glyph at [`OUTLINE_SIZE`] px (y up, origin on the baseline);
    /// scale by `size / OUTLINE_SIZE`.
    pub fn outline(&mut self, font_id: fontdb::ID, glyph: u16, weight: fontdb::Weight) -> Option<&[swash::zeno::Command]> {
        let key = (font_id, glyph, weight.0);
        if !self.outlines.contains_key(&key) {
            let (ck, _, _) =
                CacheKey::new(font_id, glyph, OUTLINE_SIZE, (0.0, 0.0), weight, CacheKeyFlags::DISABLE_HINTING);
            let cmds = self.cache.get_outline_commands_uncached(&mut self.fs, ck).map(|b| b.into_vec());
            self.outlines.insert(key, cmds);
        }
        self.outlines.get(&key).and_then(|o| o.as_deref())
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts::new()
    }
}

#[cfg(test)]
mod bundled_tests {
    use super::*;

    #[test]
    fn word_documents_get_the_bundled_stand_ins() {
        let mut f = Fonts::new();
        assert!(f.families().iter().any(|n| n == "Carlito") && f.families().iter().any(|n| n == "Caladea"));
        f.word = true;
        assert_eq!(f.choose("Calibri", false, false).family, "Carlito");
        assert_eq!(f.choose("Calibri-Bold", true, false).family, "Carlito");
        assert_eq!(f.choose("Cambria", false, true).family, "Caladea");
        // Arial and co. in the font menu, drawn with the bundled Liberation copies
        let menu = f.menu_families();
        assert!(["Arial", "Times New Roman", "Courier New"].iter().all(|n| menu.iter().any(|m| m == n)), "{menu:?}");
        if !f.has_family("Arial") {
            assert_eq!(f.ps_name_for_family("Arial").as_deref(), Some("ArialMT"));
            assert_eq!(f.choose("ArialMT", false, false).family, "Liberation Sans");
        }
        if !f.has_family("Times New Roman") {
            assert_eq!(f.choose("TimesNewRomanPSMT", true, false).family, "Liberation Serif");
        }
    }
}

#[cfg(test)]
mod stand_in_tests {
    use super::*;

    #[test]
    fn document_names_reach_the_stand_in_families() {
        assert_eq!(stand_in_family(".SFNS"), "sfprotext");
        assert_eq!(stand_in_family("System Font"), "sfprotext");
        assert_eq!(stand_in_family("New York"), "newyorksmall");
        assert_eq!(stand_in_family("Avenir Next Condensed"), "avenirnextcondensed");
        assert_eq!(key("HelveticaNeue-CondensedBold"), "helveticaneuecondensedbold");
    }

    /// Apple fonts that aren't installed draw with the built-in stand-ins (skipped where the
    /// originals are installed: a Mac).
    #[test]
    fn missing_apple_fonts_draw_with_built_in_stand_ins() {
        let mut f = Fonts::new();
        assert!(f.has_family("Futurae") && f.has_family("Sfera") && f.has_family("New Yorkshire Small"));
        if f.is_installed("Futura-Medium") {
            return;
        }
        assert_eq!(f.choose("Futura-Medium", false, false).family, "Futurae");
        assert_eq!(f.choose("Futura-CondensedExtraBold", false, false).family, "Futurae Condensed");
        if f.has_family("Helvetia Neue") && !f.is_installed("HelveticaNeue") {
            let c = f.choose("HelveticaNeue-Bold", false, false);
            assert_eq!((c.family.as_str(), c.weight), ("Helvetia Neue", 700));
            assert!(f.choose("HelveticaNeue", false, true).italic);
            assert_eq!(f.choose("HelveticaNeue-CondensedBold", false, false).family, "Helvetia Neue Condensed");
        }
        if f.has_family("Avelon") && !f.is_installed("Avenir-Book") {
            let c = f.choose("Avenir-Book", false, false);
            assert_eq!(c.family, "Avelon");
            assert_eq!(c.weight, 350);
        }
        if !f.is_installed("Verdana") {
            assert_eq!(f.choose("Verdana-Bold", false, false).family, "Verdun");
        }
        if !f.is_installed("Monaco") {
            assert_eq!(f.choose("Monaco", false, false).family, "Monarch Mono");
        }
        if f.has_family("Sfera") && !f.is_installed(".SFNS-Regular") {
            assert_eq!(f.choose(".SFNS-Regular", false, false).family, "Sfera");
        }
        // (a Word document keeps OnlyOffice's stand-ins; Futura there takes a family named
        // like it, as OnlyOffice does, which can be Futurae)
        f.word = true;
        if !f.is_installed("HelveticaNeue") {
            assert_eq!(f.choose("HelveticaNeue", false, false).family, "Liberation Sans");
        }
    }
}
