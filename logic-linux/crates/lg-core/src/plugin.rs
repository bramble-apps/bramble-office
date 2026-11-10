//! Plug-in settings: an AuCU chunk (a plug-in in a channel strip) and a .pst preset file share
//! one parameter block — "GAMETSPP" (little-endian, newer) or "EMAGPPST" (big-endian, older),
//! a u32 size, two u32 header fields, then one float per parameter, in the plug-in's own order.
//! AuCU also holds the preset name ("#default.pst"), the plug-in's display name ("Channel EQ")
//! and its maker tag ("GAME" = Logic's own).

#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    pub name: String,
    pub preset: String,
    pub params: Vec<f32>,
}

fn printable_runs(b: &[u8], min: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut s = None;
    for (i, &c) in b.iter().enumerate() {
        let ok = (0x20..0x7f).contains(&c);
        match (ok, s) {
            (true, None) => s = Some(i),
            (false, Some(a)) => {
                if i - a >= min {
                    out.push(String::from_utf8_lossy(&b[a..i]).into_owned());
                }
                s = None;
            }
            _ => {}
        }
    }
    out
}

/// The float parameters after a GAMETSPP / EMAGPPST tag (`count` given, or up to the block's size).
pub fn params_after(b: &[u8], count: Option<usize>) -> Option<Vec<f32>> {
    let (at, big) = if let Some(i) = find(b, b"GAMETSPP") {
        (i, false)
    } else {
        (find(b, b"EMAGPPST")?, true)
    };
    let rd = |o: usize| -> Option<u32> {
        let x: [u8; 4] = b.get(o..o + 4)?.try_into().ok()?;
        Some(if big { u32::from_be_bytes(x) } else { u32::from_le_bytes(x) })
    };
    let size = rd(at + 8)? as usize;
    let start = at + 8 + 12;
    let n = count.unwrap_or(size.saturating_sub(8) / 4);
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let Some(v) = rd(start + 4 * k) else { break };
        out.push(f32::from_bits(v));
    }
    Some(out)
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// A plug-in from an AuCU chunk's payload (None for units without a parameter block).
pub fn read_unit(payload: &[u8]) -> Option<Plugin> {
    let params = params_after(payload, None)?;
    let at = find(payload, b"GAMETSPP").or_else(|| find(payload, b"EMAGPPST"))?;
    let runs = printable_runs(&payload[..at], 3);
    // (the preset field: "#default.pst", or a factory preset's name)
    let preset = runs.iter().find(|r| r.ends_with(".pst")).map(|r| r.rfind('#').map_or(r.as_str(), |i| &r[i..]).to_string()).unwrap_or_default();
    // the display name: a NUL-ended field at 0x78 (Logic's own short form, e.g. "Space D"); other
    // units keep other data there
    let field = payload.get(0x78..0x98).map(|f| f.split(|b| *b == 0).next().unwrap_or(&[]));
    let name = match field {
        Some(f) if f.len() >= 2 && f.iter().all(|b| (0x20..0x7f).contains(b)) => String::from_utf8_lossy(f).into_owned(),
        _ => runs.iter().rev().find(|r| !r.ends_with(".pst") && r.as_str() != "GAME" && r.len() > 3 && r.chars().all(|c| c.is_ascii_alphanumeric() || " -_/.".contains(c))).cloned().unwrap_or_default(),
    };
    Some(Plugin { name, preset, params })
}

/// A .pst preset file: u32 file size, u32 1, u32 parameter count, then the parameter block.
pub fn read_pst(bytes: &[u8]) -> Option<Vec<f32>> {
    let count = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?) as usize;
    let count = if count > 0 && count < 10_000 { Some(count) } else { None };
    params_after(bytes, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_parameter_block() {
        let mut b = Vec::new();
        b.extend_from_slice(b"..#default.pst\0\0Channel EQ\0\0GAME\0");
        b.extend_from_slice(b"GAMETSPP");
        b.extend_from_slice(&(8u32 + 12).to_le_bytes());
        b.extend_from_slice(&[0; 8]);
        for v in [20.0f32, 2.0, 0.71] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let p = read_unit(&b).unwrap();
        assert_eq!(p.name, "Channel EQ");
        assert_eq!(p.preset, "#default.pst");
        assert_eq!(p.params, vec![20.0, 2.0, 0.71]);
    }
}
