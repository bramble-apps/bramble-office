//! Dates as Notes shows them, in local time: the list's "2:56 PM" / "Yesterday" / "Monday" /
//! "11/3/26", the editor's "November 3, 2026 at 2:56 PM" and the list's date groups (Today,
//! Yesterday, Previous 7 Days, Previous 30 Days, then months of this year, then years).

/// A moment broken down in local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    /// 1..=12
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// 0 = Sunday
    pub weekday: u32,
    /// days since 1970-01-01 in local time (for comparing calendar days)
    pub days: i64,
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// Seconds since 1970 now (NT_NOW overrides it, so test snapshots don't depend on the clock).
pub fn now() -> f64 {
    if let Some(t) = std::env::var("NT_NOW").ok().and_then(|s| s.parse::<f64>().ok()) {
        return t;
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// The local time zone's offset from UTC in seconds at `t`.
pub(crate) fn offset(t: f64) -> i64 {
    #[cfg(unix)]
    {
        let secs = t as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: localtime_r writes only into tm
        if !unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
            return tm.tm_gmtoff;
        }
    }
    let _ = t;
    0
}

pub fn local(t: f64) -> Local {
    let s = t.floor() as i64 + offset(t);
    let days = s.div_euclid(86400);
    let rem = s.rem_euclid(86400);
    // civil from days (Howard Hinnant's algorithm)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (if m <= 2 { y + 1 } else { y }) as i32;
    Local { year, month: m, day: d, hour: (rem / 3600) as u32, minute: ((rem % 3600) / 60) as u32, weekday: (days + 4).rem_euclid(7) as u32, days }
}

/// The local day number (`Local::days`) of a calendar date (Howard Hinnant's days from civil).
pub fn day_number(year: i32, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year as i64 - 1 } else { year as i64 };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// "2026-11-04" as a local day number (Smart Folders' date filters).
pub fn parse_day(s: &str) -> Option<i64> {
    let mut it = s.trim().split('-').map(|p| p.trim().parse::<i64>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then(|| day_number(y as i32, m as u32, d as u32))
}

/// A local day number as "2026-11-04".
pub fn format_day(days: i64) -> String {
    let l = local(days as f64 * 86400.0 + 43200.0 - offset(days as f64 * 86400.0) as f64);
    format!("{:04}-{:02}-{:02}", l.year, l.month, l.day)
}

/// "2:56 PM"
pub fn time(t: f64) -> String {
    let l = local(t);
    let h = if l.hour % 12 == 0 { 12 } else { l.hour % 12 };
    format!("{h}:{:02}\u{202f}{}", l.minute, if l.hour < 12 { "AM" } else { "PM" })
}

/// The list's date: the time today, "Yesterday", the weekday this week, else "11/3/26".
pub fn short(t: f64, now: f64) -> String {
    let (l, n) = (local(t), local(now));
    match n.days - l.days {
        0 => time(t),
        1 => "Yesterday".into(),
        2..=6 => WEEKDAYS[l.weekday as usize].into(),
        _ => format!("{}/{}/{:02}", l.month, l.day, l.year.rem_euclid(100)),
    }
}

/// The editor's date line: "November 3, 2026 at 2:56 PM".
pub fn long(t: f64) -> String {
    let l = local(t);
    format!("{} {}, {} at {}", MONTHS[(l.month - 1) as usize], l.day, l.year, time(t))
}

/// The date group a note edited (or created) at `t` falls in, with a key that sorts the groups
/// newest first.
pub fn group(t: f64, now: f64) -> (i64, String) {
    let (l, n) = (local(t), local(now));
    let ago = n.days - l.days;
    match ago {
        i64::MIN..=0 => (i64::MAX, "Today".into()),
        1 => (i64::MAX - 1, "Yesterday".into()),
        2..=7 => (i64::MAX - 2, "Previous 7 Days".into()),
        8..=30 => (i64::MAX - 3, "Previous 30 Days".into()),
        _ if l.year == n.year => (l.year as i64 * 100 + l.month as i64, MONTHS[(l.month - 1) as usize].into()),
        _ => (l.year as i64 * 100, l.year.to_string()),
    }
}

/// Today at hh:mm local time (the demo library's times).
pub fn today_at(hour: u32, minute: u32, days_ago: i64) -> f64 {
    let n = now();
    let l = local(n);
    let midnight = n - (l.hour * 3600 + l.minute * 60) as f64 - n.rem_euclid(60.0);
    midnight + (hour * 3600 + minute * 60) as f64 - days_ago as f64 * 86400.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        // 2026-11-03 14:56 UTC is a Tuesday
        let t = 1_793_717_760.0;
        let l = local(t - offset(t) as f64);
        assert_eq!((l.year, l.month, l.day, l.hour, l.minute, l.weekday), (2026, 11, 3, 14, 56, 2));
    }

    #[test]
    fn day_numbers() {
        assert_eq!(day_number(1970, 1, 1), 0);
        assert_eq!(parse_day("2026-11-04"), Some(day_number(2026, 11, 4)));
        assert_eq!(format_day(day_number(2026, 11, 4)), "2026-11-04");
        assert_eq!(local(1_793_717_760.0 - offset(1_793_717_760.0) as f64).days, day_number(2026, 11, 3));
    }

    #[test]
    fn groups() {
        let now = today_at(15, 0, 0);
        assert_eq!(group(now - 60.0, now).1, "Today");
        assert_eq!(group(now - 86400.0, now).1, "Yesterday");
        assert_eq!(group(now - 3.0 * 86400.0, now).1, "Previous 7 Days");
        assert_eq!(group(now - 20.0 * 86400.0, now).1, "Previous 30 Days");
        assert_eq!(short(now - 86400.0, now), "Yesterday");
    }
}
