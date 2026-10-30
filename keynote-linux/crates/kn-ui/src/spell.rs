//! Spell checking with the system Hunspell dictionaries (English + French by default, since a
//! word is accepted if any loaded dictionary knows it). Loaded on a background thread. Shared
//! by Keydeck and Numdeck; each app names its own learned-words file ([`set_words_file`]).

use std::sync::{Arc, OnceLock};

pub use spellbook::Dictionary;

static DICTS: OnceLock<Arc<Vec<Dictionary>>> = OnceLock::new();
static STARTED: std::sync::Once = std::sync::Once::new();

/// Words the user taught (Learn Spelling, kept in the app's words file) and words
/// ignored for this session (Ignore Spelling).
static KNOWN: std::sync::Mutex<Option<std::collections::HashSet<String>>> = std::sync::Mutex::new(None);
static IGNORED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

static WORDS_FILE: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Where Learn Spelling keeps the app's words (set once, at the start; later calls are ignored).
pub fn set_words_file(path: std::path::PathBuf) {
    let _ = WORDS_FILE.set(path);
}

fn words_path() -> Option<std::path::PathBuf> {
    WORDS_FILE.get().cloned()
}

fn known(word: &str) -> bool {
    if IGNORED.lock().map(|v| v.iter().any(|w| w == word)).unwrap_or(false) {
        return true;
    }
    let mut k = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    let set = k.get_or_insert_with(|| words_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect());
    set.contains(word)
}

/// Learn Spelling: never flag this word again.
pub fn learn(word: &str) {
    let _ = known(word);
    if let Ok(mut k) = KNOWN.lock() {
        if let Some(set) = k.as_mut() {
            set.insert(word.to_string());
            if let Some(p) = words_path() {
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let mut all: Vec<&String> = set.iter().collect();
                all.sort();
                let _ = std::fs::write(p, all.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"));
            }
        }
    }
}

/// Ignore Spelling: accept the word until the app quits.
pub fn ignore(word: &str) {
    if let Ok(mut v) = IGNORED.lock() {
        v.push(word.to_string());
    }
}

/// Grammar (Check Grammar With Spelling): repeated words ("the the") and a lower-case letter
/// starting a sentence. Byte ranges.
pub fn grammar(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut prev: Option<(usize, usize)> = None;
    let mut start = None;
    let mut words = Vec::new();
    for (i, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        match (word_char(c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                words.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    for &(s, e) in &words {
        if let Some((ps, pe)) = prev {
            let gap = &text[pe..s];
            if gap.chars().all(|c| c == ' ') && text[ps..pe].eq_ignore_ascii_case(&text[s..e]) && text[s..e].chars().any(|c| c.is_alphabetic()) {
                out.push((s, e));
            }
            // a sentence that starts in lower case
            let t = gap.trim_end_matches(' ');
            if (t.ends_with('.') || t.ends_with('!') || t.ends_with('?')) && gap.ends_with(' ') && text[s..e].chars().next().is_some_and(|c| c.is_lowercase()) {
                let prev_word = &text[ps..pe];
                // not after abbreviations like "e.g." or "etc."
                if prev_word.len() > 3 && !["etc"].contains(&prev_word) {
                    out.push((s, e));
                }
            }
        }
        prev = Some((s, e));
    }
    out
}

const DIRS: &[&str] = &["/usr/share/hunspell", "/usr/share/myspell/dicts", "/usr/share/myspell"];

fn languages() -> Vec<String> {
    let mut langs: Vec<String> = Vec::new();
    if let Ok(l) = std::env::var("LANG") {
        if let Some(code) = l.split('.').next() {
            langs.push(code.to_string());
        }
    }
    for l in ["en_US", "en_CA", "fr_CA", "fr_FR"] {
        if !langs.iter().any(|x| x == l) {
            langs.push(l.to_string());
        }
    }
    langs
}

fn load() -> Vec<Dictionary> {
    let mut out = Vec::new();
    let mut seen_lang = Vec::new();
    for lang in languages() {
        let base = lang.split('_').next().unwrap_or("").to_string();
        if seen_lang.contains(&base) {
            continue; // one dictionary per language is enough
        }
        for dir in DIRS {
            let (aff, dic) = (format!("{dir}/{lang}.aff"), format!("{dir}/{lang}.dic"));
            if let (Ok(a), Ok(d)) = (std::fs::read_to_string(&aff), std::fs::read_to_string(&dic)) {
                if let Ok(dict) = Dictionary::new(&a, &d) {
                    out.push(dict);
                    seen_lang.push(base.clone());
                    break;
                }
            }
        }
    }
    out
}

/// Dictionaries if loaded (starts loading on first call; never blocks).
pub fn dicts() -> Option<Arc<Vec<Dictionary>>> {
    STARTED.call_once(|| {
        std::thread::spawn(|| {
            let d = std::panic::catch_unwind(load).unwrap_or_default();
            let _ = DICTS.set(Arc::new(d));
        });
    });
    DICTS.get().cloned()
}

fn word_char(c: char) -> bool {
    c.is_alphabetic() || c == '\'' || c == '’'
}

/// Byte ranges of misspelled words in `text`.
pub fn misspelled(text: &str) -> Vec<(usize, usize)> {
    let Some(d) = dicts() else { return Vec::new() };
    if d.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = None;
    let mut push = |s: usize, e: usize| {
        let w = text[s..e].trim_matches(|c| c == '\'' || c == '’');
        if w.chars().count() < 2 || w.chars().all(|c| c.is_uppercase()) {
            return; // skip single letters and acronyms
        }
        let w2 = w.replace('’', "'");
        if !known(w) && !d.iter().any(|dict| dict.check(&w2)) {
            out.push((s, e));
        }
    };
    for (i, c) in text.char_indices() {
        match (word_char(c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                // words glued to digits (e.g. "3rd") are not checked
                if !text[s..i].chars().any(|c| c.is_ascii_digit()) && !c.is_ascii_digit() {
                    push(s, i);
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        push(s, text.len());
    }
    out
}

pub fn suggestions(word: &str) -> Vec<String> {
    let Some(d) = dicts() else { return Vec::new() };
    let mut out = Vec::new();
    for dict in d.iter() {
        let mut v = Vec::new();
        dict.suggest(word, &mut v);
        for s in v {
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out.truncate(8);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_finds_doubled_words_and_lowercase_sentences() {
        let t = "This is the the plan. then we go.";
        let g = grammar(t);
        assert!(g.contains(&(12, 15)));
        assert!(g.iter().any(|&(s, _)| &t[s..s + 4] == "then"));
        assert!(grammar("It is fine. Really.").is_empty());
    }

    #[test]
    fn english_and_french_words() {
        // wait for the background load (only in tests)
        for _ in 0..200 {
            if dicts().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if dicts().is_some_and(|d| d.is_empty()) {
            return; // no dictionaries installed on this machine
        }
        let bad = misspelled("Comment faire la phase théorique? How to lecture phaze.");
        let words: Vec<&str> = bad.iter().map(|(s, e)| &"Comment faire la phase théorique? How to lecture phaze."[*s..*e]).collect();
        assert_eq!(words, vec!["phaze"]);
        assert!(suggestions("phaze").iter().any(|s| s == "phase"));
    }
}
