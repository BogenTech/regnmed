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
pub struct Nokkel(chacha20poly1305::Key);

/// Redacting on purpose: a key that can be printed ends up in a log the
/// day someone adds `{:?}` to an error path.
impl std::fmt::Debug for Nokkel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Nokkel(skjult)")
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum HemmeligError {
    #[error("nøkkelen må være 64 hex-tegn (32 byte), ikke {0}")]
    Nokkellengde(usize),
    #[error("nøkkelen er ikke gyldig hex")]
    Nokkelformat,
    #[error("den forseglede verdien er for kort til å inneholde en nonce")]
    ForKort,
    #[error("den forseglede verdien lot seg ikke åpne — feil nøkkel, eller endret innhold")]
    Apning,
}

/// The nonce sits in front of the ciphertext; XChaCha20's is 24 bytes.
const NONCE: usize = 24;

impl Nokkel {
    /// Reads the key from 64 hex characters, which is how it travels as
    /// configuration. Rejects anything else loudly: a key quietly
    /// truncated or padded would seal rows nothing can open later.
    pub fn fra_hex(hex: &str) -> Result<Self, HemmeligError> {
        let hex = hex.trim();
        if hex.len() != 64 {
            return Err(HemmeligError::Nokkellengde(hex.len()));
        }
        let mut bytes = [0u8; 32];
        for (i, par) in hex.as_bytes().chunks(2).enumerate() {
            let s = std::str::from_utf8(par).map_err(|_| HemmeligError::Nokkelformat)?;
            bytes[i] = u8::from_str_radix(s, 16).map_err(|_| HemmeligError::Nokkelformat)?;
        }
        Ok(Nokkel(bytes.into()))
    }
}

/// Seals a secret. Every call draws a fresh nonce, so sealing the same
/// key twice gives two different rows — an observer with database access
/// cannot tell that two companies configured the same provider key.
pub fn forsegl(nokkel: &Nokkel, klartekst: &[u8]) -> Result<Vec<u8>, HemmeligError> {
    let chiffer = XChaCha20Poly1305::new(&nokkel.0);
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let mut ut = nonce.to_vec();
    ut.extend(
        chiffer
            .encrypt(&nonce, klartekst)
            .map_err(|_| HemmeligError::Apning)?,
    );
    Ok(ut)
}

/// Opens a sealed secret. Fails rather than returning anything when the
/// key is wrong or the bytes were changed — that is the whole point of
/// an authenticated cipher, and it means a tampered row is visible as an
/// error instead of as a nonsense API key sent to a provider.
pub fn apne(nokkel: &Nokkel, forseglet: &[u8]) -> Result<Vec<u8>, HemmeligError> {
    if forseglet.len() <= NONCE {
        return Err(HemmeligError::ForKort);
    }
    let (nonce, chiffertekst) = forseglet.split_at(NONCE);
    XChaCha20Poly1305::new(&nokkel.0)
        .decrypt(XNonce::from_slice(nonce), chiffertekst)
        .map_err(|_| HemmeligError::Apning)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    const B: &str = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";

    #[test]
    fn a_sealed_secret_comes_back_exactly() {
        let n = Nokkel::fra_hex(A).unwrap();
        let hemmelig = b"sk-proj-aldri-i-et-repo";
        let forseglet = forsegl(&n, hemmelig).unwrap();
        assert_ne!(forseglet.as_slice(), hemmelig, "klartekst i databasen");
        assert_eq!(apne(&n, &forseglet).unwrap(), hemmelig);
    }

    #[test]
    fn the_same_secret_seals_differently_every_time() {
        let n = Nokkel::fra_hex(A).unwrap();
        let (en, to) = (
            forsegl(&n, b"samme").unwrap(),
            forsegl(&n, b"samme").unwrap(),
        );
        assert_ne!(en, to, "nonce gjenbrukt — to like rader røper likheten");
        assert_eq!(apne(&n, &en).unwrap(), apne(&n, &to).unwrap());
    }

    #[test]
    fn another_key_cannot_open_it() {
        let forseglet = forsegl(&Nokkel::fra_hex(A).unwrap(), b"hemmelig").unwrap();
        assert_eq!(
            apne(&Nokkel::fra_hex(B).unwrap(), &forseglet),
            Err(HemmeligError::Apning)
        );
    }

    #[test]
    fn a_changed_row_fails_instead_of_decrypting_to_something_else() {
        let n = Nokkel::fra_hex(A).unwrap();
        let mut forseglet = forsegl(&n, b"hemmelig").unwrap();
        let siste = forseglet.len() - 1;
        forseglet[siste] ^= 1;
        assert_eq!(apne(&n, &forseglet), Err(HemmeligError::Apning));

        // A row truncated to less than a nonce says so rather than
        // panicking on the split.
        assert_eq!(apne(&n, &[0u8; NONCE]), Err(HemmeligError::ForKort));
    }

    #[test]
    fn the_key_does_not_print_itself() {
        let n = Nokkel::fra_hex(A).unwrap();
        assert_eq!(format!("{n:?}"), "Nokkel(skjult)");
        assert!(!format!("{n:?}").contains("0011"));
    }

    #[test]
    fn a_key_of_the_wrong_shape_is_refused_loudly() {
        assert_eq!(
            Nokkel::fra_hex("abc").unwrap_err(),
            HemmeligError::Nokkellengde(3)
        );
        assert_eq!(
            Nokkel::fra_hex(&"z".repeat(64)).unwrap_err(),
            HemmeligError::Nokkelformat
        );
        // Whitespace from a config file must not change the key.
        assert!(Nokkel::fra_hex(&format!("  {A}\n")).is_ok());
    }
}
