//! The calculation history: entries saved as JSON, shown newest first in date groups like iOS
//! ("Today", "Yesterday", "Previous 7 Days", "Previous 30 Days", then months).

use crate::expr::Tok;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Most entries kept; the oldest are dropped.
pub const MAX_ENTRIES: usize = 500;

/// A conversion done in Convert mode ("562 USD → 4,519.6 MOP").
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConvInfo {
    pub cat: String,
    pub from: String,
    pub to: String,
    pub out: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub expr: Vec<Tok>,
    pub value: Option<f64>,
    /// Unix seconds.
    pub time: i64,
    pub convert: Option<ConvInfo>,
}

/// Entries oldest first.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct History {
    pub entries: Vec<Entry>,
    #[serde(default)]
    next_id: u64,
}

impl History {
    /// Adds an entry and returns its id.
    pub fn add(&mut self, expr: Vec<Tok>, value: Option<f64>, time: i64, convert: Option<ConvInfo>) -> u64 {
        // ids stay unique even if a loaded file lost next_id
        let id = self.next_id.max(self.entries.iter().map(|e| e.id + 1).max().unwrap_or(0));
        self.next_id = id + 1;
        self.entries.push(Entry { id, expr, value, time, convert });
        if self.entries.len() > MAX_ENTRIES {
            let n = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..n);
        }
        id
    }

    pub fn delete(&mut self, ids: &[u64]) {
        self.entries.retain(|e| !ids.contains(&e.id));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// A missing or unreadable file gives an empty history.
    pub fn load(path: &Path) -> History {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// Writes a temporary file next to `path` and renames it over.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        save_json(self, path)
    }
}

pub(crate) fn save_json<T: Serialize>(v: &T, path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let json = serde_json::to_string(v).map_err(std::io::Error::other)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// A titled section of the history list; ids newest first.
#[derive(Clone, Debug, Serialize)]
pub struct Group {
    pub title: String,
    pub ids: Vec<u64>,
}

const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// Days since 1970-01-01 to (year, month 1-12, day 1-31).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

fn local_day(time: i64, utc_offset: i32) -> i64 {
    (time + utc_offset as i64).div_euclid(86_400)
}

/// Newest first: "Today", "Yesterday", "Previous 7 Days", "Previous 30 Days", then "<Month>" for
/// the current year or "<Month> <Year>" for older (like iOS / Notes). `utc_offset` is the local
/// time zone offset in seconds.
pub fn groups(h: &History, now: i64, utc_offset: i32) -> Vec<Group> {
    let today = local_day(now, utc_offset);
    let (year, _, _) = civil_from_days(today);
    let mut es: Vec<&Entry> = h.entries.iter().collect();
    es.sort_by(|a, b| b.time.cmp(&a.time).then(b.id.cmp(&a.id)));
    let mut out: Vec<Group> = Vec::new();
    for e in es {
        let d = local_day(e.time, utc_offset);
        let title = match today - d {
            n if n <= 0 => "Today".to_string(),
            1 => "Yesterday".to_string(),
            n if n <= 7 => "Previous 7 Days".to_string(),
            n if n <= 30 => "Previous 30 Days".to_string(),
            _ => {
                let (y, m, _) = civil_from_days(d);
                let name = MONTHS[m as usize - 1];
                if y == year {
                    name.to_string()
                } else {
                    format!("{name} {y}")
                }
            }
        };
        match out.last_mut() {
            Some(g) if g.title == title => g.ids.push(e.id),
            _ => out.push(Group { title, ids: vec![e.id] }),
        }
    }
    out
}

/// The date shown at the right of a row: "6/24/24" style (M/D/YY), local time.
pub fn short_date(time: i64, utc_offset: i32) -> String {
    let (y, m, d) = civil_from_days(local_day(time, utc_offset));
    format!("{m}/{d}/{:02}", y.rem_euclid(100))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    // 2026-11-01 12:00 UTC
    const NOW: i64 = 1_793_534_400;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(NOW / DAY), (2026, 11, 1));
        assert_eq!(short_date(1_719_230_400, 0), "6/24/24");
        // 2026-11-01 02:00 UTC is still Oct 31 at UTC-5
        assert_eq!(short_date(NOW - 10 * 3600, -5 * 3600), "10/31/26");
    }

    #[test]
    fn grouping() {
        let mut h = History::default();
        let t = |d: i64| NOW - d * DAY;
        let a = h.add(vec![], Some(1.0), t(400), None); // Sept 2025
        let b = h.add(vec![], Some(1.0), t(60), None); // early Sept 2026
        let c = h.add(vec![], Some(1.0), t(20), None);
        let d = h.add(vec![], Some(1.0), t(3), None);
        let e = h.add(vec![], Some(1.0), t(1), None);
        let f = h.add(vec![], Some(1.0), t(0), None);
        let g = h.add(vec![], Some(1.0), t(0) + 60, None);
        let gs = groups(&h, NOW + 120, 0);
        let titles: Vec<&str> = gs.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["Today", "Yesterday", "Previous 7 Days", "Previous 30 Days", "September", "September 2025"]);
        assert_eq!(gs[0].ids, [g, f]);
        assert_eq!(gs[1].ids, [e]);
        assert_eq!(gs[2].ids, [d]);
        assert_eq!(gs[3].ids, [c]);
        assert_eq!(gs[4].ids, [b]);
        assert_eq!(gs[5].ids, [a]);
    }

    #[test]
    fn add_delete_cap() {
        let mut h = History::default();
        for i in 0..505 {
            h.add(vec![], Some(i as f64), i, None);
        }
        assert_eq!(h.entries.len(), MAX_ENTRIES);
        assert_eq!(h.entries[0].id, 5);
        h.delete(&[5, 6]);
        assert!(h.get(5).is_none() && h.get(7).is_some());
        h.clear();
        assert!(h.entries.is_empty());
        assert_eq!(h.add(vec![], None, 0, None), 505);
    }

    #[test]
    fn save_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("calc-history-test-{}", std::process::id()));
        let path = dir.join("history.json");
        let mut h = History::default();
        let conv = ConvInfo { cat: "Currency".into(), from: "USD".into(), to: "MOP".into(), out: 4519.6 };
        h.add(vec![Tok::Num { s: "2".into(), neg: false }, Tok::Op(crate::expr::Bin::Add), Tok::Pi], Some(5.14), 100, None);
        h.add(vec![Tok::Num { s: "562".into(), neg: false }], Some(562.0), 200, Some(conv));
        h.save(&path).unwrap();
        let l = History::load(&path);
        assert_eq!(l.entries.len(), 2);
        assert_eq!(l.entries[0].expr, h.entries[0].expr);
        assert_eq!(l.entries[1].convert.as_ref().unwrap().to, "MOP");
        let mut l = l;
        assert_eq!(l.add(vec![], None, 300, None), 2);
        std::fs::write(&path, "not json").unwrap();
        assert!(History::load(&path).entries.is_empty());
        assert!(History::load(&dir.join("missing.json")).entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
