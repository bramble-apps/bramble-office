//! Everything one running calculator holds: the keypad, settings, history, Math Notes and
//! exchange rates, saved as JSON in the app's data folder. The desktop app uses this directly;
//! the Android app reaches it through api.rs / calc-ffi.

use crate::calc::{plain_text, segments, Calc, Display, Seg};
use crate::format::{format_value, plain, NumberStyle};
use crate::history::{self, ConvInfo, History};
use crate::layout::{self, Insets, Layout, LayoutIn, Mode};
use crate::notes::Notes;
use crate::theme::{self, Theme};
use crate::units::{self, Rates, Unit};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    pub style: NumberStyle,
    pub mode: Mode,
    pub convert: bool,
    pub conv_cat: String,
    pub conv_from: String,
    pub conv_to: String,
    pub sidebar_open: bool,
    pub haptics: bool,
    pub sounds: bool,
    /// set once the Braun look has become the theme of a calculator still on the old default
    /// (missing from older settings files, so false there)
    #[serde(default)]
    pub braun_default: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: "braun".into(),
            style: NumberStyle::Comma,
            mode: Mode::Basic,
            convert: false,
            conv_cat: "currency".into(),
            conv_from: "USD".into(),
            conv_to: "EUR".into(),
            sidebar_open: true,
            haptics: true,
            sounds: false,
            braun_default: true,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ConvState {
    pub cat: String,
    pub cat_name: String,
    pub from: Unit,
    pub to: Unit,
    /// the converted value for the lower row
    pub out: String,
    pub out_value: f64,
    pub rates_updated: i64,
    pub rates_source: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct HistRow {
    pub id: u64,
    pub expr: Vec<Seg>,
    pub result: String,
    pub date: String,
    /// "562 USD" / "4,519.6 MOP" lines for conversions
    pub conv: Option<(String, String)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HistGroup {
    pub title: String,
    pub rows: Vec<HistRow>,
}

pub struct Session {
    dir: Option<PathBuf>,
    pub calc: Calc,
    pub settings: Settings,
    pub history: History,
    pub notes: Notes,
    pub rates: Rates,
    theme: Theme,
    size: (f32, f32, Insets),
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn read<T: for<'de> Deserialize<'de> + Default>(p: &Path) -> T {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn write(p: &Path, s: &str) {
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, s).is_ok() {
        let _ = std::fs::rename(&tmp, p);
    }
}

impl Session {
    /// `dir`: where to keep settings, history and notes (None keeps nothing, for tests).
    pub fn new(dir: Option<PathBuf>) -> Session {
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d);
        }
        let mut settings: Settings = dir.as_ref().map(|d| read(&d.join("settings.json"))).unwrap_or_default();
        // Braun became the default after iOS 26: move calculators still on iOS 26 over once
        if !settings.braun_default {
            if settings.theme == "ios26" {
                settings.theme = "braun".into();
            }
            settings.braun_default = true;
        }
        let mut calc: Calc = dir.as_ref().map(|d| read(&d.join("calc.json"))).unwrap_or_default();
        calc.max_digits = 9;
        let history = dir.as_ref().map(|d| History::load(&d.join("history.json"))).unwrap_or_default();
        let notes = dir.as_ref().map(|d| Notes::load(&d.join("notes.json"))).unwrap_or_default();
        let rates = dir
            .as_ref()
            .and_then(|d| std::fs::read_to_string(d.join("rates.json")).ok())
            .and_then(|s| Rates::from_json(&s))
            .filter(|r| r.updated >= Rates::builtin().updated)
            .unwrap_or_else(Rates::builtin);
        let theme = theme::theme(&settings.theme);
        Session { dir, calc, settings, history, notes, rates, theme, size: (393.0, 852.0, Insets::default()) }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(name))
    }

    pub fn save_settings(&self) {
        if let Some(p) = self.path("settings.json") {
            write(&p, &serde_json::to_string_pretty(&self.settings).unwrap_or_default());
        }
    }

    pub fn save_calc(&self) {
        if let Some(p) = self.path("calc.json") {
            write(&p, &serde_json::to_string(&self.calc).unwrap_or_default());
        }
    }

    fn save_history(&self) {
        if let Some(p) = self.path("history.json") {
            let _ = self.history.save(&p);
        }
    }

    pub fn save_notes(&self) {
        if let Some(p) = self.path("notes.json") {
            let _ = self.notes.save(&p);
        }
    }

    fn classic(&self) -> bool {
        self.theme.family.classic()
    }

    pub fn display(&self) -> Display {
        self.calc.display(self.settings.style, self.classic())
    }

    /// Presses a key by id (layout.rs ids). "calc" and toolbar buttons belong to the app.
    pub fn press(&mut self, key: &str) {
        let key = match key {
            "acback" => {
                if self.display().first_key == "back" {
                    "back"
                } else {
                    "allclear"
                }
            }
            "ce" => {
                self.calc.clear_entry();
                self.save_calc();
                return;
            }
            k => k,
        };
        if let Some(done) = self.calc.press(key) {
            let convert = if self.settings.convert {
                done.value.and_then(|v| {
                    let s = &self.settings;
                    units::convert(&s.conv_cat, &s.conv_from, &s.conv_to, v, &self.rates)
                        .map(|out| ConvInfo { cat: s.conv_cat.clone(), from: s.conv_from.clone(), to: s.conv_to.clone(), out })
                })
            } else {
                None
            };
            self.history.add(done.expr, done.value, now(), convert);
            self.save_history();
        }
        self.save_calc();
    }

    /// Swiping across the number deletes its last digit.
    pub fn swipe_delete(&mut self) {
        self.calc.backspace();
        self.save_calc();
    }

    pub fn set_size(&mut self, w: f32, h: f32, insets: Insets) {
        self.size = (w, h, insets);
    }

    pub fn layout(&mut self) -> Layout {
        let (w, h, insets) = self.size;
        let d = self.display();
        let l = layout::layout(&LayoutIn {
            w,
            h,
            insets,
            mode: self.settings.mode,
            convert: self.settings.convert,
            sidebar_open: self.settings.sidebar_open,
            theme: &self.theme,
            disp: &d,
            decimal: self.settings.style.decimal(),
        });
        self.calc.max_digits = l.max_digits;
        l
    }

    pub fn set_theme(&mut self, id: &str) {
        self.theme = theme::theme(id);
        self.settings.theme = self.theme.id.into();
        self.save_settings();
    }

    pub fn set_mode(&mut self, m: Mode) {
        self.settings.mode = m;
        self.save_settings();
    }

    pub fn set_convert(&mut self, on: bool) {
        self.settings.convert = on;
        self.save_settings();
    }

    pub fn set_style(&mut self, s: NumberStyle) {
        self.settings.style = s;
        self.save_settings();
    }

    pub fn convert_state(&self) -> Option<ConvState> {
        if !self.settings.convert {
            return None;
        }
        let s = &self.settings;
        let cat = units::category(&self.rates, &s.conv_cat)?;
        let from = units::unit(&self.rates, &s.conv_cat, &s.conv_from)?;
        let to = units::unit(&self.rates, &s.conv_cat, &s.conv_to)?;
        let v = self.calc.value();
        let out_value = units::convert(&s.conv_cat, &s.conv_from, &s.conv_to, v, &self.rates).unwrap_or(f64::NAN);
        Some(ConvState {
            cat: cat.id.into(),
            cat_name: cat.name.into(),
            from,
            to,
            out: format_value(money(&s.conv_cat, out_value), self.calc.max_digits, s.style),
            out_value,
            rates_updated: self.rates.updated,
            rates_source: self.rates.source.clone(),
        })
    }

    /// Picks a unit for the upper (`top`) or lower row; another category resets the other row.
    pub fn set_unit(&mut self, top: bool, cat: &str, unit: &str) {
        let s = &mut self.settings;
        if s.conv_cat != cat {
            let (a, b) = units::default_pair(cat);
            s.conv_cat = cat.into();
            s.conv_from = a.into();
            s.conv_to = b.into();
        }
        if top {
            if s.conv_to == unit {
                s.conv_to = s.conv_from.clone();
            }
            s.conv_from = unit.into();
        } else {
            if s.conv_from == unit {
                s.conv_from = s.conv_to.clone();
            }
            s.conv_to = unit.into();
        }
        self.save_settings();
    }

    /// ⇅: the units trade places and the converted value becomes the input.
    pub fn swap_units(&mut self) {
        let out = self.convert_state().map(|c| c.out_value).filter(|v| v.is_finite());
        let s = &mut self.settings;
        std::mem::swap(&mut s.conv_from, &mut s.conv_to);
        if let Some(v) = out {
            if v != 0.0 {
                self.calc.all_clear();
                self.calc.enter_value(round_sig(v, 10));
            }
        }
        self.save_settings();
        self.save_calc();
    }

    pub fn set_rates_json(&mut self, json: &str) -> bool {
        match Rates::from_json(json) {
            Some(r) => {
                if let Some(p) = self.path("rates.json") {
                    write(&p, &r.to_json());
                }
                self.rates = r;
                true
            }
            None => false,
        }
    }

    /// Rates older than six hours are refreshed when the converter opens.
    pub fn rates_stale(&self) -> bool {
        now() - self.rates.updated > 6 * 3600
    }

    pub fn history_groups(&self, utc_offset: i32) -> Vec<HistGroup> {
        let style = self.settings.style;
        history::groups(&self.history, now(), utc_offset)
            .into_iter()
            .map(|g| HistGroup {
                title: g.title,
                rows: g
                    .ids
                    .iter()
                    .filter_map(|id| self.history.get(*id))
                    .map(|e| {
                        let result = e.value.map(|v| format_value(v, 12, style)).unwrap_or_else(|| "Error".into());
                        let conv = e.convert.as_ref().map(|c| {
                            let from = units::unit(&self.rates, &c.cat, &c.from).map(|u| u.symbol).unwrap_or(c.from.clone());
                            let to = units::unit(&self.rates, &c.cat, &c.to).map(|u| u.symbol).unwrap_or(c.to.clone());
                            (format!("{result} {from}"), format!("{} {to}", format_value(money(&c.cat, c.out), 12, style)))
                        });
                        HistRow { id: e.id, expr: segments(&e.expr, style, 12), result, date: history::short_date(e.time, utc_offset), conv }
                    })
                    .collect(),
            })
            .collect()
    }

    pub fn history_load(&mut self, id: u64) {
        if let Some(e) = self.history.get(id).cloned() {
            self.calc.load(e.expr, e.value);
            if let Some(c) = e.convert {
                self.settings.convert = true;
                self.settings.conv_cat = c.cat;
                self.settings.conv_from = c.from;
                self.settings.conv_to = c.to;
                self.save_settings();
            }
            self.save_calc();
        }
    }

    pub fn history_delete(&mut self, ids: &[u64]) {
        self.history.delete(ids);
        self.save_history();
    }

    pub fn history_clear(&mut self) {
        self.history.clear();
        self.save_history();
    }

    /// The text of a history row, for copying.
    pub fn history_text(&self, id: u64) -> Option<String> {
        let e = self.history.get(id)?;
        let style = self.settings.style;
        Some(format!("{} = {}", plain_text(&segments(&e.expr, style, 12)), e.value.map(|v| format_value(v, 12, style)).unwrap_or("Error".into())))
    }

    /// The shown value as plain text (Copy).
    pub fn copy_text(&self) -> String {
        let d = self.display();
        if d.error {
            return "Error".into();
        }
        plain(d.value)
    }

    /// Paste: a number (grouping and either decimal mark accepted) is entered as typed.
    pub fn paste(&mut self, text: &str) -> bool {
        let style = self.settings.style;
        let t: String = text.trim().chars().filter(|c| !c.is_whitespace() && *c != '\u{202F}').collect();
        let t = if style.decimal() == ',' { t.replace('.', "").replace(',', ".") } else { t.replace(',', "") };
        match t.replace('−', "-").parse::<f64>() {
            Ok(v) if v.is_finite() => {
                self.calc.enter_value(v);
                self.save_calc();
                true
            }
            _ => false,
        }
    }
}

/// The key a typed character presses (desktop and hardware keyboards).
pub fn key_for_char(ch: char) -> Option<&'static str> {
    Some(match ch {
        '0' => "0",
        '1' => "1",
        '2' => "2",
        '3' => "3",
        '4' => "4",
        '5' => "5",
        '6' => "6",
        '7' => "7",
        '8' => "8",
        '9' => "9",
        '.' | ',' => "dot",
        '+' => "add",
        '-' | '−' => "sub",
        '*' | 'x' | 'X' | '×' => "mul",
        '/' | '÷' => "div",
        '=' => "eq",
        '%' => "pct",
        '(' => "lp",
        ')' => "rp",
        '^' => "pow",
        '!' => "fact",
        'p' => "pi",
        'e' => "e",
        's' => "sin",
        'c' => "cos",
        't' => "tan",
        'l' => "ln",
        'r' => "sqrt",
        _ => return None,
    })
}

/// Currency amounts show cents, as on iOS (523.73 EUR).
fn money(cat: &str, v: f64) -> f64 {
    if cat == "currency" && v.is_finite() && v.abs() < 1e13 {
        (v * 100.0).round() / 100.0
    } else {
        v
    }
}

fn round_sig(v: f64, d: i32) -> f64 {
    if v == 0.0 {
        return 0.0;
    }
    let m = 10f64.powi(d - 1 - v.abs().log10().floor() as i32);
    (v * m).round() / m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_and_convert() {
        let mut s = Session::new(None);
        for k in ["5", "6", "2", "eq"] {
            s.press(k);
        }
        assert_eq!(s.history.entries.len(), 1);
        s.set_convert(true);
        s.set_unit(true, "temperature", "C");
        s.set_unit(false, "temperature", "F");
        s.press("allclear");
        for k in ["1", "0", "0"] {
            s.press(k);
        }
        assert_eq!(s.convert_state().unwrap().out, "212");
        s.swap_units();
        assert_eq!(s.settings.conv_from, "F");
        assert_eq!(s.convert_state().unwrap().out, "100");
        s.set_unit(true, "currency", "USD");
        s.set_unit(false, "currency", "EUR");
        let out = s.convert_state().unwrap().out;
        assert!(out.split('.').nth(1).map_or(true, |f| f.len() <= 2), "{out}");
    }

    #[test]
    fn acback_and_paste() {
        let mut s = Session::new(None);
        s.set_theme("ios18");
        s.press("7");
        assert_eq!(s.display().first_key, "back");
        s.press("acback");
        assert!(s.display().empty);
        assert!(s.paste("1,234.5"));
        assert_eq!(s.copy_text(), "1234.5");
    }
}
