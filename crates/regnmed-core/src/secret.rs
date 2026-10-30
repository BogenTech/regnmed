//! Sealing a customer's own secret at rest (docs/agent.md §8).
//!
//! A hosted agent calls the customer's LLM provider with the customer's
//! key. Storing that key is a new responsibility for us -- every other
//! secret in this system is an environment variable on a process
//! (docs/secrets.md), never a row -- so it is sealed before it reaches
//! the database and the sealing key never lives there.
//!
//! XChaCha20-Poly1305: authenticated, so a row somebody edited in the
//! database fails to open rather than decrypting to something else, and
//! a 192-bit nonce can be random per sealing without a counter to keep.
//! The nonce is not secret and is stored in front of the ciphertext.
//!
//! This is the one place in the workspace that does not hand-roll its
//! format. PDF, EHF, pain.001 and SAF-T are specifications we must hit
//! exactly and a dependency would not make them more correct; an AEAD
//! is the opposite case, and the same judgement is why `hash.rs` uses
//! sha2 rather than its own SHA-256.

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

/// The sealing key, 32 bytes. Held by the process, never stored.
#[derive(Clone)]
pub struct SealingKey(chacha20poly1305::Key);

/// Redacting on purpose: a key that can be printed ends up in a log the
/// day someone adds `{:?}` to an error path.
impl std::fmt::Debug for SealingKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SealingKey(redacted)")
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SecretError {
    #[error("the key must be 64 hex characters (32 bytes), not {0}")]
    KeyLength(usize),
    #[error("the key is not valid hex")]
    KeyFormat,
    #[error("the sealed value is too short to contain a nonce")]
    TooShort,
    #[error("the sealed value could not be opened — wrong key, or changed content")]
    CouldNotOpen,
}

/// The nonce sits in front of the ciphertext; XChaCha20's is 24 bytes.
const NONCE: usize = 24;

impl SealingKey {
    /// Reads the key from 64 hex characters, which is how it travels as
    /// configuration. Rejects anything else loudly: a key quietly
    /// truncated or padded would seal rows nothing can open later.
    pub fn from_hex(hex: &str) -> Result<Self, SecretError> {
        let hex = hex.trim();
        if hex.len() != 64 {
            return Err(SecretError::KeyLength(hex.len()));
        }
        let mut bytes = [0u8; 32];
        for (i, pair) in hex.as_bytes().chunks(2).enumerate() {
            let s = std::str::from_utf8(pair).map_err(|_| SecretError::KeyFormat)?;
            bytes[i] = u8::from_str_radix(s, 16).map_err(|_| SecretError::KeyFormat)?;
        }
        Ok(SealingKey(bytes.into()))
    }
}

/// Seals a secret. Every call draws a fresh nonce, so sealing the same
/// key twice gives two different rows — an observer with database access
/// cannot tell that two companies configured the same provider key.
pub fn seal(key: &SealingKey, plaintext: &[u8]) -> Result<Vec<u8>, SecretError> {
    let cipher = XChaCha20Poly1305::new(&key.0);
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let mut out = nonce.to_vec();
    out.extend(
        cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| SecretError::CouldNotOpen)?,
    );
    Ok(out)
}

/// Opens a sealed secret. Fails rather than returning anything when the
/// key is wrong or the bytes were changed — that is the whole point of
/// an authenticated cipher, and it means a tampered row is visible as an
/// error instead of as a nonsense API key sent to a provider.
pub fn open(key: &SealingKey, sealed: &[u8]) -> Result<Vec<u8>, SecretError> {
    if sealed.len() <= NONCE {
        return Err(SecretError::TooShort);
    }
    let (nonce, ciphertext) = sealed.split_at(NONCE);
    XChaCha20Poly1305::new(&key.0)
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| SecretError::CouldNotOpen)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    const B: &str = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";

    #[test]
    fn a_sealed_secret_comes_back_exactly() {
        let n = SealingKey::from_hex(A).unwrap();
        let secret = b"sk-proj-never-in-a-repo";
        let sealed = seal(&n, secret).unwrap();
        assert_ne!(sealed.as_slice(), secret, "plaintext in the database");
        assert_eq!(open(&n, &sealed).unwrap(), secret);
    }

    #[test]
    fn the_same_secret_seals_differently_every_time() {
        let n = SealingKey::from_hex(A).unwrap();
        let (one, two) = (seal(&n, b"same").unwrap(), seal(&n, b"same").unwrap());
        assert_ne!(
            one, two,
            "nonce reused — two identical rows give the sameness away"
        );
        assert_eq!(open(&n, &one).unwrap(), open(&n, &two).unwrap());
    }

    #[test]
    fn another_key_cannot_open_it() {
        let sealed = seal(&SealingKey::from_hex(A).unwrap(), b"secret").unwrap();
        assert_eq!(
            open(&SealingKey::from_hex(B).unwrap(), &sealed),
            Err(SecretError::CouldNotOpen)
        );
    }

    #[test]
    fn a_changed_row_fails_instead_of_decrypting_to_something_else() {
        let n = SealingKey::from_hex(A).unwrap();
        let mut sealed = seal(&n, b"secret").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert_eq!(open(&n, &sealed), Err(SecretError::CouldNotOpen));

        // A row truncated to less than a nonce says so rather than
        // panicking on the split.
        assert_eq!(open(&n, &[0u8; NONCE]), Err(SecretError::TooShort));
    }

    #[test]
    fn the_key_does_not_print_itself() {
        let n = SealingKey::from_hex(A).unwrap();
        assert_eq!(format!("{n:?}"), "SealingKey(redacted)");
        assert!(!format!("{n:?}").contains("0011"));
    }

    #[test]
    fn a_key_of_the_wrong_shape_is_refused_loudly() {
        assert_eq!(
            SealingKey::from_hex("abc").unwrap_err(),
            SecretError::KeyLength(3)
        );
        assert_eq!(
            SealingKey::from_hex(&"z".repeat(64)).unwrap_err(),
            SecretError::KeyFormat
        );
        // Whitespace from a config file must not change the key.
        assert!(SealingKey::from_hex(&format!("  {A}\n")).is_ok());
    }
}
