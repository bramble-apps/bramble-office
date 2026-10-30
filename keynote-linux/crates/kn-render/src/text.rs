//! Text layout for Keynote text boxes: per-paragraph shaping with cosmic-text, then our own
//! paragraph stacking (space before/after, indents, list labels, vertical alignment, shrink-to-fit).

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight, Wrap};
use kn_model::{Align, Caps, CharStyle, Color, LineSpacing, ListLabel, Paragraph, TabAlign, TextBox, VAlign};

use crate::fonts::Fonts;

#[derive(Debug, Clone)]
pub struct GlyphPos {
    pub font_id: cosmic_text::fontdb::ID,
    pub glyph_id: u16,
    pub weight: Weight,
    pub size: f32,
    /// Baseline origin, frame-local points.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub color: Color,
    /// Byte range in TextBox::text.
    pub start: usize,
    pub end: usize,
    pub para: usize,
    /// Index into TextLayout::lines.
    pub line: usize,
    /// List label (bullet/number), not part of the text.
    pub label: bool,
}

#[derive(Debug, Clone)]
pub struct LineBox {
    pub para: usize,
    pub top: f32,
    pub baseline: f32,
    pub height: f32,
    pub x: f32,
    pub width: f32,
    /// Byte range in TextBox::text.
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct Deco {
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    pub thickness: f32,
    pub color: Color,
}

#[derive(Debug, Clone, Default)]
pub struct TextLayout {
    pub glyphs: Vec<GlyphPos>,
    pub lines: Vec<LineBox>,
    pub decos: Vec<Deco>,
    /// Background highlight rectangles (x, y, w, h, color).
    pub highlights: Vec<(f32, f32, f32, f32, Color)>,
    /// Review highlights (Insert > Highlight), like `highlights`; drawn only where review
    /// marks show (an app's editing canvas, not exports or a slideshow).
    pub review: Vec<(f32, f32, f32, f32, Color)>,
    /// Size of the laid out text (without padding).
    pub content_w: f32,
    pub content_h: f32,
    /// Font scale applied by shrink-to-fit (1.0 = none).
    pub scale: f32,
}

/// Natural line height as a multiple of the font size (Helvetica Neue-like metrics).
const NATURAL_LINE: f32 = 1.2;

fn line_height(spacing: LineSpacing, size: f32) -> f32 {
    let natural = size * NATURAL_LINE;
    match spacing {
        LineSpacing::Relative(r) => natural * r,
        LineSpacing::AtLeast(v) => natural.max(v),
        LineSpacing::Exactly(v) => v,
        LineSpacing::AtMost(v) => natural.min(v),
        LineSpacing::Between(v) => natural + v,
    }
}

fn apply_caps(s: &str, caps: Caps) -> String {
    match caps {
        Caps::All | Caps::Small => s.to_uppercase(),
        Caps::Start => {
            let mut out = String::with_capacity(s.len());
            let mut start = true;
            for c in s.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                start = c.is_whitespace();
            }
            out
        }
        Caps::Title => {
            // like Start Case, but short articles, conjunctions and prepositions stay lower case
            // (except as the first word)
            const MINOR: &[&str] = &["a", "an", "the", "and", "but", "or", "nor", "for", "so", "yet", "as", "at", "by", "in", "of", "off", "on", "per", "to", "up", "via", "vs"];
            let mut out = String::with_capacity(s.len());
            let mut first = true;
            for (i, word) in s.split(' ').enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                let bare: String = word.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
                if !first && MINOR.contains(&bare.as_str()) {
                    out.push_str(word);
                } else {
                    let mut done = false;
                    for c in word.chars() {
                        if !done && c.is_alphabetic() {
                            out.extend(c.to_uppercase());
                            done = true;
                        } else {
                            out.push(c);
                        }
                    }
                }
                if !word.is_empty() {
                    first = false;
                }
            }
            out
        }
        Caps::None => s.to_string(),
    }
}

struct Shaped {
    glyphs: Vec<GlyphPos>,
    lines: Vec<LineBox>,
    decos: Vec<Deco>,
    highlights: Vec<(f32, f32, f32, f32, Color)>,
    height: f32,
    width: f32,
}

fn attrs<'a>(fonts: &mut Fonts, cs: &CharStyle, scale: f32, spacing: LineSpacing, family: &'a mut String, meta: usize) -> Attrs<'a> {
    let choice = fonts.choose(&cs.font_name, cs.bold, cs.italic);
    *family = choice.family;
    let mut size = cs.size * scale;
    if fonts.word {
        // Word: small capitals at 0.8 of the size, super/subscripts at 0.65, both 0.52, each
        // rounded to a half point (OnlyOffice)
        let k = match (cs.script != 0, cs.caps == Caps::Small) {
            (true, true) => 0.52,
            (true, false) => 0.65,
            (false, true) => 0.8,
            _ => 1.0,
        };
        if k != 1.0 {
            size = (cs.size * k * 2.0 + 0.5).floor() / 2.0 * scale;
        }
    } else if cs.script != 0 || cs.caps == Caps::Small {
        size *= 0.7;
    }
    let lh = line_height(spacing, cs.size * scale);
    let mut a = Attrs::new()
        .family(Family::Name(family.as_str()))
        .weight(Weight(choice.weight))
        .style(if choice.italic { Style::Italic } else { Style::Normal })
        .metrics(Metrics::new(size.max(0.5), lh.max(0.5)))
        .metadata(meta);
    if cs.tracking != 0.0 {
        a = a.letter_spacing(cs.tracking);
    }
    // Word (and OnlyOffice) don't kern unless a run asks for it: the same widths, the same
    // line breaks
    if fonts.word {
        let mut f = cosmic_text::FontFeatures::new();
        f.disable(cosmic_text::FeatureTag::new(b"kern"));
        a = a.font_features(f);
    }
    a
}

/// A run's style for a span of characters its font lacks (Word documents): 1 DejaVu Sans, 2 a
/// blank (a no-break space widened to the font's missing-glyph width); 0 as it is.
fn span_style(fonts: &mut Fonts, style: &CharStyle, class: u8, c: &crate::fonts::FontChoice) -> CharStyle {
    let mut s = style.clone();
    match class {
        1 => s.font_name = "DejaVuSans".into(),
        2 => {
            let nbsp = fonts.advance_em(&c.family, c.weight, c.italic, Some('\u{00A0}'));
            s.tracking += (fonts.advance_em(&c.family, c.weight, c.italic, None) - nbsp).max(0.0);
        }
        _ => {}
    }
    s
}

/// Shape one paragraph into lines of width `width` (None = no wrapping).
/// Paragraph `pi` shaped and broken into lines at `width`. Its tabs go to their stops (when it
/// has a default stop interval): `origin` is where its lines start and `hang` its left indent
/// (an implicit stop), both from the text's left edge, at `scale`.
fn shape_paragraph(fonts: &mut Fonts, tb: &TextBox, pi: usize, width: Option<f32>, scale: f32, origin: f32, hang: f32) -> Shaped {
    let p = &tb.paragraphs[pi];
    let tab_at: Vec<usize> = tb.text[p.start..p.end].match_indices('\t').map(|(i, _)| p.start + i).collect();
    if p.default_tab <= 0.0 || tab_at.is_empty() {
        return shape_core(fonts, tb, pi, width, scale, None);
    }
    // each tab a no-break space as wide as its stop asks: right / centre / decimal stops depend
    // on the text after them, and widths can move line breaks, so shape until they settle
    let mut track = vec![0.0f32; tab_at.len()];
    let mut base: Vec<Option<f32>> = vec![None; tab_at.len()];
    let mut sh = shape_core(fonts, tb, pi, width, scale, Some(&track));
    for _ in 0..6 {
        let want = tab_widths(tb, p, &sh, &tab_at, width, scale, origin, hang);
        let mut changed = false;
        for (k, w) in want.iter().enumerate() {
            let Some((gw, fs, w)) = w.map(|w| (w.0, w.1, w.2)) else { continue };
            // the space's own advance (its width without the spacing added so far)
            let b = *base[k].get_or_insert(gw - track[k] * fs);
            let t = (w - b) / fs.max(0.01);
            if (t - track[k]).abs() * fs > 0.01 {
                track[k] = t;
                changed = true;
            }
        }
        if !changed {
            break;
        }
        sh = shape_core(fonts, tb, pi, width, scale, Some(&track));
    }
    sh
}

/// Each tab's (current glyph width, font size, wanted width) in a shaped paragraph, by Word's
/// rules (OnlyOffice's): the first custom stop after it, else the left indent, else the next
/// default stop (no further than the right edge); a right / centre / decimal stop puts the text
/// after the tab (to the next tab or the line's end) before / around / at it.
#[allow(clippy::too_many_arguments)]
fn tab_widths(tb: &TextBox, p: &Paragraph, sh: &Shaped, tab_at: &[usize], width: Option<f32>, scale: f32, origin: f32, hang: f32) -> Vec<Option<(f32, f32, f32)>> {
    let mut stops: Vec<(f32, TabAlign)> = p.tabs.iter().map(|t| (t.pos * scale, t.align)).collect();
    if !stops.iter().any(|s| (s.0 - hang).abs() < 0.01) {
        stops.push((hang, TabAlign::Left));
    }
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let def = p.default_tab * scale;
    let col_end = width.map(|w| origin + w);
    tab_at
        .iter()
        .map(|&at| {
            let gi = sh.glyphs.iter().position(|g| g.start == at)?;
            let g = &sh.glyphs[gi];
            let x = origin + g.x;
            let (mut stop, mut align) = match stops.iter().find(|s| s.0 > x + 0.05) {
                Some(s) => (s.0, s.1),
                None if x < hang - 0.05 => (hang, TabAlign::Left),
                None => {
                    let mut n = 0.0f32;
                    while x >= n - 0.05 {
                        n += def.max(1.0);
                    }
                    // (a default stop past the right edge stops at it)
                    if let Some(e) = col_end.filter(|e| x < *e && n >= *e && p.right_indent <= 0.0) {
                        n = e;
                    }
                    (n, TabAlign::Left)
                }
            };
            // a right / centre / decimal stop past the edge is a right stop at it (Word 2013+)
            if let Some(e) = col_end.filter(|e| align != TabAlign::Left && stop > *e) {
                stop = e;
                align = TabAlign::Right;
            }
            // the text after the tab on its line, to the next tab
            let line = g.line;
            let seg: Vec<&GlyphPos> = sh.glyphs[gi + 1..].iter().take_while(|h| h.line == line && !tb.text.get(h.start..h.end).is_some_and(|t| t.contains('\t'))).collect();
            let seg_w = |upto_decimal: bool| -> f32 {
                let start = g.x + g.w;
                let end = seg.iter().take_while(|h| !(upto_decimal && tb.text.get(h.start..h.end).is_some_and(|t| t.contains(['.', ',']))) ).map(|h| h.x + h.w).fold(start, f32::max);
                end - start
            };
            let w = match align {
                TabAlign::Left => stop - x,
                TabAlign::Right => (stop - x - seg_w(false)).max(0.0),
                TabAlign::Center => (stop - x - seg_w(false) / 2.0).max(0.0),
                TabAlign::Decimal => (stop - x - seg_w(true)).max(0.0),
            };
            Some((g.w, g.size, w))
        })
        .collect()
}

fn shape_core(fonts: &mut Fonts, tb: &TextBox, pi: usize, width: Option<f32>, scale: f32, tab_track: Option<&[f32]>) -> Shaped {
    let p = &tb.paragraphs[pi];
    let mut tab_k = 0usize;
    let spacing = p.line_spacing;
    let first_style = p.runs.first().map(|r| r.style.clone()).unwrap_or_default();

    // Build spans: caps applied and U+2028 (Keynote's line break) turned into '\n' so the
    // shaper breaks there. `map` takes a byte in the concatenated shaped text to the source byte.
    let mut span_texts: Vec<String> = Vec::new();
    let mut map: Vec<usize> = Vec::new();
    let word = fonts.word;
    let mut span_styles: Vec<CharStyle> = Vec::new();
    for r in &p.runs {
        if r.end <= r.start {
            continue;
        }
        let mut out = String::new();
        // a Word document: characters its font lacks are drawn as OnlyOffice draws them, from
        // DejaVu Sans, else as a blank the width of the font's missing glyph (0 normal, 1 DejaVu,
        // 2 blank)
        let choice = word.then(|| fonts.choose(&r.style.font_name, r.style.bold, r.style.italic));
        let mut class = 0u8;
        // the run's style for the span being built (Word's small caps: a span per case)
        let mut rs = r.style.clone();
        for (bi, ch) in tb.text[r.start..r.end].char_indices() {
            let src = r.start + bi;
            // Word's small caps: only lower-case letters are drawn as smaller capitals, the rest
            // at full size (OnlyOffice)
            if word && r.style.caps == Caps::Small {
                let caps = if ch.to_uppercase().next() != Some(ch) { Caps::Small } else { Caps::All };
                if caps != rs.caps {
                    if !out.is_empty() {
                        span_texts.push(std::mem::take(&mut out));
                        span_styles.push(match choice.as_ref() {
                            Some(c) => span_style(fonts, &rs, class, c),
                            None => rs.clone(),
                        });
                    }
                    rs.caps = caps;
                }
            }
            if let Some(c) = choice.as_ref() {
                // (Latin and the layout's own marks: the font's)
                let k = if (ch as u32) < 0x250 || matches!(ch, '\u{2028}' | '\u{2029}' | '\u{200B}' | '\u{2060}' | '\u{00A0}') || fonts.covers(&c.family, c.weight, c.italic, ch) {
                    0
                } else if fonts.covers("DejaVu Sans", if c.weight >= 600 { 700 } else { 400 }, false, ch) {
                    1
                } else {
                    2
                };
                if k != 0 || class != 0 {
                    // a span of its own
                    if !out.is_empty() {
                        span_texts.push(std::mem::take(&mut out));
                        span_styles.push(span_style(fonts, &rs, class, c));
                    }
                    class = k;
                    if k == 2 {
                        out.push('\u{00A0}');
                        for _ in 0..'\u{00A0}'.len_utf8() {
                            map.push(src);
                        }
                        continue;
                    }
                }
            }
            // a Word document's inline object (its stand-in, a figure space): it breaks like a
            // word of its own, as Word lets a picture go to the next line: zero-width spaces
            // around it, in spans of their own (not the stand-in's size and spacing)
            if word && ch == '\u{2007}' {
                if !out.is_empty() {
                    span_texts.push(std::mem::take(&mut out));
                    span_styles.push(match choice.as_ref() {
                        Some(c) => span_style(fonts, &rs, class, c),
                        None => rs.clone(),
                    });
                }
                let mut tiny = rs.clone();
                tiny.size = 1.0;
                tiny.tracking = 0.0;
                // (the break after it only where text follows: after an object wider than the
                // line it would start an empty line of its own; a word joiner instead keeps the
                // stand-in, a figure space, from hanging past the edge like a trailing space)
                let more = tb.text[src + ch.len_utf8()..p.end].chars().any(|c| c != '\u{2028}');
                // an object wider than the line overhangs it (OnlyOffice): its stand-in no wider
                let mut stand = rs.clone();
                if let Some(w) = width {
                    let em = (stand.size * scale).max(0.01);
                    stand.tracking = stand.tracking.min((w - 0.5) / em - 0.56).max(0.0);
                }
                for (piece, st) in [("\u{200B}", tiny.clone()), ("\u{2007}", stand), (if more { "\u{200B}" } else { "\u{2060}" }, tiny)] {
                    span_texts.push(piece.into());
                    span_styles.push(st);
                    for _ in 0..piece.len() {
                        map.push(src);
                    }
                }
                continue;
            }
            // a tab going to its stop: a no-break space in a span of its own, spaced to the
            // stop's width
            if let (Some(tt), '\t') = (tab_track, ch) {
                if !out.is_empty() {
                    span_texts.push(std::mem::take(&mut out));
                    span_styles.push(match choice.as_ref() {
                        Some(c) => span_style(fonts, &rs, class, c),
                        None => rs.clone(),
                    });
                }
                let mut ts = rs.clone();
                // (in ems of the font: what the shaper's letter spacing takes)
                ts.tracking = tt.get(tab_k).copied().unwrap_or(0.0);
                ts.underline = false;
                ts.strike = false;
                tab_k += 1;
                span_texts.push("\u{00A0}".into());
                span_styles.push(ts);
                for _ in 0..'\u{00A0}'.len_utf8() {
                    map.push(src);
                }
                continue;
            }
            // line break, and column break (U+000C; one column: a new line)
            let piece: String = if ch == '\u{2028}' || ch == '\u{000C}' {
                "\n".into()
            } else if ch == '/' && word {
                // Word breaks lines only at spaces and hyphens, not after a slash
                // ("Lagos/Portugal" stays whole): a word joiner holds it to what follows
                "/\u{2060}".into()
            } else if ch == '\u{2013}' && word {
                // nor after an en dash ("Luke 10:1–9", "pages 3–5" stay whole in OnlyOffice)
                "\u{2013}\u{2060}".into()
            } else if ch == ' ' && word && !tb.text[..src].ends_with(' ') && tb.text[src..p.end].trim_start_matches(' ').chars().next().is_some_and(|n| !n.is_alphanumeric() && !n.is_whitespace()) {
                // and at any space, also before "/", "?", ";", ")", "[" … (the line-break rules
                // keep those with the word before): a zero-width space before the spaces lets
                // the line break after them (they stay the line's last, free to hang past the
                // edge; before the first of several, so none counts as part of the word before)
                "\u{200B} ".into()
            } else {
                apply_caps(ch.encode_utf8(&mut [0u8; 4]), rs.caps)
            };
            for _ in 0..piece.len() {
                map.push(src);
            }
            out.push_str(&piece);
        }
        if !out.is_empty() {
            span_texts.push(out);
            span_styles.push(match choice.as_ref() {
                Some(c) => span_style(fonts, &rs, class, c),
                None => rs.clone(),
            });
        }
    }
    // a line break at the end starts one more (empty) line, as in Pages and Word: give that
    // line something to hold (a zero-width space) or the shaper drops it
    if let Some(last) = span_texts.last_mut().filter(|t| t.ends_with('\n')) {
        // (a Word paragraph's mark, a zero-length last run, sizes that line: a span of its own)
        match p.runs.last().filter(|r| word && r.start == r.end && r.start == p.end && p.end > p.start) {
            Some(r) => {
                span_texts.push("\u{200B}".into());
                span_styles.push(r.style.clone());
            }
            None => last.push('\u{200B}'),
        }
        for _ in 0..'\u{200B}'.len_utf8() {
            map.push(p.end);
        }
    }
    let empty = span_texts.is_empty();
    if empty {
        span_texts.push(String::new());
    }
    map.push(p.end);
    let mut fams: Vec<String> = vec![String::new(); span_texts.len()];
    let styles: Vec<&CharStyle> = if empty { vec![&first_style] } else { span_styles.iter().collect() };

    let default_size = first_style.size * scale;
    let mut buf = Buffer::new(&mut fonts.fs, Metrics::new(default_size.max(0.5), line_height(spacing, default_size).max(0.5)));
    buf.set_wrap(if width.is_some() { Wrap::WordOrGlyph } else { Wrap::None });
    // (a Word document: OnlyOffice measures glyphs about 0.03 % narrower than their exact
    // advances, so a line that just fits there fits here too)
    let room = if word { 1.0003 } else { 1.0 };
    buf.set_size(width.map(|w| (w * room).max(1.0)), None);
    let align = match p.align {
        Align::Left => cosmic_text::Align::Left,
        Align::Right => cosmic_text::Align::Right,
        Align::Center => cosmic_text::Align::Center,
        Align::Justify => cosmic_text::Align::Justified,
        Align::Natural => {
            if p.rtl {
                cosmic_text::Align::Right
            } else {
                cosmic_text::Align::Left
            }
        }
    };
    {
        // Attrs borrow the family strings, so build them in a separate scope.
        let mut list = Vec::with_capacity(span_texts.len());
        let mut fam_iter = fams.iter_mut();
        for (i, t) in span_texts.iter().enumerate() {
            let fam = fam_iter.next().unwrap();
            let a = attrs(fonts, styles[i], scale, spacing, fam, i);
            list.push((t.as_str(), a));
        }
        let mut dfam = String::new();
        let default = attrs(fonts, &first_style, scale, spacing, &mut dfam, 0);
        buf.set_rich_text(list, &default, Shaping::Advanced, Some(align));
    }
    buf.shape_until_scroll(&mut fonts.fs, false);

    // start of each shaped line (the shaper splits on '\n'); glyph offsets are line-relative
    let joined: String = span_texts.concat();
    let mut line_starts = vec![0usize];
    for (i, ch) in joined.char_indices() {
        if ch == '\n' {
            line_starts.push(i + 1);
        }
    }
    let to_src = |line: usize, b: usize| -> usize {
        let g = line_starts.get(line).copied().unwrap_or(0) + b;
        map.get(g).copied().unwrap_or(p.end)
    };
    let mut out = Shaped { glyphs: vec![], lines: vec![], decos: vec![], highlights: vec![], height: 0.0, width: 0.0 };
    for run in buf.layout_runs() {
        let mut lstart = usize::MAX;
        let mut lend = 0;
        // this line's highlights, and whether any covers more than spaces
        let hl_from = out.highlights.len();
        let mut hl_ink = false;
        for g in run.glyphs {
            let st = styles[g.metadata.min(styles.len() - 1)];
            let mut y = run.line_y;
            if st.script == 1 {
                y -= st.size * scale * 0.33;
            } else if st.script == -1 {
                y += st.size * scale * 0.15;
            }
            y -= st.baseline_shift * scale;
            let start = to_src(run.line_i, g.start);
            let end = to_src(run.line_i, g.end);
            lstart = lstart.min(start);
            lend = lend.max(end);
            out.glyphs.push(GlyphPos {
                font_id: g.font_id,
                glyph_id: g.glyph_id,
                weight: g.font_weight,
                size: g.font_size,
                x: g.x + g.font_size * g.x_offset,
                y: y - g.font_size * g.y_offset,
                w: g.w,
                color: st.color,
                start,
                end,
                para: pi,
                line: out.lines.len(),
                label: false,
            });
            if st.underline {
                out.decos.push(Deco { x0: g.x, x1: g.x + g.w, y: run.line_y + g.font_size * 0.1, thickness: (g.font_size / 16.0).max(1.0), color: st.color });
            }
            if st.strike {
                out.decos.push(Deco { x0: g.x, x1: g.x + g.w, y: run.line_y - g.font_size * 0.3, thickness: (g.font_size / 16.0).max(1.0), color: st.color });
            }
            if let Some(bg) = st.background {
                // Keynote's highlight covers the font's ascent and descent around the baseline,
                // whatever the line spacing (a 0.8 line would otherwise clip it)
                let (asc, desc, _) = vmetrics(fonts, &st.font_name, Some((g.font_id, g.font_weight)));
                out.highlights.push((g.x, run.line_y - asc * g.font_size, g.w, (asc + desc) * g.font_size, bg));
                hl_ink |= !run.text.get(g.start..g.end).unwrap_or("").trim().is_empty();
            }
        }
        // Keynote draws no band on a line whose only highlighted characters are spaces (the
        // highlighted space ending "DISCIPLESHIP " in R39)
        if !hl_ink {
            out.highlights.truncate(hl_from);
        }
        if lstart == usize::MAX {
            // empty line: it starts where its shaped line starts
            lstart = to_src(run.line_i, 0);
            lend = lstart;
        }
        let x = run.glyphs.first().map(|g| g.x).unwrap_or(match align {
            cosmic_text::Align::Center => width.unwrap_or(0.0) / 2.0,
            cosmic_text::Align::Right | cosmic_text::Align::End => width.unwrap_or(0.0),
            _ => 0.0,
        });
        out.lines.push(LineBox { para: pi, top: run.line_top, baseline: run.line_y, height: run.line_height, x, width: run.line_w, start: lstart, end: lend });
        out.width = out.width.max(run.line_w);
        out.height = out.height.max(run.line_top + run.line_height);
    }
    if out.lines.is_empty() {
        let h = line_height(spacing, default_size);
        out.lines.push(LineBox { para: pi, top: 0.0, baseline: h * 0.8, height: h, x: 0.0, width: 0.0, start: p.start, end: p.start });
        out.height = h;
    }
    out
}

/// A paragraph whose first line starts elsewhere than the others (Format > Indents > First, a
/// Word first-line or hanging indent; measured from the left edge, as the left indent is): the
/// first line laid out at its own width, the rest from where it ends. Offsets are relative to
/// `indent` (the other lines' start), as the caller adds that. None: nothing to do.
fn first_line_indented(fonts: &mut Fonts, tb: &TextBox, pi: usize, width: Option<f32>, indent: f32, scale: f32) -> Option<Shaped> {
    let p = &tb.paragraphs[pi];
    // a labelled paragraph: its first line's own text position, if it has one
    let first_x = match (&p.label, p.first_text_indent) {
        (Some(_), Some(t)) => (p.left_indent + p.list_indent + t) * scale,
        (Some(_), None) => return None,
        (None, _) => p.first_line_indent * scale,
    };
    let other_x = if p.label.is_some() { indent } else { p.left_indent * scale };
    if (first_x - other_x).abs() < 0.01 || p.end <= p.start {
        return None;
    }
    let right = p.right_indent * scale;
    let mut sh = shape_paragraph(fonts, tb, pi, width.map(|w| (w - first_x - right).max(1.0)), scale, first_x, indent);
    let shift = first_x - indent;
    // (the next line starts after the spaces it broke at)
    let mut cut = sh.lines.first()?.end;
    while cut < p.end && tb.text[cut..].starts_with([' ', '\t']) {
        cut += 1;
    }
    let h0 = sh.lines[0].top + sh.lines[0].height;
    sh.glyphs.retain(|g| g.line == 0);
    sh.lines.truncate(1);
    for g in &mut sh.glyphs {
        g.x += shift;
    }
    sh.lines[0].x += shift;
    sh.decos.retain(|d| d.y <= h0);
    for d in &mut sh.decos {
        d.x0 += shift;
        d.x1 += shift;
    }
    sh.highlights.retain(|h| h.1 < h0);
    for h in &mut sh.highlights {
        h.0 += shift;
    }
    sh.width += shift.max(0.0);
    if cut >= p.end {
        return Some(sh);
    }
    // the rest of the paragraph at the other lines' width
    let mut rest_tb = tb.clone();
    let q = &mut rest_tb.paragraphs[pi];
    q.start = cut;
    q.runs = q.runs.iter().filter(|r| r.end > cut).map(|r| kn_model::Run { start: r.start.max(cut), end: r.end, style: r.style.clone() }).collect();
    let rest = shape_paragraph(fonts, &rest_tb, pi, width.map(|w| (w - indent - right).max(1.0)), scale, indent, indent);
    for mut g in rest.glyphs {
        g.y += h0;
        g.line += 1;
        sh.glyphs.push(g);
    }
    for mut l in rest.lines {
        l.top += h0;
        l.baseline += h0;
        sh.lines.push(l);
    }
    for mut d in rest.decos {
        d.y += h0;
        sh.decos.push(d);
    }
    for mut h in rest.highlights {
        h.1 += h0;
        sh.highlights.push(h);
    }
    sh.height = h0 + rest.height;
    sh.width = sh.width.max(rest.width);
    Some(sh)
}

fn layout_at_scale(fonts: &mut Fonts, tb: &TextBox, width: Option<f32>, scale: f32) -> TextLayout {
    let mut tl = TextLayout { scale, ..Default::default() };
    let mut y = 0.0f32;
    for pi in 0..tb.paragraphs.len() {
        let p = &tb.paragraphs[pi];
        let has_label = p.label.is_some();
        let em = p.runs.first().map(|r| r.style.size).unwrap_or(24.0);
        let indent = (p.left_indent + if has_label { p.list_indent + p.label_indent * em } else { 0.0 }) * scale;
        let avail = width.map(|w| (w - indent - p.right_indent * scale).max(1.0));
        if pi > 0 {
            y += p.space_before * scale;
        }
        let sh = match first_line_indented(fonts, tb, pi, width, indent, scale) {
            Some(sh) => sh,
            None => shape_paragraph(fonts, tb, pi, avail, scale, indent, indent),
        };
        let line_base = tl.lines.len();
        for mut g in sh.glyphs {
            g.x += indent;
            g.y += y;
            g.line += line_base;
            tl.glyphs.push(g);
        }
        for mut d in sh.decos {
            d.x0 += indent;
            d.x1 += indent;
            d.y += y;
            tl.decos.push(d);
        }
        for (hx, hy, hw, hh, c) in sh.highlights {
            tl.highlights.push((hx + indent, hy + y, hw, hh, c));
        }
        // list label on the first line
        let has_text = !tb.text[p.start..p.end].trim().is_empty();
        if let (Some(label), Some(first), true) = (&p.label, sh.lines.first(), has_text) {
            let cs = p.runs.first().map(|r| r.style.clone()).unwrap_or_default();
            let text = match label {
                ListLabel::Bullet(s) | ListLabel::Number(s) => s.clone(),
                ListLabel::Image(_) => "•".into(),
            };
            let mut ls = cs.clone();
            ls.size = cs.size * p.label_scale.max(0.1);
            if let Some(c) = p.label_color {
                ls.color = c;
            }
            let lx = (p.left_indent + p.list_indent) * scale;
            let mut label_tb = TextBox {
                storage: 0,
        highlights: Vec::new(),
        fields: Vec::new(),
                text: text.clone(),
                paragraphs: vec![p.clone()],
                valign: VAlign::Top,
                padding: [0.0; 4],
                shrink_to_fit: false,
                columns: 1,
                column_gap: 0.0,
                autogrow: true,
            };
            label_tb.paragraphs[0].start = 0;
            label_tb.paragraphs[0].end = text.len();
            label_tb.paragraphs[0].label = None;
            label_tb.paragraphs[0].align = Align::Left;
            label_tb.paragraphs[0].runs = vec![kn_model::Run { start: 0, end: text.len(), style: ls }];
            let lsh = shape_paragraph(fonts, &label_tb, 0, None, scale, 0.0, 0.0);
            let dy = first.baseline - lsh.lines.first().map(|l| l.baseline).unwrap_or(0.0);
            for mut g in lsh.glyphs {
                g.x += lx;
                g.y += y + dy;
                g.start = p.start;
                g.end = p.start;
                g.para = pi;
                g.label = true;
                g.line = tl.lines.len();
                tl.glyphs.push(g);
            }
        }
        for mut l in sh.lines {
            l.x += indent;
            l.top += y;
            l.baseline += y;
            tl.lines.push(l);
        }
        tl.content_w = tl.content_w.max(sh.width + indent + p.right_indent * scale);
        y += sh.height;
        if pi + 1 < tb.paragraphs.len() {
            y += p.space_after * scale;
        }
    }
    tl.content_h = y;
    keynote_lines(fonts, tb, &mut tl, scale);
    tl
}

/// Vertical metrics per em (ascent, descent, line gap). Helvetica Neue (Keynote's default
/// face) uses its own figures, since it is usually drawn here with a stand-in of other proportions.
/// A font's ascent, descent and line gap per point of size, as the text layout uses them (for
/// apps that lay paragraphs out one by one and space them as continuous text, as Letters does).
pub fn font_vmetrics(fonts: &mut Fonts, font_name: &str, face: Option<(cosmic_text::fontdb::ID, Weight)>) -> (f32, f32, f32) {
    vmetrics(fonts, font_name, face)
}

fn vmetrics(fonts: &mut Fonts, font_name: &str, face: Option<(cosmic_text::fontdb::ID, Weight)>) -> (f32, f32, f32) {
    const HELVETICA_NEUE: (f32, f32, f32) = (0.952, 0.213, 0.027);
    // a Word document: the face actually drawn, by its OS/2 win height (Word's single line)
    if fonts.word {
        if let Some((a, d)) = face.and_then(|(id, _)| fonts.win_metrics(id)) {
            return (a, d, 0.0);
        }
    }
    if font_name.replace(' ', "").starts_with("HelveticaNeue") {
        return HELVETICA_NEUE;
    }
    // a font that isn't installed: Keynote / Pages laid the text out with the real one; a
    // stand-in's own metrics (URW's ascent is only cap height) would squash lines and
    // highlights. The Mac fonts documents use most, as their lines measure in Pages' renders
    // (ascent + descent = the line pitch at single spacing): Avenir / Avenir Next 1.37 em,
    // Century Gothic 1.23, Helvetica / Arial 1.2, Calibri 1.22, Times / Baskerville / Futura
    // 1.15-1.17; anything else, Helvetica Neue's
    if !font_name.is_empty() && !fonts.is_installed(font_name) {
        let n = font_name.replace(' ', "");
        let known: &[(&str, (f32, f32, f32))] = &[
            ("Avenir", (1.0, 0.366, 0.0)),
            ("CenturyGothic", (1.006, 0.22, 0.0)),
            ("Helvetica", (0.95, 0.25, 0.0)),
            ("Arial", (0.95, 0.25, 0.0)),
            ("Calibri", (0.97, 0.25, 0.0)),
            ("Cambria", (0.95, 0.222, 0.0)),
            ("TimesNewRoman", (0.92, 0.23, 0.0)),
            ("Times", (0.92, 0.23, 0.0)),
            ("Baskerville", (0.93, 0.24, 0.0)),
            ("Futura", (0.92, 0.23, 0.0)),
            ("Georgia", (0.917, 0.219, 0.0)),
            ("Verdana", (1.005, 0.21, 0.0)),
            ("Menlo", (0.928, 0.236, 0.0)),
            ("GillSans", (0.92, 0.23, 0.0)),
            ("Optima", (0.93, 0.26, 0.0)),
            ("Palatino", (0.94, 0.27, 0.0)),
        ];
        return known.iter().find(|(k, _)| n.starts_with(k)).map(|(_, m)| *m).unwrap_or(HELVETICA_NEUE);
    }
    let Some(m) = face.and_then(|(id, w)| fonts.fs.get_font(id, w)).map(|f| f.as_swash().metrics(&[])) else { return HELVETICA_NEUE };
    let em = m.units_per_em.max(1) as f32;
    (m.ascent / em, m.descent.abs() / em, m.leading.max(0.0) / em)
}

/// Place lines the way Keynote does (measured from its PDFs, R28): the first line's baseline
/// sits ascent + line gap below the top whatever the line spacing; each next one sits the
/// previous line's descent, the paragraph spacing and `spacing × ascent` lower; the text ends
/// one descent below the last baseline. (A single line is therefore always 1.19 em tall,
/// which is what Shrink Text to Fit measures.)
fn keynote_lines(fonts: &mut Fonts, tb: &TextBox, tl: &mut TextLayout, scale: f32) {
    let n = tl.lines.len();
    if n == 0 {
        return;
    }
    if fonts.word {
        return word_lines(fonts, tb, tl, scale);
    }
    // each line's biggest glyph decides its metrics
    let mut big: Vec<(f32, Option<(cosmic_text::fontdb::ID, Weight)>)> = vec![(0.0, None); n];
    for g in tl.glyphs.iter().filter(|g| !g.label && g.line < n) {
        if g.size > big[g.line].0 {
            big[g.line] = (g.size, Some((g.font_id, g.weight)));
        }
    }
    let mut shift = vec![0.0f32; n];
    let mut prev: Option<(f32, f32, usize)> = None;
    let mut last_desc = 0.0;
    for i in 0..n {
        let pi = tl.lines[i].para.min(tb.paragraphs.len().saturating_sub(1));
        let Some(p) = tb.paragraphs.get(pi) else { return };
        let first = p.runs.first().map(|r| &r.style);
        // (at least the paragraph's size: small caps draw smaller letters, but the line is spaced
        // for the font's size)
        let nominal = first.map(|s| s.size).unwrap_or(24.0) * scale;
        let small_caps = first.is_some_and(|s| s.caps == Caps::Small);
        let size = if big[i].0 > 0.0 { if small_caps { big[i].0.max(nominal) } else { big[i].0 } } else { nominal };
        let (asc, desc, gap) = vmetrics(fonts, first.map(|s| s.font_name.as_str()).unwrap_or(""), big[i].1);
        let (asc, desc, gap) = (asc * size, desc * size, gap * size);
        let base = match prev {
            None => gap + asc,
            Some((pb, pd, pp)) => {
                let between = if pp != pi { (tb.paragraphs[pp].space_after + p.space_before) * scale } else { 0.0 };
                // (at least / at most / between measure from the font's own height, as Pages)
                let natural = asc + desc + gap;
                let advance = match p.line_spacing {
                    LineSpacing::Relative(r) => r * asc,
                    LineSpacing::AtLeast(v) => (natural.max(v) - pd).max(0.0),
                    LineSpacing::AtMost(v) => (natural.min(v) - pd).max(0.0),
                    LineSpacing::Between(v) => (natural + v - pd).max(0.0),
                    other => (line_height(other, size) - pd).max(0.0),
                };
                pb + pd + between + advance
            }
        };
        shift[i] = base - tl.lines[i].baseline;
        prev = Some((base, desc, pi));
        last_desc = desc;
    }
    // decorations and highlights move with the line they were in
    let line_of = |y: f32| tl.lines.iter().position(|l| y >= l.top - 0.5 && y <= l.top + l.height + 0.5);
    for d in &mut tl.decos {
        if let Some(i) = line_of(d.y) {
            d.y += shift[i];
        }
    }
    let hl: Vec<Option<usize>> = tl.highlights.iter().map(|h| line_of(h.1 + h.3 / 2.0)).collect();
    for (h, i) in tl.highlights.iter_mut().zip(hl) {
        if let Some(i) = i {
            h.1 += shift[i];
        }
    }
    for g in &mut tl.glyphs {
        if let Some(s) = shift.get(g.line) {
            g.y += s;
        }
    }
    for (l, s) in tl.lines.iter_mut().zip(&shift) {
        l.top += s;
        l.baseline += s;
    }
    if let Some(l) = tl.lines.last() {
        tl.content_h = l.baseline + last_desc;
    }
}

/// Place lines as Word does: each line is the font's single height (OS/2 win ascent + descent)
/// times the spacing multiple (or the exact / at-least height), its baseline one ascent below
/// its top (the extra of a multiple goes below the text); paragraph spacing between.
fn word_lines(fonts: &mut Fonts, tb: &TextBox, tl: &mut TextLayout, scale: f32) {
    let n = tl.lines.len();
    let mut big: Vec<(f32, Option<(cosmic_text::fontdb::ID, Weight)>)> = vec![(0.0, None); n];
    // an inline object's stand-in (a figure space as tall as the object): the line is as tall
    // as the object (it sits on the baseline), not a letter of its size
    let is_object = |g: &GlyphPos| tb.text.get(g.start..).is_some_and(|t| t.starts_with('\u{2007}'));
    let mut objects = vec![0.0f32; n];
    for g in tl.glyphs.iter().filter(|g| !g.label && g.line < n && is_object(g)) {
        objects[g.line] = objects[g.line].max(g.size);
    }
    for g in tl.glyphs.iter().filter(|g| !g.label && g.line < n && !is_object(g)) {
        if g.size > big[g.line].0 {
            big[g.line] = (g.size, Some((g.font_id, g.weight)));
        }
    }
    let mut shift = vec![0.0f32; n];
    let mut boxes = vec![(0.0f32, 0.0f32); n];
    // (Word keeps the space before the first paragraph and after the last: a table cell or text
    // box is that much taller)
    let mut top = tb.paragraphs.get(tl.lines[0].para).map(|p| p.space_before * scale).unwrap_or(0.0);
    let mut prev_para: Option<usize> = None;
    for i in 0..n {
        let pi = tl.lines[i].para.min(tb.paragraphs.len().saturating_sub(1));
        let Some(p) = tb.paragraphs.get(pi) else { return };
        let first = p.runs.first().map(|r| &r.style);
        let nominal = first.map(|s| s.size).unwrap_or(12.0) * scale;
        let size = if big[i].0 > 0.0 { big[i].0 } else { nominal };
        // an empty line: its font is the paragraph's
        let face = big[i].1.or_else(|| {
            let s = first?;
            let c = fonts.choose(&s.font_name, s.bold, s.italic);
            let q = cosmic_text::fontdb::Query { families: &[cosmic_text::fontdb::Family::Name(&c.family)], weight: Weight(c.weight), style: if c.italic { cosmic_text::fontdb::Style::Italic } else { cosmic_text::fontdb::Style::Normal }, ..Default::default() };
            fonts.fs.db().query(&q).map(|id| (id, Weight(c.weight)))
        });
        let (asc, desc, _) = vmetrics(fonts, first.map(|s| s.font_name.as_str()).unwrap_or(""), face);
        let (mut asc, mut desc) = (asc * size, desc * size);
        // a list label taller than the text raises the line by its ascent (its font's win
        // ascent without the external leading); its descent doesn't count (OnlyOffice)
        for g in tl.glyphs.iter().filter(|g| g.label && g.line == i) {
            if let Some((a, _, lead)) = fonts.win_metrics_lead(g.font_id) {
                asc = asc.max((a - lead) * g.size);
            }
        }
        // (an object taller than the text: the line is its height and the text's descent)
        let object = objects[i];
        if object > asc {
            asc = object;
        }
        // (a line holding only an object: no text, no descent)
        if object > 0.0 && big[i].0 == 0.0 {
            desc = 0.0;
        }
        let natural = asc + desc;
        // (between two paragraphs Word keeps the larger of the space after and before)
        if let Some(pp) = prev_para.filter(|pp| *pp != pi) {
            top += tb.paragraphs[pp].space_after.max(p.space_before) * scale;
        }
        let h = match p.line_spacing {
            LineSpacing::Relative(_) if object > 0.0 && object >= asc => natural,
            LineSpacing::Relative(r) => natural * r,
            LineSpacing::AtLeast(v) => natural.max(v * scale),
            LineSpacing::Exactly(v) => v * scale,
            LineSpacing::AtMost(v) => natural.min(v * scale),
            LineSpacing::Between(v) => natural + v * scale,
        };
        // a multiple's extra space goes below the text (OnlyOffice / Word); a fixed height
        // holds the text at its bottom
        let base = match p.line_spacing {
            LineSpacing::Relative(_) | LineSpacing::AtLeast(_) | LineSpacing::Between(_) => top + asc,
            _ => top + h - desc,
        };
        shift[i] = base - tl.lines[i].baseline;
        boxes[i] = (top, h);
        top += h;
        prev_para = Some(pi);
    }
    for d in &mut tl.decos {
        if let Some(i) = tl.lines.iter().position(|l| d.y >= l.top - 0.5 && d.y <= l.top + l.height + 0.5) {
            d.y += shift[i];
        }
    }
    let hl: Vec<Option<usize>> = tl.highlights.iter().map(|h| tl.lines.iter().position(|l| h.1 + h.3 / 2.0 >= l.top - 0.5 && h.1 + h.3 / 2.0 <= l.top + l.height + 0.5)).collect();
    for (h, i) in tl.highlights.iter_mut().zip(hl) {
        if let Some(i) = i {
            h.1 += shift[i];
        }
    }
    for g in &mut tl.glyphs {
        if let Some(s) = shift.get(g.line) {
            g.y += s;
        }
    }
    for ((l, s), b) in tl.lines.iter_mut().zip(&shift).zip(&boxes) {
        l.baseline += s;
        (l.top, l.height) = *b;
    }
    let last = tl.lines.last().map(|l| l.para).and_then(|i| tb.paragraphs.get(i)).map(|p| p.space_after * scale).unwrap_or(0.0);
    tl.content_h = top + last;
}

/// Whether a line ends inside a word (the word was wider than the box).
fn breaks_word(tb: &TextBox, tl: &TextLayout) -> bool {
    tl.lines.windows(2).any(|w| {
        w[0].para == w[1].para && {
            let before = tb.text.get(..w[1].start).and_then(|t| t.chars().next_back());
            let at = tb.text.get(w[1].start..).and_then(|t| t.chars().next());
            matches!((before, at), (Some(b), Some(a)) if !b.is_whitespace() && !a.is_whitespace() && b.is_alphanumeric() && a.is_alphanumeric())
        }
    })
}

/// Lay out a text box inside a frame of `frame_w` x `frame_h` points. When `auto_width` is set the
/// text is not wrapped. Positions are frame-local (padding and vertical alignment applied).
pub fn layout(fonts: &mut Fonts, tb: &TextBox, frame_w: f32, frame_h: f32, auto_width: bool, auto_height: bool) -> TextLayout {
    let [pl, pt, pr, pb] = tb.padding;
    let width = if auto_width { None } else { Some((frame_w - pl - pr).max(1.0)) };
    let mut tl = layout_at_scale(fonts, tb, width, 1.0);
    let avail_h = frame_h - pt - pb;
    // Shrink Text to Fit: smaller until the text fits the height and no word is broken across
    // lines (Keynote shrank "DISCIPLESHIP" until it fitted the box's width, R39)
    if tb.shrink_to_fit && !auto_height && (tl.content_h > avail_h || breaks_word(tb, &tl)) && avail_h > 0.0 {
        let (mut lo, mut hi) = (0.05f32, 1.0f32);
        for _ in 0..10 {
            let mid = (lo + hi) / 2.0;
            let t = layout_at_scale(fonts, tb, width, mid);
            if t.content_h <= avail_h && !breaks_word(tb, &t) {
                lo = mid;
                tl = t;
            } else {
                hi = mid;
            }
        }
        if tl.scale == 1.0 {
            tl = layout_at_scale(fonts, tb, width, lo);
        }
    }
    let box_h = if auto_height { tl.content_h } else { avail_h };
    let dy = pt
        + match tb.valign {
            VAlign::Top => 0.0,
            VAlign::Middle => (box_h - tl.content_h) / 2.0,
            VAlign::Bottom => box_h - tl.content_h,
        };
    // unwrapped text in an auto-width box: alignment is relative to the content width
    let dx = pl;
    for g in &mut tl.glyphs {
        g.x += dx;
        g.y += dy;
    }
    for l in &mut tl.lines {
        l.x += dx;
        l.top += dy;
        l.baseline += dy;
    }
    for d in &mut tl.decos {
        d.x0 += dx;
        d.x1 += dx;
        d.y += dy;
    }
    for h in &mut tl.highlights {
        h.0 += dx;
        h.1 += dy;
    }
    // review highlights (Insert > Highlight: TSWP.HighlightArchive, in the author's colour):
    // a band behind the highlighted characters, kept apart from the text's own highlights
    for &(a, b, color) in &tb.highlights {
        let bands: Vec<(f32, f32, f32, f32, Color)> = tl
            .glyphs
            .iter()
            .filter(|g| !g.label && g.end > g.start && g.start >= a && g.end <= b)
            .map(|g| {
                let (asc, desc, _) = vmetrics(fonts, "", Some((g.font_id, g.weight)));
                (g.x, g.y - asc * g.size, g.w, (asc + desc) * g.size, color)
            })
            .collect();
        tl.review.extend(bands);
    }
    tl
}

/// Caret rectangle (x, top, height) for byte index `idx` of the text, frame-local.
pub fn caret(tl: &TextLayout, idx: usize) -> Option<(f32, f32, f32)> {
    // the last line that starts at or before idx (a line break position belongs to the next line)
    let li = tl.lines.iter().rposition(|l| l.start <= idx).unwrap_or(0);
    let line = tl.lines.get(li)?;
    let glyphs: Vec<&GlyphPos> = tl.glyphs.iter().filter(|g| g.line == li && !g.label).collect();
    let x = if let Some(g) = glyphs.iter().find(|g| g.start <= idx && idx < g.end.max(g.start + 1) && g.start == idx) {
        g.x
    } else if let Some(g) = glyphs.iter().filter(|g| g.end <= idx).max_by(|a, b| a.end.cmp(&b.end).then(a.x.total_cmp(&b.x))) {
        g.x + g.w
    } else if let Some(g) = glyphs.first() {
        g.x
    } else {
        line.x
    };
    Some((x, line.top, line.height))
}

/// Byte index of the text position nearest to frame-local point (x, y).
pub fn hit_index(tl: &TextLayout, x: f32, y: f32) -> usize {
    let Some((li, line)) = tl
        .lines
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let da = if y < a.1.top { a.1.top - y } else if y > a.1.top + a.1.height { y - a.1.top - a.1.height } else { 0.0 };
            let db = if y < b.1.top { b.1.top - y } else if y > b.1.top + b.1.height { y - b.1.top - b.1.height } else { 0.0 };
            da.total_cmp(&db)
        })
    else {
        return 0;
    };
    let mut best = line.start;
    let mut best_d = f32::INFINITY;
    for g in tl.glyphs.iter().filter(|g| g.line == li && !g.label) {
        for (pos, gx) in [(g.start, g.x), (g.end, g.x + g.w)] {
            let d = (gx - x).abs();
            if d < best_d {
                best_d = d;
                best = pos;
            }
        }
    }
    best
}

#[cfg(test)]
mod review_highlight_tests {
    use super::*;

    /// A review highlight over "big" gives bands behind just those glyphs.
    #[test]
    fn review_highlights_are_laid_out() {
        let mut fonts = Fonts::new();
        let mut tb = crate::simple_text_box("a big word", kn_model::CharStyle { size: 20.0, ..Default::default() }, kn_model::Align::Left, kn_model::VAlign::Top);
        let yellow = Color { r: 1.0, g: 0.9, b: 0.3, a: 1.0 };
        tb.highlights = vec![(2, 5, yellow)];
        let tl = layout(&mut fonts, &tb, 300.0, 100.0, false, false);
        assert_eq!(tl.review.len(), 3);
        assert!(tl.highlights.is_empty());
        let plain = layout(&mut fonts, &crate::simple_text_box("a big word", kn_model::CharStyle { size: 20.0, ..Default::default() }, kn_model::Align::Left, kn_model::VAlign::Top), 300.0, 100.0, false, false);
        assert!(plain.review.is_empty());
    }

    /// The band is painted only with the renderer's review marks on.
    #[test]
    fn review_highlights_draw_only_when_on() {
        let mut r = crate::Renderer::new();
        let yellow = Color { r: 1.0, g: 0.9, b: 0.0, a: 1.0 };
        let mut tl = TextLayout::default();
        tl.review.push((2.0, 2.0, 16.0, 16.0, yellow));
        let px = |r: &mut crate::Renderer| {
            let mut pm = tiny_skia::Pixmap::new(20, 20).unwrap();
            r.draw_text(&tl, &mut pm, tiny_skia::Transform::identity());
            pm.pixel(10, 10).map(|p| (p.red(), p.green(), p.blue(), p.alpha())).unwrap()
        };
        assert_eq!(px(&mut r).3, 0, "off: nothing painted");
        r.review_marks = true;
        let (red, green, blue, alpha) = px(&mut r);
        assert!(alpha > 200 && red > 200 && green > 180 && blue < 60, "on: the yellow band");
    }
}
