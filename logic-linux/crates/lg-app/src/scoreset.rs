//! Score project settings (File ▸ Project Settings ▸ Score, and the Score's Layout ▸ settings
//! window): Global (margins, header space, line distance, Maximum Bars/Line, justify last staff,
//! open single staffs, hiding muted notes / regions / tracks), Numbers & Names (page numbers,
//! bar numbers, instrument names) and Clefs & Signatures (where clefs, key and time signatures
//! show; hiding bar lines). The values live with the project's other settings (projsettings.rs
//! rows "Score / …"); score.rs lays the score out by them.
//!
//! Lengths are in centimetres on Studio's A4 page (21 cm wide = 120 staff spaces).

use crate::projsettings::{set, Values};

/// Staff spaces per centimetre (a staff space is 1.75 mm).
pub const SP_PER_CM: f32 = 10.0 / 1.75;

pub const SHOW: [&str; 4] = ["Every Staff", "First Staff on Every Page", "First Staff on Page 1", "Hide All"];
pub const PAGE_H: [&str; 5] = ["Alternating", "Left", "Right", "Centered", "Reverse Alternating"];
pub const NAMES: [&str; 3] = ["No Names", "Short Names", "Full Names"];

/// Where a clef / key / time signature shows (Clefs & Signatures' menus).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Show {
    Every,
    FirstEveryPage,
    FirstPage1,
    Hide,
}

impl Show {
    fn parse(s: &str) -> Show {
        match SHOW.iter().position(|x| *x == s) {
            Some(1) => Show::FirstEveryPage,
            Some(2) => Show::FirstPage1,
            Some(3) => Show::Hide,
            _ => Show::Every,
        }
    }

    /// Shown on a line: `first_on_page` (the page's first line), `first` (the score's first).
    pub fn on(self, first_on_page: bool, first: bool) -> bool {
        match self {
            Show::Every => true,
            Show::FirstEveryPage => first_on_page,
            Show::FirstPage1 => first,
            Show::Hide => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NameKind {
    None,
    Short,
    Full,
}

/// The settings as the layout uses them.
#[derive(Clone, Debug, PartialEq)]
pub struct Fmt {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
    pub bracket_space: bool,
    pub alternating: bool,
    pub header: f32,
    pub line_dist: f32,
    pub max_bars: usize,
    pub justify_last: bool,
    pub open_single: bool,
    pub hide_muted_notes: bool,
    pub hide_muted_regions: bool,
    pub hide_muted_tracks: bool,
    pub page_numbers: bool,
    pub page_h: usize,
    pub page_top: bool,
    pub page_offset: i32,
    pub page_hdist: f32,
    pub page_vdist: f32,
    pub hide_first_page: bool,
    pub prefix: String,
    pub bar_numbers: bool,
    pub bar_center: bool,
    pub bar_v: f32,
    pub bar_step: u32,
    pub bar_offset: i32,
    pub bar_start: u32,
    pub bar_hide_linear: bool,
    pub bar_follow: bool,
    pub bar_top_only: bool,
    pub names: bool,
    pub names_above: bool,
    pub names_right: bool,
    pub first_names: NameKind,
    pub other_names: NameKind,
    pub clefs: Show,
    pub keys: Show,
    pub times: Show,
    pub hide_bar_lines: bool,
}

fn g(v: &Values, sub: &str, l: &str) -> String {
    v.get(&format!("Score / {sub} / {l}"))
}

fn names_of(s: &str) -> NameKind {
    match s {
        "No Names" => NameKind::None,
        "Short Names" => NameKind::Short,
        _ => NameKind::Full,
    }
}

pub fn fmt(v: &Values) -> Fmt {
    let n = |sub: &str, l: &str| g(v, sub, l).parse::<f32>().unwrap_or(0.0);
    let on = |sub: &str, l: &str| g(v, sub, l) == "1";
    let (gl, nn, cs) = ("Global", "Numbers & Names", "Clefs & Signatures");
    Fmt {
        top: n(gl, "Top Margin:"),
        bottom: n(gl, "Bottom Margin:"),
        left: n(gl, "Left Margin:"),
        right: n(gl, "Right Margin:"),
        bracket_space: on(gl, "Add bracket space"),
        alternating: on(gl, "Alternating margins"),
        header: n(gl, "Header Space:"),
        line_dist: n(gl, "Line Distance:"),
        max_bars: n(gl, "Maximum Bars/Line:").max(1.0) as usize,
        justify_last: on(gl, "Justify last staff"),
        open_single: on(gl, "Open single staffs"),
        hide_muted_notes: on(gl, "Hide muted notes"),
        hide_muted_regions: on(gl, "Hide muted regions"),
        hide_muted_tracks: on(gl, "Hide muted tracks"),
        page_numbers: on(nn, "Page Numbers"),
        page_h: PAGE_H.iter().position(|x| *x == g(v, nn, "Page Horizontal Position:")).unwrap_or(0),
        page_top: g(v, nn, "Page Vertical Position:") == "Top",
        page_offset: n(nn, "Page Offset:") as i32,
        page_hdist: n(nn, "Page Horizontal Distance:"),
        page_vdist: n(nn, "Page Vertical Distance:"),
        hide_first_page: on(nn, "Hide first page number"),
        prefix: g(v, nn, "Prefix:"),
        bar_numbers: on(nn, "Bar Numbers"),
        bar_center: g(v, nn, "Bar Horizontal Position:") == "At Bar Center",
        bar_v: n(nn, "Bar Vertical Position:"),
        bar_step: n(nn, "Step:").max(0.0) as u32,
        bar_offset: n(nn, "Bar Offset:") as i32,
        bar_start: n(nn, "Start With:").max(1.0) as u32,
        bar_hide_linear: on(nn, "Hide bar numbers in linear view"),
        bar_follow: on(nn, "Follow staff size"),
        bar_top_only: on(nn, "Top/bottom staff only"),
        names: on(nn, "Instrument Names"),
        names_above: g(v, nn, "Name Position:") == "Above Staffs",
        names_right: g(v, nn, "Name Align:") == "Right",
        first_names: names_of(&g(v, nn, "First Staff:")),
        other_names: names_of(&g(v, nn, "Other Staffs:")),
        clefs: Show::parse(&g(v, cs, "Clefs:")),
        keys: Show::parse(&g(v, cs, "Key Signatures:")),
        times: Show::parse(&g(v, cs, "Time Signatures:")),
        hide_bar_lines: on(cs, "Hide bar lines"),
    }
}

impl Fmt {
    /// Left and right margins of page `page` (0-based) in staff spaces (Alternating margins swap
    /// them on every second page; Add bracket space widens the left).
    pub fn margins(&self, page: usize) -> (f32, f32) {
        let (mut l, mut r) = (self.left, self.right);
        if self.alternating && page % 2 == 1 {
            std::mem::swap(&mut l, &mut r);
        }
        let bracket = if self.bracket_space { 0.5 } else { 0.0 };
        ((l + bracket) * SP_PER_CM, r * SP_PER_CM)
    }

    /// The text of page `page`'s number (None: not shown). The prefix's # becomes the number
    /// (else the number follows after a space); \i the score set, \n the project, \s the display
    /// level, \d the print date.
    pub fn page_text(&self, page: usize, project: &str, set_name: &str, level: &str) -> Option<String> {
        if !self.page_numbers || (self.hide_first_page && page == 0) {
            return None;
        }
        let num = (page as i32 + 1 + self.page_offset).to_string();
        let date = today();
        let pre = self.prefix.replace("\\i", set_name).replace("\\n", project).replace("\\s", level).replace("\\d", &date);
        Some(if pre.contains('#') { pre.replace('#', &num) } else if pre.is_empty() { num } else { format!("{pre} {num}") })
    }

    /// Where page `page`'s number sits: 0 left, 1 centre, 2 right.
    pub fn page_side(&self, page: usize) -> u8 {
        let odd = page % 2 == 0; // (page 1, 3, …)
        match self.page_h {
            1 => 0,
            2 => 2,
            3 => 1,
            4 => if odd { 0 } else { 2 },
            _ => if odd { 2 } else { 0 },
        }
    }

    /// The number bar `bar` (0-based) shows, if any: `line_start` (the first bar of a line).
    pub fn bar_number(&self, bar: u32, line_start: bool) -> Option<i32> {
        if !self.bar_numbers || bar + 1 < self.bar_start {
            return None;
        }
        let shown = if self.bar_step == 0 { line_start } else { (bar + 1 - self.bar_start) % self.bar_step == 0 };
        shown.then_some(bar as i32 + 1 + self.bar_offset)
    }

    /// A track's name on a line: the first line takes First Staff's choice, the others Other
    /// Staffs'. Short names: the first word, up to four letters, and a full stop.
    pub fn name_for(&self, full: &str, first: bool) -> Option<String> {
        if !self.names {
            return None;
        }
        match if first { self.first_names } else { self.other_names } {
            NameKind::None => None,
            NameKind::Full => Some(full.to_string()),
            NameKind::Short => Some(short_name(full)),
        }
    }
}

pub fn short_name(full: &str) -> String {
    let w = full.split_whitespace().next().unwrap_or("");
    if w.chars().count() <= 4 {
        w.to_string()
    } else {
        format!("{}.", w.chars().take(4).collect::<String>())
    }
}

pub(crate) fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let days = secs.div_euclid(86_400);
    // (civil date from days since 1970, Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

// ---- the panes -----------------------------------------------------------------------------

fn check(ui: &mut egui::Ui, v: &mut Values, sub: &str, l: &str, enabled: bool) -> bool {
    let k = format!("Score / {sub} / {l}");
    let mut on = v.on(&k);
    if ui.add_enabled(enabled, egui::Checkbox::new(&mut on, l)).changed() {
        set(v, &k, if on { "1" } else { "0" });
    }
    on
}

fn num(ui: &mut egui::Ui, v: &mut Values, sub: &str, l: &str, lo: f32, hi: f32, unit: &str, decimals: usize, enabled: bool) {
    let k = format!("Score / {sub} / {l}");
    ui.add_enabled(enabled, egui::Label::new(l));
    let mut x = v.num(&k);
    let r = ui.add_enabled(enabled, egui::DragValue::new(&mut x).range(lo..=hi).speed(if decimals > 0 { 0.02 } else { 0.2 }).fixed_decimals(decimals).suffix(unit));
    if r.changed() {
        set(v, &k, &format!("{x:.*}", decimals));
    }
}

fn popup(ui: &mut egui::Ui, v: &mut Values, sub: &str, l: &str, choices: &[&str], enabled: bool) {
    let k = format!("Score / {sub} / {l}");
    ui.add_enabled(enabled, egui::Label::new(l));
    let cur = v.get(&k);
    ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::from_id_salt(&k).selected_text(&cur).width(200.0).show_ui(ui, |ui| {
            for c in choices {
                if ui.selectable_label(cur == *c, *c).clicked() {
                    set(v, &k, c);
                }
            }
        });
    });
}

/// One sub-pane (Global, Numbers & Names, Clefs & Signatures).
pub fn pane(ui: &mut egui::Ui, v: &mut Values, sub: &str) {
    match sub {
        "Global" => {
            let gl = "Global";
            egui::Grid::new("sc-global").num_columns(4).spacing([10.0, 5.0]).show(ui, |ui| {
                num(ui, v, gl, "Top Margin:", 0.0, 10.0, " cm", 2, true);
                num(ui, v, gl, "Bottom Margin:", 0.0, 10.0, " cm", 2, true);
                ui.end_row();
                num(ui, v, gl, "Left Margin:", 0.0, 10.0, " cm", 2, true);
                num(ui, v, gl, "Right Margin:", 0.0, 10.0, " cm", 2, true);
                ui.end_row();
                num(ui, v, gl, "Header Space:", 0.0, 15.0, " cm", 2, true);
                num(ui, v, gl, "Line Distance:", 0.0, 10.0, " cm", 2, true);
                ui.end_row();
                num(ui, v, gl, "Maximum Bars/Line:", 1.0, 99.0, "", 0, true);
                ui.end_row();
            });
            ui.horizontal(|ui| {
                check(ui, v, gl, "Add bracket space", true);
                check(ui, v, gl, "Alternating margins", true);
            });
            for l in ["Justify last staff", "Open single staffs", "Hide muted notes", "Hide muted regions", "Hide muted tracks"] {
                check(ui, v, gl, l, true);
            }
        }
        "Numbers & Names" => {
            let nn = "Numbers & Names";
            let pages = check(ui, v, nn, "Page Numbers", true);
            egui::Grid::new("sc-pages").num_columns(4).spacing([10.0, 5.0]).show(ui, |ui| {
                popup(ui, v, nn, "Page Horizontal Position:", &PAGE_H, pages);
                popup(ui, v, nn, "Page Vertical Position:", &["Top", "Bottom"], pages);
                ui.end_row();
                num(ui, v, nn, "Page Offset:", -99.0, 999.0, "", 0, pages);
                num(ui, v, nn, "Page Horizontal Distance:", 0.0, 10.0, " cm", 2, pages);
                ui.end_row();
                num(ui, v, nn, "Page Vertical Distance:", 0.0, 10.0, " cm", 2, pages);
                ui.add_enabled(pages, egui::Label::new("Prefix:"));
                let k = "Score / Numbers & Names / Prefix:";
                let mut t = v.get(k);
                if ui.add_enabled(pages, egui::TextEdit::singleline(&mut t).desired_width(160.0)).on_hover_text("# the page number; \\i score set, \\n project, \\s display level, \\d date").changed() {
                    set(v, k, &t);
                }
                ui.end_row();
            });
            check(ui, v, nn, "Hide first page number", pages);
            ui.separator();
            let bars = check(ui, v, nn, "Bar Numbers", true);
            egui::Grid::new("sc-bars").num_columns(4).spacing([10.0, 5.0]).show(ui, |ui| {
                popup(ui, v, nn, "Bar Horizontal Position:", &["At Bar Line", "At Bar Center"], bars);
                num(ui, v, nn, "Bar Vertical Position:", -20.0, 20.0, "", 1, bars);
                ui.end_row();
                num(ui, v, nn, "Step:", 0.0, 99.0, "", 0, bars);
                num(ui, v, nn, "Bar Offset:", -999.0, 999.0, "", 0, bars);
                ui.end_row();
                num(ui, v, nn, "Start With:", 1.0, 999.0, "", 0, bars);
                ui.end_row();
            });
            ui.horizontal_wrapped(|ui| {
                for l in ["Hide bar numbers in linear view", "Follow staff size", "Top/bottom staff only"] {
                    check(ui, v, nn, l, bars);
                }
            });
            ui.separator();
            let names = check(ui, v, nn, "Instrument Names", true);
            egui::Grid::new("sc-names").num_columns(4).spacing([10.0, 5.0]).show(ui, |ui| {
                popup(ui, v, nn, "Name Position:", &["Above Staffs", "Beside Staffs"], names);
                popup(ui, v, nn, "Name Align:", &["Left", "Right"], names);
                ui.end_row();
                popup(ui, v, nn, "First Staff:", &NAMES, names);
                popup(ui, v, nn, "Other Staffs:", &NAMES, names);
                ui.end_row();
            });
        }
        "Clefs & Signatures" => {
            let cs = "Clefs & Signatures";
            egui::Grid::new("sc-clefs").num_columns(2).spacing([10.0, 5.0]).show(ui, |ui| {
                popup(ui, v, cs, "Clefs:", &SHOW, true);
                ui.end_row();
                popup(ui, v, cs, "Key Signatures:", &SHOW, true);
                ui.end_row();
                popup(ui, v, cs, "Time Signatures:", &SHOW, true);
                ui.end_row();
            });
            check(ui, v, cs, "Hide bar lines", true);
            ui.label(egui::RichText::new("Hide bar lines hides the automatic bar lines; the final double bar line stays.").small().color(crate::look::text_dim()));
        }
        "Colors" => crate::scorecolors::pane(ui, v),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_numbers() {
        let mut v = Values::default();
        let f = fmt(&v);
        assert_eq!(f.page_text(0, "Song", "All", "Region"), Some("1".into()));
        // alternating: odd pages right, even pages left; reverse the other way round
        assert_eq!((f.page_side(0), f.page_side(1)), (2, 0));
        set(&mut v, "Score / Numbers & Names / Page Horizontal Position:", "Reverse Alternating");
        set(&mut v, "Score / Numbers & Names / Page Offset:", "4");
        set(&mut v, "Score / Numbers & Names / Prefix:", "\\n – page #");
        set(&mut v, "Score / Numbers & Names / Hide first page number", "1");
        let f = fmt(&v);
        assert_eq!(f.page_side(0), 0);
        assert_eq!(f.page_text(0, "Song", "", ""), None);
        assert_eq!(f.page_text(1, "Song", "", ""), Some("Song – page 6".into()));
    }

    #[test]
    fn bar_numbers_step_offset_start() {
        let mut v = Values::default();
        let f = fmt(&v);
        // step 0: line starts only, numbering from bar 2
        assert_eq!(f.bar_number(0, true), None);
        assert_eq!(f.bar_number(4, true), Some(5));
        assert_eq!(f.bar_number(5, false), None);
        set(&mut v, "Score / Numbers & Names / Step:", "4");
        set(&mut v, "Score / Numbers & Names / Start With:", "1");
        set(&mut v, "Score / Numbers & Names / Bar Offset:", "10");
        let f = fmt(&v);
        assert_eq!(f.bar_number(0, false), Some(11));
        assert_eq!(f.bar_number(4, false), Some(15));
        assert_eq!(f.bar_number(2, false), None);
    }

    #[test]
    fn margins_names_show() {
        let mut v = Values::default();
        set(&mut v, "Score / Global / Left Margin:", "2.00");
        set(&mut v, "Score / Global / Right Margin:", "1.00");
        set(&mut v, "Score / Global / Alternating margins", "1");
        let f = fmt(&v);
        assert!((f.margins(0).0 - 2.0 * SP_PER_CM).abs() < 1e-3);
        assert!((f.margins(1).0 - 1.0 * SP_PER_CM).abs() < 1e-3);
        set(&mut v, "Score / Numbers & Names / Instrument Names", "1");
        let f = fmt(&v);
        assert_eq!(f.name_for("Piano Grand", true), Some("Piano Grand".into()));
        assert_eq!(f.name_for("Violoncello", false), Some("Viol.".into()));
        assert!(Show::FirstEveryPage.on(true, false) && !Show::FirstEveryPage.on(false, false));
        assert!(Show::FirstPage1.on(true, true) && !Show::FirstPage1.on(true, false) && !Show::Hide.on(true, true));
        assert!(today().len() == 10);
    }
}
