//! Settings ▸ View ▸ General's help-tag options: "Show beats and time in help tags" adds the
//! time under every position a help tag shows.

use crate::app::App;

pub const BEATS_AND_TIME: &str = "View / General / Show beats and time in help tags";

/// The rows a help tag shows, with the time after each position when the setting is on.
pub fn decorate(app: &App, rows: Vec<(String, String)>) -> Vec<(String, String)> {
    if !app.logic_settings.on(BEATS_AND_TIME) {
        return rows;
    }
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let mut out = Vec::with_capacity(rows.len() + 1);
    for (k, v) in rows {
        let time = is_position(&k).then(|| crate::eventedit::parse_position(&v, sig)).flatten();
        out.push((k, v));
        if let Some(t) = time {
            out.push((String::new(), crate::prefs::time_text(&app.logic_settings, &app.proj, app.seconds_at(t as f64))));
        }
    }
    out
}

fn is_position(label: &str) -> bool {
    matches!(label.trim_end_matches(':'), "Position" | "Start" | "Move")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_follows_positions() {
        let mut app = crate::tm::test_app();
        let rows = vec![("Position:".to_string(), "2 1 1 1".to_string()), ("Length:".into(), "1 0 0 0".into())];
        assert_eq!(decorate(&app, rows.clone()).len(), 2);
        app.logic_settings.0.insert(BEATS_AND_TIME.into(), "1".into());
        let d = decorate(&app, rows);
        assert_eq!(d.len(), 3);
        assert_eq!(d[1].0, "");
        assert!(d[1].1.contains(':'), "{:?}", d[1]);
    }
}
