//! Spell checking: kn-ui's (shared with Numdeck), with Keydeck's own learned words
//! (~/.config/keydeck/words.txt, as before).

use std::sync::Arc;

pub(crate) fn ensure() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")));
        if let Some(b) = base {
            kn_ui::spell::set_words_file(b.join("keydeck").join("words.txt"));
        }
    });
}

pub fn learn(word: &str) {
    ensure();
    kn_ui::spell::learn(word);
}

pub fn ignore(word: &str) {
    ensure();
    kn_ui::spell::ignore(word);
}

pub fn grammar(text: &str) -> Vec<(usize, usize)> {
    kn_ui::spell::grammar(text)
}

pub fn dicts() -> Option<Arc<Vec<kn_ui::spell::Dictionary>>> {
    kn_ui::spell::dicts()
}

pub fn misspelled(text: &str) -> Vec<(usize, usize)> {
    ensure();
    kn_ui::spell::misspelled(text)
}

pub fn suggestions(word: &str) -> Vec<String> {
    kn_ui::spell::suggestions(word)
}
