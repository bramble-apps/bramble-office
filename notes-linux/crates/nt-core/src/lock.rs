//! Locked notes, encrypted the way Notes does it (format section 5; the scheme as
//! apple_cloud_notes_parser implements it, MIT, see THIRD-PARTY.md):
//!
//! - key-encrypting key = PBKDF2-HMAC-SHA256(password, ZCRYPTOSALT, ZCRYPTOITERATIONCOUNT, 16 bytes)
//! - each encrypted object (note, attachment, media, inline token) has its own random AES-128 key,
//!   stored RFC 3394-wrapped with that key in ZCRYPTOWRAPPEDKEY (older notes: ZCRYPTOVERIFIER)
//! - data = AES-128-GCM with a 16-byte IV and a separate 16-byte tag: the note body (ZICNOTEDATA.ZDATA,
//!   IV/tag beside it), an object's values (ZENCRYPTEDVALUESJSON: JSON such as `{"mergeableData": …}`,
//!   ZCRYPTOINITIALIZATIONVECTOR/ZCRYPTOTAG), media files (ZASSETCRYPTO*) and fallback images
//!   (ZFALLBACKIMAGECRYPTO*, ZFALLBACKPDFCRYPTO*)
//! - the account's ZCRYPTOVERIFIER is a key wrapped with the password's key: it unwraps only with the
//!   right password, which is how a password is checked before any note is touched.

use std::path::{Path, PathBuf};

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::generic_array::typenum::U16;
use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::aes::Aes128;
use aes_gcm::AesGcm;

use crate::model::Lock;

/// Apple's 16-byte-IV AES-128-GCM.
type Gcm = AesGcm<Aes128, U16>;

/// PBKDF2 rounds for new locks (what Notes writes).
pub const ITERATIONS: u32 = 20_000;

/// The password's key-encrypting key for `salt`.
pub fn derive(password: &str, salt: &[u8], iterations: u32) -> [u8; 16] {
    let mut k = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), salt, iterations.max(1), &mut k);
    k
}

/// RFC 3394 AES key wrap of a 16-byte key (24 bytes out).
pub fn wrap(kek: &[u8; 16], key: &[u8; 16]) -> Vec<u8> {
    let mut out = [0u8; 24];
    aes_kw::KekAes128::from(*kek).wrap(key, &mut out).expect("16-byte key wraps");
    out.to_vec()
}

/// RFC 3394 unwrap; None when the integrity check fails (wrong password).
pub fn unwrap(kek: &[u8; 16], wrapped: &[u8]) -> Option<[u8; 16]> {
    if wrapped.len() != 24 {
        return None;
    }
    let mut out = [0u8; 16];
    aes_kw::KekAes128::from(*kek).unwrap(wrapped, &mut out).ok()?;
    Some(out)
}

/// Decrypt and authenticate; None when the key, IV or tag don't match the data.
pub fn open(key: &[u8; 16], iv: &[u8], tag: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    if iv.len() != 16 || tag.len() != 16 {
        return None;
    }
    let mut buf = data.to_vec();
    Gcm::new(GenericArray::from_slice(key))
        .decrypt_in_place_detached(GenericArray::from_slice(iv), b"", &mut buf, GenericArray::from_slice(tag))
        .ok()?;
    Some(buf)
}

/// Encrypt with a fresh IV: (iv, tag, ciphertext).
pub fn seal(key: &[u8; 16], plain: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let iv: [u8; 16] = random();
    let mut buf = plain.to_vec();
    let tag = Gcm::new(GenericArray::from_slice(key))
        .encrypt_in_place_detached(GenericArray::from_slice(&iv), b"", &mut buf)
        .expect("AES-GCM encrypts");
    (iv.to_vec(), tag.to_vec(), buf)
}

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("system random numbers");
    b
}

impl Lock {
    /// A new lock for `password` with a fresh salt.
    pub fn new(password: &str, hint: Option<&str>) -> Lock {
        let salt: [u8; 16] = random();
        Lock::with_salt(password, &salt, ITERATIONS, hint)
    }

    pub fn with_salt(password: &str, salt: &[u8], iterations: u32, hint: Option<&str>) -> Lock {
        Lock { hint: hint.map(str::to_string), salt: salt.to_vec(), iterations, kek: derive(password, salt, iterations) }
    }

    /// A fresh object key and its wrapped form (for ZCRYPTOWRAPPEDKEY).
    pub fn new_key(&self) -> ([u8; 16], Vec<u8>) {
        let key: [u8; 16] = random();
        (key, wrap(&self.kek, &key))
    }

    /// Unwrap an object's key, when it was wrapped with this lock's salt and password.
    pub fn key(&self, salt: &[u8], iterations: u32, wrapped: &[u8]) -> Option<[u8; 16]> {
        if salt != self.salt.as_slice() || iterations != self.iterations {
            return None;
        }
        unwrap(&self.kek, wrapped)
    }

    /// Re-wrap an object key (changing the password keeps the data, only the wrapped keys change).
    pub(crate) fn rewrap(&self, to: &Lock, wrapped: &[u8]) -> Option<Vec<u8>> {
        unwrap(&self.kek, wrapped).map(|k| wrap(&to.kek, &k))
    }
}

/// The private folder decrypted attachment files of unlocked notes are written to (this process only,
/// readable by the user only). [`forget_unlocked`] removes it.
pub fn unlocked_dir() -> PathBuf {
    std::env::temp_dir().join(format!("bramble-notes-unlocked-{}", std::process::id()))
}

/// Remove every decrypted file (call when locked notes are locked again, and on quit).
pub fn forget_unlocked() {
    let _ = std::fs::remove_dir_all(unlocked_dir());
}

/// Write a decrypted file as unlocked_dir()/<object id>/<name> (owner-only permissions).
pub(crate) fn write_unlocked(object: &str, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let dir = unlocked_dir().join(object);
    private_dir(&unlocked_dir())?;
    private_dir(&dir)?;
    let safe: String = name.chars().map(|c| if c == '/' || c == '\\' || c == '\0' { '_' } else { c }).collect();
    let path = dir.join(if safe.is_empty() || safe == "." || safe == ".." { "file".to_string() } else { safe });
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    std::io::Write::write_all(&mut o.open(&path)?, bytes)?;
    Ok(path)
}

fn private_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 (the encoding of `mergeableData` in encrypted values).
pub(crate) fn base64(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                s.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

pub(crate) fn unbase64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}
