//! Password-protected presentations (File > Set Password…), as Keynote writes them (verified
//! on the Mac): `.iwpv2` = u16 2 | u16 1 | u32 iterations | salt[16] | iv[16] | verifier[64],
//! key = PBKDF2-HMAC-SHA1(password, salt, iterations, 16); the verifier decrypts (AES-128-CBC)
//! to R[32] || SHA-256(R). Every Index/ and Data/ entry is iv[16] | AES-128-CBC(16 throwaway
//! bytes + data, PKCS7) | 20 trailer bytes (unused when reading). `.iwph` holds the hint.

use aes::cipher::{block_padding::NoPadding, block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use sha2::Digest;

type Dec = cbc::Decryptor<aes::Aes128>;
type Enc = cbc::Encryptor<aes::Aes128>;

pub const VERIFIER: &str = ".iwpv2";
pub const HINT: &str = ".iwph";

fn random(n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n + 16);
    while out.len() < n {
        out.extend(uuid::Uuid::new_v4().as_bytes());
    }
    out.truncate(n);
    out
}

pub fn derive_key(password: &str, salt: &[u8], iterations: u32) -> [u8; 16] {
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password.as_bytes(), salt, iterations, &mut key);
    key
}

/// The key, if `password` is right for this verifier.
pub fn check(verifier: &[u8], password: &str) -> Option<[u8; 16]> {
    if verifier.len() < 104 {
        return None;
    }
    let iterations = u32::from_le_bytes(verifier[4..8].try_into().ok()?);
    let (salt, iv, enc) = (&verifier[8..24], &verifier[24..40], &verifier[40..104]);
    let key = derive_key(password, salt, iterations);
    let mut buf = enc.to_vec();
    let plain = Dec::new_from_slices(&key, iv).ok()?.decrypt_padded_mut::<NoPadding>(&mut buf).ok()?;
    let digest = sha2::Sha256::digest(&plain[..32]);
    (digest.as_slice() == &plain[32..64]).then_some(key)
}

/// A new verifier for `password`, and its key.
pub fn make_verifier(password: &str) -> (Vec<u8>, [u8; 16]) {
    let iterations = 100_000u32;
    let salt = random(16);
    let iv = random(16);
    let key = derive_key(password, &salt, iterations);
    let r = random(32);
    let mut plain = r.clone();
    plain.extend(sha2::Sha256::digest(&r));
    let enc = Enc::new_from_slices(&key, &iv).unwrap().encrypt_padded_vec_mut::<NoPadding>(&plain);
    let mut out = Vec::with_capacity(104);
    out.extend(2u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(iterations.to_le_bytes());
    out.extend(&salt);
    out.extend(&iv);
    out.extend(enc);
    (out, key)
}

pub fn decrypt(key: &[u8; 16], data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 16 + 16 + 20 {
        return None;
    }
    let (iv, rest) = data.split_at(16);
    let ct = &rest[..rest.len() - 20];
    let mut buf = ct.to_vec();
    let plain = Dec::new_from_slices(key, iv).ok()?.decrypt_padded_mut::<Pkcs7>(&mut buf).ok()?;
    Some(plain.get(16..)?.to_vec())
}

pub fn encrypt(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let iv = random(16);
    let mut plain = random(16);
    plain.extend_from_slice(data);
    let ct = Enc::new_from_slices(key, &iv).unwrap().encrypt_padded_vec_mut::<Pkcs7>(&plain);
    let mut out = iv;
    out.extend(ct);
    out.extend(random(20));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let (v, key) = make_verifier("test");
        assert_eq!(check(&v, "test"), Some(key));
        assert!(check(&v, "nope").is_none());
        let data = b"hello keynote".to_vec();
        assert_eq!(decrypt(&key, &encrypt(&key, &data)).unwrap(), data);
    }
}
