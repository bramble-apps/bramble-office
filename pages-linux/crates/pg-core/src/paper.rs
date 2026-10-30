//! Paper sizes of the Document inspector's paper pop-up (portrait points, Pages' paper ids).

/// (display name, paper id, width pt, height pt)
pub const PAPERS: &[(&str, &str, f32, f32)] = &[
    ("US Letter", "na-letter", 612.0, 792.0),
    ("US Legal", "na-legal", 612.0, 1008.0),
    ("Tabloid", "tabloid", 792.0, 1224.0),
    ("Statement", "na-invoice", 396.0, 612.0),
    ("A3", "iso-a3", 841.89, 1190.55),
    ("A4", "iso-a4", 595.28, 841.89),
    ("A5", "iso-a5", 419.53, 595.28),
    ("A6", "iso-a6", 297.64, 419.53),
    ("B5", "iso-b5", 498.9, 708.66),
    ("JIS B5", "jis-b5", 515.91, 728.5),
    ("Postcard", "jpn-hagaki", 283.46, 419.53),
    ("#10 Envelope", "na-number-10", 297.0, 684.0),
    ("DL Envelope", "iso-dl", 311.81, 623.62),
    ("C5 Envelope", "iso-c5", 459.21, 649.13),
];

/// The paper for an id, or the one whose size matches (either orientation).
pub fn find(id: &str, w: f32, h: f32) -> Option<&'static (&'static str, &'static str, f32, f32)> {
    PAPERS.iter().find(|p| p.1 == id).or_else(|| {
        PAPERS.iter().find(|p| ((p.2 - w).abs() < 1.0 && (p.3 - h).abs() < 1.0) || ((p.2 - h).abs() < 1.0 && (p.3 - w).abs() < 1.0))
    })
}

/// "21.59 × 27.94 cm" as the inspector shows under the paper pop-up.
pub fn dimension_text(w: f32, h: f32, unit: &str) -> String {
    match unit {
        "in" => format!("{:.2} × {:.2} in", w / 72.0, h / 72.0),
        "pt" => format!("{:.0} × {:.0} pt", w, h),
        "mm" => format!("{:.0} × {:.0} mm", w / 72.0 * 25.4, h / 72.0 * 25.4),
        _ => format!("{:.2} × {:.2} cm", w / 72.0 * 2.54, h / 72.0 * 2.54),
    }
}
