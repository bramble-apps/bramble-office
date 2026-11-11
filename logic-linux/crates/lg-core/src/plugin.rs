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
    /// The channel strip it's on (AuCU header u32 @12 >> 16) and its slot there (@16 >> 16: 2 the
    /// instrument, 3 on the Audio FX in order).
    pub channel: u32,
    pub slot: u32,
    /// Bypassed in the Mixer (the slot's power button).
    pub bypass: bool,
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
/// A project's plug-in floats (AuCU): "GAMETSPP", a u32 block id, a zero word, then the
/// parameters from 16 bytes after the tag (index = the parameter's id, as lg-fx and the
/// research maps number them).
pub fn params_after(b: &[u8], count: Option<usize>) -> Option<Vec<f32>> {
    params_at(b, count, 16)
}

/// A preset file's (.pst): the parameters 20 bytes after the tag (size and two zero words).
pub fn pst_params(b: &[u8], count: Option<usize>) -> Option<Vec<f32>> {
    params_at(b, count, 20)
}

fn params_at(b: &[u8], count: Option<usize>, offset: usize) -> Option<Vec<f32>> {
    let (at, big) = if let Some(i) = find(b, b"GAMETSPP") {
        (i, false)
    } else {
        (find(b, b"EMAGPPST")?, true)
    };
    let rd = |o: usize| -> Option<u32> {
        let x: [u8; 4] = b.get(o..o + 4)?.try_into().ok()?;
        Some(if big { u32::from_be_bytes(x) } else { u32::from_le_bytes(x) })
    };
    let start = at + offset;
    // (a preset gives its size at +8; in a project that word is the plug-in's code, and the
    // floats run on to the end of the chunk)
    let n = count.unwrap_or(if offset == 20 { (rd(at + 8)? as usize).saturating_sub(8) / 4 } else { (b.len().saturating_sub(start) / 4).min(1024) });
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

/// A plug-in from an AuCU chunk's payload: its display name (a NUL-ended field at 0x78, Logic's
/// short form, e.g. "Space D"), its preset and, when it has one, its parameter block (Logic's own
/// plug-ins; instruments such as Drum Kit Designer keep their state another way). None for units
/// that aren't plug-ins.
pub fn read_unit(payload: &[u8]) -> Option<Plugin> {
    let field = payload.get(0x78..0x98).map(|f| f.split(|b| *b == 0).next().unwrap_or(&[]))?;
    if field.len() < 2 || !field.iter().all(|b| (0x20..0x7f).contains(b)) {
        return None;
    }
    let name = String::from_utf8_lossy(field).into_owned();
    let at = find(payload, b"GAMETSPP").or_else(|| find(payload, b"EMAGPPST"));
    let params = params_after(payload, None).unwrap_or_default();
    let runs = printable_runs(&payload[..at.unwrap_or(payload.len().min(0x78))], 3);
    let preset = runs.iter().find(|r| r.ends_with(".pst")).map(|r| r.rfind('#').map_or(r.as_str(), |i| &r[i..]).to_string()).unwrap_or_default();
    Some(Plugin { name, preset, params, channel: 0, slot: 0, bypass: false })
}

/// A .pst preset file: u32 file size, u32 1, u32 parameter count, then the parameter block.
pub fn read_pst(bytes: &[u8]) -> Option<Vec<f32>> {
    let count = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?) as usize;
    let count = if count > 0 && count < 10_000 { Some(count) } else { None };
    pst_params(bytes, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_parameter_block() {
        let mut b = vec![0u8; 0x98];
        b[0x0C..0x18].copy_from_slice(b"#default.pst");
        b[0x78..0x82].copy_from_slice(b"Channel EQ");
        b.extend_from_slice(b"GAMETSPP");
        b.extend_from_slice(&236u32.to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        for v in [20.0f32, 2.0, 0.71] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let p = read_unit(&b).unwrap();
        assert_eq!(p.name, "Channel EQ");
        assert_eq!(p.preset, "#default.pst");
        assert_eq!(p.params, vec![20.0, 2.0, 0.71]);
    }
}
