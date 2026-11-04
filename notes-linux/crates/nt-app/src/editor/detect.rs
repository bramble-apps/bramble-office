//! Edit > Substitutions > Data Detectors: dates, phone numbers and street addresses found in the
//! text, marked with a light dotted line and offered in the right-click menu (copy; call or
//! message a number; a map of an address). Found per paragraph, cached by its text.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Date,
    Phone,
    Address,
}

const MONTHS: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
const DAYS: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
const STREETS: [&str; 30] = [
    "street", "st", "avenue", "ave", "road", "rd", "boulevard", "blvd", "lane", "ln", "drive", "dr", "way", "court", "ct", "place", "pl", "terrace", "crescent", "cres", "highway", "hwy", "parkway", "pkwy", "circle", "cir", "square", "sq", "trail", "close",
];

/// Byte ranges of what's found in `t`, in order, not overlapping.
pub fn find(t: &str) -> Vec<(usize, usize, Kind)> {
    let words = words(t);
    let mut out: Vec<(usize, usize, Kind)> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if let Some((end, k)) = address(t, &words, i).map(|e| (e, Kind::Address)).or_else(|| date(t, &words, i).map(|e| (e, Kind::Date))) {
            out.push((words[i].0, words[end].1, k));
            i = end + 1;
            continue;
        }
        i += 1;
    }
    // phone numbers: runs of digits with the usual separators, 7 to 15 digits
    let b = t.as_bytes();
    let mut j = 0;
    while j < b.len() {
        let starts = (b[j].is_ascii_digit() || b[j] == b'+' || b[j] == b'(') && (j == 0 || !(b[j - 1].is_ascii_alphanumeric() || b[j - 1] == b'/' || b[j - 1] == b'.' || b[j - 1] == b'-'));
        if !starts {
            j += 1;
            continue;
        }
        let mut k = j;
        let mut digits = 0;
        let mut last_digit = j;
        while k < b.len() && (b[k].is_ascii_digit() || matches!(b[k], b' ' | b'-' | b'.' | b'(' | b')' | b'+')) {
            if b[k].is_ascii_digit() {
                digits += 1;
                last_digit = k;
            }
            // (two spaces, or a space before a letter, end it)
            if b[k] == b' ' && b.get(k + 1).is_none_or(|c| !(c.is_ascii_digit() || *c == b'(')) {
                break;
            }
            k += 1;
        }
        let end = last_digit + 1;
        let s = &t[j..end];
        let seps = s.chars().filter(|c| matches!(c, ' ' | '-' | '.' | '(' | ')')).count();
        // (a bare 7+ digit run is a number, not a phone, unless it starts with +; a date like
        // 2026-11-03 isn't one; neither are decimals)
        let plausible = (7..=15).contains(&digits) && (seps >= 1 || s.starts_with('+')) && !s.contains("..") && !is_iso_date(s) && b.get(end).is_none_or(|c| !c.is_ascii_alphanumeric());
        if plausible && !out.iter().any(|(a, z, _)| j < *z && end > *a) {
            out.push((j, end, Kind::Phone));
        }
        j = end.max(j + 1);
    }
    out.sort_by_key(|x| x.0);
    out
}

fn is_iso_date(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    p.len() == 3 && p[0].len() == 4 && p.iter().all(|x| x.chars().all(|c| c.is_ascii_digit()))
}

/// The words of `t` (letters, digits and a few joiners), as byte ranges.
fn words(t: &str) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    let mut start = None;
    for (i, c) in t.char_indices() {
        let w = c.is_alphanumeric() || matches!(c, '/' | ':' | '\'' | '.' | '-');
        match (w, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                v.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        v.push((s, t.len()));
    }
    // (a word's trailing full stop or comma is punctuation)
    v.into_iter().map(|(a, b)| (a, a + t[a..b].trim_end_matches(['.', ':', '-']).len())).filter(|(a, b)| b > a).collect()
}

fn w<'a>(t: &'a str, words: &[(usize, usize)], i: usize) -> &'a str {
    words.get(i).map_or("", |(a, b)| &t[*a..*b])
}

fn month(s: &str) -> bool {
    let s = s.to_lowercase();
    let s = s.trim_end_matches('.');
    s.len() >= 3 && MONTHS.iter().any(|m| m.starts_with(s) && (s.len() == m.len() || s.len() <= 4))
}

fn day_num(s: &str) -> bool {
    let d = s.trim_end_matches(|c: char| c.is_alphabetic());
    let suf = &s[d.len()..].to_lowercase();
    d.parse::<u32>().is_ok_and(|n| (1..=31).contains(&n)) && (suf.is_empty() || ["st", "nd", "rd", "th"].contains(&suf.as_str()))
}

fn year(s: &str) -> bool {
    s.len() == 4 && s.parse::<u32>().is_ok_and(|y| (1900..=2100).contains(&y))
}

/// "at 3pm", "3:30 PM", "15:00" after a date: the index of its last word.
fn time_after(t: &str, words: &[(usize, usize)], i: usize) -> usize {
    let mut j = i;
    if w(t, words, j + 1).eq_ignore_ascii_case("at") {
        j += 1;
    }
    let x = w(t, words, j + 1).to_lowercase();
    let num = x.trim_end_matches("am").trim_end_matches("pm");
    let clock = num.split(':').all(|p| !p.is_empty() && p.len() <= 2 && p.chars().all(|c| c.is_ascii_digit()));
    if clock && (x.contains(':') || x.ends_with("am") || x.ends_with("pm") || ["am", "pm"].contains(&w(t, words, j + 2).to_lowercase().as_str())) {
        j += 1;
        if ["am", "pm"].contains(&w(t, words, j + 1).to_lowercase().as_str()) {
            j += 1;
        }
        return j;
    }
    i
}

/// A date starting at word `i`: the index of its last word.
fn date(t: &str, words: &[(usize, usize)], i: usize) -> Option<usize> {
    let a = w(t, words, i);
    let lower = a.to_lowercase();
    // 2026-11-03, 11/3/2026, 3/11/26, 11/3
    let slash: Vec<&str> = a.split('/').collect();
    if is_iso_date(a) || (slash.len() == 3 && slash.iter().all(|p| !p.is_empty() && p.len() <= 4 && p.chars().all(|c| c.is_ascii_digit())) && slash[0].parse::<u32>().is_ok_and(|m| (1..=31).contains(&m))) {
        return Some(time_after(t, words, i));
    }
    // today, tomorrow, yesterday, (next) Monday
    if ["today", "tomorrow", "yesterday", "tonight"].contains(&lower.as_str()) {
        return Some(time_after(t, words, i));
    }
    if lower == "next" && DAYS.contains(&w(t, words, i + 1).to_lowercase().as_str()) {
        return Some(time_after(t, words, i + 1));
    }
    if DAYS.iter().any(|d| *d == lower || (lower.len() == 3 && d.starts_with(&lower))) {
        // "Monday, November 3" / "Mon 3 Nov": the date after it is part of it
        if let Some(e) = date(t, words, i + 1) {
            return Some(e);
        }
        return a.chars().next().is_some_and(|c| c.is_uppercase()).then(|| time_after(t, words, i));
    }
    // November 3 (2026), Nov 3rd
    if month(a) && a.chars().next().is_some_and(|c| c.is_uppercase()) && day_num(w(t, words, i + 1)) {
        let mut e = i + 1;
        if year(w(t, words, e + 1)) {
            e += 1;
        }
        return Some(time_after(t, words, e));
    }
    // 3 November (2026), 3rd of November
    if day_num(a) {
        let mut j = i + 1;
        if w(t, words, j).eq_ignore_ascii_case("of") {
            j += 1;
        }
        let m = w(t, words, j);
        if month(m) && m.chars().next().is_some_and(|c| c.is_uppercase()) {
            let mut e = j;
            if year(w(t, words, e + 1)) {
                e += 1;
            }
            return Some(time_after(t, words, e));
        }
    }
    None
}

/// A street address at word `i`: a number, one to four words, a street kind (and a city, a
/// state or province and a postal code after a comma). The index of its last word.
fn address(t: &str, words: &[(usize, usize)], i: usize) -> Option<usize> {
    let n = w(t, words, i);
    if !(n.chars().next().is_some_and(|c| c.is_ascii_digit()) && n.len() <= 6 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')) {
        return None;
    }
    let mut j = i + 1;
    let mut named = 0;
    while named < 5 {
        let x = w(t, words, j);
        if x.is_empty() || !x.chars().next().is_some_and(|c| c.is_alphabetic() || c.is_ascii_digit()) {
            return None;
        }
        if named > 0 && STREETS.contains(&x.to_lowercase().trim_end_matches('.')) {
            break;
        }
        if !x.chars().next().is_some_and(|c| c.is_uppercase() || c.is_ascii_digit()) {
            return None;
        }
        named += 1;
        j += 1;
    }
    if named == 5 {
        return None;
    }
    let mut end = j;
    // ", City, ST 12345" / ", City ON K1A 0B1": words up to a postal code on the same line
    let rest = &t[words[end].1..];
    if rest.starts_with(',') {
        let mut k = end + 1;
        let mut seen = 0;
        while seen < 6 && k < words.len() {
            let x = w(t, words, k);
            let between = &t[words[k - 1].1..words[k].0];
            if between.contains('\n') || between.trim_matches([' ', ',']).len() > 0 {
                break;
            }
            let postal = x.chars().all(|c| c.is_ascii_digit()) && (x.len() == 5 || x.len() == 4) || x.len() == 3 && x.chars().any(|c| c.is_ascii_digit()) && x.chars().any(|c| c.is_ascii_uppercase());
            if x.chars().next().is_some_and(|c| c.is_uppercase()) || postal {
                end = k;
                seen += 1;
                k += 1;
                if postal && x.len() != 3 {
                    break;
                }
            } else {
                break;
            }
        }
    }
    Some(end)
}

/// The right-click menu's items for what's found under the click.
pub fn actions(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Phone => &["Call", "Send Message", "Copy Phone Number"],
        Kind::Address => &["Show in Maps", "Copy Address"],
        Kind::Date => &["Copy Date"],
    }
}

/// What an action opens (None: it copies).
pub fn action_url(action: &str, text: &str) -> Option<String> {
    let digits: String = text.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
    match action {
        "Call" => Some(format!("tel:{digits}")),
        "Send Message" => Some(format!("sms:{digits}")),
        "Show in Maps" => Some(format!("https://www.openstreetmap.org/search?query={}", url_encode(text))),
        _ => None,
    }
}

fn url_encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(t: &str) -> Vec<(&str, Kind)> {
        find(t).into_iter().map(|(a, b, k)| (&t[a..b], k)).collect()
    }

    #[test]
    fn each_kind() {
        let d = |t: &str| found(t).into_iter().filter(|x| x.1 == Kind::Date).map(|x| x.0.to_string()).collect::<Vec<_>>();
        let p = |t: &str| found(t).into_iter().filter(|x| x.1 == Kind::Phone).map(|x| x.0.to_string()).collect::<Vec<_>>();
        let a = |t: &str| found(t).into_iter().filter(|x| x.1 == Kind::Address).map(|x| x.0.to_string()).collect::<Vec<_>>();
        assert_eq!(d("Dentist tomorrow at 10:30 am."), ["tomorrow at 10:30 am"]);
        assert_eq!(d("Due 2026-11-03 and 11/3/2026"), ["2026-11-03", "11/3/2026"]);
        assert_eq!(d("on 3rd of November 2026"), ["3rd of November 2026"]);
        assert_eq!(d("Meet next Friday"), ["next Friday"]);
        assert_eq!(d("Nov 3"), ["Nov 3"]);
        assert!(d("I may go, march on, 3 apples").is_empty(), "lower-case month words aren't dates");
        assert_eq!(p("Call (555) 123-4567 or +1 555 987 6543."), ["(555) 123-4567", "+1 555 987 6543"]);
        assert_eq!(p("07123 456789"), ["07123 456789"]);
        assert!(p("Order 12345678, 3.14159, 2026-11-03, v1.2.3").is_empty());
        assert_eq!(a("Ship to 1600 Pennsylvania Avenue, Washington, DC 20500 please"), ["1600 Pennsylvania Avenue, Washington, DC 20500"]);
        assert_eq!(a("at 221B Baker St."), ["221B Baker St"]);
        assert!(a("I have 3 Big Dogs").is_empty());
        assert_eq!(action_url("Call", "(555) 123-4567").as_deref(), Some("tel:5551234567"));
    }
}
