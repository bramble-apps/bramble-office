//! A hosted plug-in's settings in Studio's float block (the slot's `params`, what Copy / Paste,
//! Compare, undo, the Plug-in Presets and Studio's own project settings keep): the parameter
//! values by Studio id, then a marker and the plug-in's own state blob, three bytes to a float
//! (whole numbers below 2^24 are exact in an f32).

/// Marks where the state starts (a float no parameter value is: f32::MAX's neighbour).
pub const MARK: u32 = 0x7F7F_FFFE;

/// Values, then the state when there is one.
pub fn encode(values: &[f32], blob: Option<&[u8]>) -> Vec<f32> {
    let mut v = values.to_vec();
    if let Some(b) = blob {
        v.reserve(3 + b.len().div_ceil(3));
        v.push(f32::from_bits(MARK));
        let n = b.len() as u64;
        v.push((n & 0xFF_FFFF) as f32);
        v.push(((n >> 24) & 0xFF_FFFF) as f32);
        for c in b.chunks(3) {
            let mut x = 0u32;
            for (k, byte) in c.iter().enumerate() {
                x |= (*byte as u32) << (8 * k);
            }
            v.push(x as f32);
        }
    }
    v
}

/// The values and the state (None when the floats carry none, or it is damaged).
pub fn decode(floats: &[f32]) -> (Vec<f32>, Option<Vec<u8>>) {
    let Some(at) = floats.iter().position(|f| f.to_bits() == MARK) else { return (floats.to_vec(), None) };
    let values = floats[..at].to_vec();
    let rest = &floats[at + 1..];
    if rest.len() < 2 {
        return (values, None);
    }
    let n = (rest[0] as u64 | ((rest[1] as u64) << 24)) as usize;
    let packed = &rest[2..];
    if packed.len() < n.div_ceil(3) {
        return (values, None);
    }
    let mut b = Vec::with_capacity(n);
    for f in packed {
        let x = *f as u32;
        for k in 0..3 {
            if b.len() < n {
                b.push((x >> (8 * k)) as u8);
            }
        }
    }
    (values, Some(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let blob: Vec<u8> = (0..1000u32).map(|i| (i * 7 + 3) as u8).collect();
        for len in [0usize, 1, 2, 3, 4, 999, 1000] {
            let f = encode(&[0.5, -3.0, 12000.0], Some(&blob[..len]));
            let (v, b) = decode(&f);
            assert_eq!(v, [0.5, -3.0, 12000.0]);
            assert_eq!(b.as_deref(), Some(&blob[..len]));
        }
        assert_eq!(decode(&[1.0, 2.0]), (vec![1.0, 2.0], None));
    }
}
