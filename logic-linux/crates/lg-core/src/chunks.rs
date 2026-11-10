//! Logic Pro's ProjectData container (Alternatives/<NNN>/ProjectData): a 24-byte header, then a flat
//! list of chunks to the end of the file. Little-endian throughout.
//!
//! Header: magic 23 47 C0 AB, ..., u32 at 0x10 = file length - 24.
//! Chunk: 36-byte header — tag[4] stored byte-reversed ("karT" = Trak), u16 version @4,
//! u16 @6, u32 id @8 (links a sequence's MSeq / Trak / EvSq), u64 payload length @28 — then the
//! payload. The next chunk starts at start + 36 + length. (Logic research, LOGICX-FORMAT.txt.)

pub const MAGIC: [u8; 4] = [0x23, 0x47, 0xC0, 0xAB];
const HEADER: usize = 24;
const CHUNK_HEADER: usize = 36;

#[derive(Debug)]
pub enum Error {
    NotProjectData,
    Truncated(usize),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotProjectData => write!(f, "not a Logic ProjectData file"),
            Error::Truncated(at) => write!(f, "the project data ends in the middle of a chunk (at byte {at})"),
        }
    }
}

impl std::error::Error for Error {}

/// One chunk: its tag (readable, e.g. "MSeq"), id, the header's u16 at 6, where it starts and
/// its payload.
#[derive(Clone, Debug)]
pub struct Chunk<'a> {
    pub tag: [u8; 4],
    pub version: u16,
    pub index: u16,
    pub id: u32,
    /// Offset of the chunk's header in the file (Song field offsets are counted from here).
    pub start: usize,
    /// The whole chunk, header included.
    pub raw: &'a [u8],
    pub payload: &'a [u8],
}

impl Chunk<'_> {
    pub fn is(&self, tag: &str) -> bool {
        self.tag == tag.as_bytes()[..4]
    }

    pub fn tag_str(&self) -> String {
        String::from_utf8_lossy(&self.tag).into_owned()
    }

    /// A u32 at `off` from the chunk's start (header included), as the research counts offsets.
    pub fn u32_at(&self, off: usize) -> Option<u32> {
        self.raw.get(off..off + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Walks the chunks; the walk must end exactly at the end of the file.
pub fn walk(data: &[u8]) -> Result<Vec<Chunk<'_>>, Error> {
    if data.len() < HEADER || data[..4] != MAGIC {
        return Err(Error::NotProjectData);
    }
    let mut out = Vec::new();
    let mut p = HEADER;
    while p < data.len() {
        if p + CHUNK_HEADER > data.len() {
            return Err(Error::Truncated(p));
        }
        let h = &data[p..p + CHUNK_HEADER];
        let len = u64::from_le_bytes(h[28..36].try_into().unwrap()) as usize;
        let end = p.checked_add(CHUNK_HEADER + len).filter(|e| *e <= data.len()).ok_or(Error::Truncated(p))?;
        let tag = [h[3], h[2], h[1], h[0]];
        out.push(Chunk {
            tag,
            version: u16::from_le_bytes([h[4], h[5]]),
            index: u16::from_le_bytes([h[6], h[7]]),
            id: u32::from_le_bytes(h[8..12].try_into().unwrap()),
            start: p,
            raw: &data[p..end],
            payload: &data[p + CHUNK_HEADER..end],
        });
        p = end;
    }
    Ok(out)
}

/// A name stored as u16 length + text at `off` of a payload (MSeq names, at 16).
pub fn short_string(payload: &[u8], off: usize) -> Option<String> {
    let n = u16::from_le_bytes(payload.get(off..off + 2)?.try_into().ok()?) as usize;
    let s = payload.get(off + 2..off + 2 + n)?;
    Some(String::from_utf8_lossy(s).trim_end_matches('\0').to_string())
}

/// The first run of at least `min` printable characters in `bytes` (Envi object names).
pub fn first_text(bytes: &[u8], min: usize) -> Option<String> {
    let mut start = None;
    for (i, &b) in bytes.iter().enumerate() {
        let printable = (0x20..0x7f).contains(&b);
        match (printable, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - s >= min {
                    return Some(String::from_utf8_lossy(&bytes[s..i]).into_owned());
                }
                start = None;
            }
            _ => {}
        }
    }
    start.filter(|s| bytes.len() - s >= min).map(|s| String::from_utf8_lossy(&bytes[s..]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(tag: &str, id: u32, payload: &[u8]) -> Vec<u8> {
        let mut h = vec![0u8; 36];
        let t: Vec<u8> = tag.bytes().rev().collect();
        h[..4].copy_from_slice(&t);
        h[8..12].copy_from_slice(&id.to_le_bytes());
        h[28..36].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        h.extend_from_slice(payload);
        h
    }

    #[test]
    fn walks_to_the_end() {
        let mut body = chunk("Song", 0, &[1, 2, 3]);
        body.extend(chunk("MSeq", 0x40000, &[0; 10]));
        let mut file = MAGIC.to_vec();
        file.extend_from_slice(&[0; 12]);
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&[0; 4]);
        file.extend(body);
        let c = walk(&file).unwrap();
        assert_eq!(c.len(), 2);
        assert!(c[0].is("Song") && c[1].is("MSeq"));
        assert_eq!(c[1].id, 0x40000);
        assert_eq!(c[1].payload.len(), 10);
        // cut short: an error, not a panic
        assert!(walk(&file[..file.len() - 3]).is_err());
        assert!(matches!(walk(b"nope"), Err(Error::NotProjectData)));
    }
}
