//! Protobuf wire format and IWA (Snappy) framing.

use crate::Error;

pub fn read_varint(buf: &[u8], pos: &mut usize) -> Result<u64, Error> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        let b = *buf.get(*pos).ok_or(Error::Corrupt("truncated varint"))?;
        *pos += 1;
        if shift < 64 {
            result |= u64::from(b & 0x7f) << shift;
        }
        if b & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
        if shift > 70 {
            return Err(Error::Corrupt("varint too long"));
        }
    }
}

pub fn write_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

pub fn varint_len(mut v: u64) -> usize {
    let mut n = 1;
    while v >= 0x80 {
        v >>= 7;
        n += 1;
    }
    n
}

pub fn packed_varints(b: &[u8]) -> Result<Vec<u64>, Error> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < b.len() {
        out.push(read_varint(b, &mut pos)?);
    }
    Ok(out)
}

/// An .iwa part compressed with Apple's LZFSE / LZVN ("bvx2" / "bvxn" / "bvx-" blocks) instead
/// of IWA's Snappy chunks (iWork 14+, e.g. the collaboration OperationStorage; the package's
/// component info marks it with compression_algorithm).
pub fn is_lzfse(raw: &[u8]) -> bool {
    raw.len() >= 4 && &raw[..3] == b"bvx" && matches!(raw[3], b'2' | b'n' | b'-' | b'1')
}

/// IWA framing: repeated `[0x00][u24 LE length][raw snappy block]` (not standard snappy framing, no CRC).
/// LZFSE-compressed parts are decoded too.
pub fn iwa_unframe(raw: &[u8]) -> Result<Vec<u8>, Error> {
    if is_lzfse(raw) {
        let mut plain = Vec::new();
        lzfse_rust::decode_bytes(raw, &mut plain).map_err(|e| Error::Invalid(format!("LZFSE: {e}")))?;
        return Ok(plain);
    }
    let mut out = Vec::with_capacity(raw.len() * 3);
    let mut pos = 0;
    let mut dec = snap::raw::Decoder::new();
    while pos < raw.len() {
        if raw[pos] != 0 {
            return Err(Error::Corrupt("unexpected IWA chunk type"));
        }
        if pos + 4 > raw.len() {
            return Err(Error::Corrupt("truncated IWA chunk header"));
        }
        let len = u32::from_le_bytes([raw[pos + 1], raw[pos + 2], raw[pos + 3], 0]) as usize;
        let block = raw.get(pos + 4..pos + 4 + len).ok_or(Error::Corrupt("truncated IWA chunk"))?;
        out.extend(dec.decompress_vec(block).map_err(|_| Error::Corrupt("bad snappy block"))?);
        pos += 4 + len;
    }
    Ok(out)
}

/// Inverse of [`iwa_unframe`]; chunks hold at most 64 KiB of uncompressed data, like Keynote's.
pub fn iwa_frame(data: &[u8]) -> Vec<u8> {
    const CHUNK: usize = 65536;
    let mut out = Vec::with_capacity(data.len() / 2 + 16);
    let mut enc = snap::raw::Encoder::new();
    let mut emit = |part: &[u8], out: &mut Vec<u8>| {
        let block = enc.compress_vec(part).expect("snappy compress");
        let n = block.len() as u32;
        out.push(0);
        out.extend_from_slice(&n.to_le_bytes()[..3]);
        out.extend(block);
    };
    if data.is_empty() {
        emit(&[], &mut out);
    }
    for part in data.chunks(CHUNK) {
        emit(part, &mut out);
    }
    out
}
