//! Settings ▸ View ▸ General ▸ Display MIDI Data As: how MIDI values (velocity, controller and
//! aftertouch values) read in the Event List: MIDI 1.0 (0 … 127), 0.0 - 127.9 (the MIDI 1.0
//! value before the point, the finer resolution after it) or Percent (0% … 100%).

pub const KEY: &str = "View / General / Display MIDI Data As:";

/// A 7-bit value as the setting shows it.
pub fn value_text(mode: &str, v: f32) -> String {
    match mode {
        "0.0 - 127.9" => format!("{v:.1}"),
        "Percent" => {
            let pc = v / 127.0 * 100.0;
            // (whole percents where they are exact, else one decimal)
            if (pc - pc.round()).abs() < 0.05 { format!("{pc:.0}%") } else { format!("{pc:.1}%") }
        }
        _ => format!("{v:.0}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_ways() {
        assert_eq!(value_text("MIDI 1.0", 90.0), "90");
        assert_eq!(value_text("0.0 - 127.9", 90.0), "90.0");
        assert_eq!(value_text("Percent", 127.0), "100%");
        assert_eq!(value_text("Percent", 0.0), "0%");
        assert_eq!(value_text("Percent", 64.0), "50.4%");
    }
}
