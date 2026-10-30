//! Typing substitutions shared by Keydeck and Numdeck: smart quotes and dashes, text
//! replacements (Edit > Substitutions) and Correct Spelling Automatically.

/// What typing `t` after `before` becomes: (characters to take back, text to insert). Smart
/// quotes curl by what comes before; "--" becomes an em dash.
pub fn smart_text(before: Option<char>, t: &str, no_selection: bool, quotes: bool, dashes: bool) -> (usize, String) {
    if dashes && t == "-" && before == Some('-') && no_selection {
        return (1, "\u{2014}".into());
    }
    if quotes && (t == "\"" || t == "'") {
        let opening = before.is_none_or(|c| c.is_whitespace() || "([{\u{2014}\u{2013}\u{2028}".contains(c));
        let q = match (t, opening) {
            ("\"", true) => "\u{201C}",
            ("\"", false) => "\u{201D}",
            (_, true) => "\u{2018}",
            _ => "\u{2019}",
        };
        return (0, q.into());
    }
    (0, t.replace('\r', "\n"))
}

/// Edit > Substitutions > Text Replacement (the common macOS defaults).
pub const REPLACEMENTS: &[(&str, &str)] = &[
    ("(c)", "\u{a9}"),
    ("(r)", "\u{ae}"),
    ("(tm)", "\u{2122}"),
    ("1/2", "\u{bd}"),
    ("1/3", "\u{2153}"),
    ("2/3", "\u{2154}"),
    ("1/4", "\u{bc}"),
    ("3/4", "\u{be}"),
    ("->", "\u{2192}"),
    ("<-", "\u{2190}"),
    ("=>", "\u{21d2}"),
    ("...", "\u{2026}"),
    ("omw", "On my way!"),
];

/// The word before the caret (`before` ends at the caret) and what it should become:
/// a text replacement, or (Correct Spelling Automatically) the first spelling suggestion.
pub fn word_fix(before: &str, text_replacement: bool, autocorrect: bool) -> Option<(usize, String)> {
    let start = before.rfind(|c: char| c.is_whitespace()).map(|i| i + before[i..].chars().next().map_or(1, |c| c.len_utf8())).unwrap_or(0);
    let word = &before[start..];
    if word.is_empty() {
        return None;
    }
    if text_replacement {
        if let Some((_, to)) = REPLACEMENTS.iter().find(|(from, _)| *from == word) {
            return Some((start, to.to_string()));
        }
    }
    if autocorrect && word.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '\u{2019}') {
        if crate::spell::misspelled(word).is_empty() {
            return None;
        }
        let first = crate::spell::suggestions(word).into_iter().next()?;
        // keep a capital first letter
        let fixed = if word.chars().next().is_some_and(|c| c.is_uppercase()) {
            let mut c = first.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or(first)
        } else {
            first
        };
        return Some((start, fixed));
    }
    None
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_quotes_and_dashes() {
        assert_eq!(smart_text(None, "\"", true, true, true), (0, "\u{201C}".into()));
        assert_eq!(smart_text(Some('i'), "\"", true, true, true), (0, "\u{201D}".into()));
        assert_eq!(smart_text(Some('n'), "'", true, true, true), (0, "\u{2019}".into()));
        assert_eq!(smart_text(Some(' '), "'", true, true, true), (0, "\u{2018}".into()));
        assert_eq!(smart_text(Some('-'), "-", true, true, true), (1, "\u{2014}".into()));
        assert_eq!(smart_text(Some('-'), "-", true, true, false), (0, "-".into()));
        assert_eq!(smart_text(Some('a'), "\"", true, false, true), (0, "\"".into()));
    }

    /// Keydeck's typing loop (textedit::handle_keys) over these functions: each character's
    /// smart text, and at a word's end the replacement / autocorrection.
    fn type_text(typed: &str, quotes: bool, dashes: bool, repl: bool, autocorrect: bool) -> String {
        let mut text = String::new();
        for ch in typed.chars() {
            let t = ch.to_string();
            let a = text.len();
            let (back, ins) = smart_text(text.chars().last(), &t, true, quotes, dashes);
            let ends_word = back == 0 && t.chars().all(|c| c.is_whitespace() || ".,;:!?)".contains(c));
            match ends_word.then(|| word_fix(&text[..a], repl, autocorrect)).flatten() {
                Some((start, fixed)) => text.replace_range(start..a, &(fixed + &ins)),
                None => {
                    let from = text[..a].char_indices().rev().nth(back.saturating_sub(1)).map(|(i, _)| if back == 0 { a } else { i }).unwrap_or(a);
                    text.replace_range(from..a, &ins);
                }
            }
        }
        text
    }

    #[test]
    fn typing_quotes_dash_and_autocorrect() {
        // wait for the dictionaries (only in tests)
        for _ in 0..200 {
            if crate::spell::dicts().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(type_text("\"hello\" -- go ", true, true, true, false), "\u{201C}hello\u{201D} \u{2014} go ");
        if crate::spell::dicts().is_some_and(|d| !d.is_empty()) {
            assert_eq!(type_text("\"hello\" -- teh ", true, true, true, true), "\u{201C}hello\u{201D} \u{2014} the ");
        }
    }

    #[test]
    fn replacements() {
        assert_eq!(word_fix("see (c)", true, false), Some((4, "\u{a9}".into())));
        assert_eq!(word_fix("go ->", true, false), Some((3, "\u{2192}".into())));
        assert_eq!(word_fix("see (c)", false, false), None);
        assert_eq!(word_fix("hello", true, false), None);
    }
}
