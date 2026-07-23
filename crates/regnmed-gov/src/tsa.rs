//! RFC 3161 timestamping: the external witness for anchor roots.
//!
//! A Time-Stamp Authority signs "this hash existed no later than time T"
//! with its own key — evidence outside our database and outside any
//! DBA's reach. We hand-roll the tiny fixed DER structures instead of
//! pulling in an ASN.1 stack (frugality): a `TimeStampReq` for SHA-256
//! is 59 bytes with only the digest varying, and on the response we only
//! need the PKIStatus before storing the whole DER token opaquely.
//!
//! The stored token is verifiable offline, by anyone, with standard
//! tooling (`openssl ts -reply`/`-verify` — see docs/anchoring.md). No
//! nonce is sent: every anchor root is unique, so replaying an old token
//! for it is impossible.
//!
//! Configuration: `ANCHOR_TSA_URL` (e.g. `https://freetsa.org/tsr`).
//! Unset means no external witness — anchoring still records snapshots
//! and publishes roots on `/anchors`.

use anyhow::{Context, Result, bail, ensure};

/// DER `TimeStampReq` for a SHA-256 digest: version 1, messageImprint
/// with the NIST SHA-256 OID (2.16.840.1.101.3.4.2.1), certReq TRUE so
/// the token embeds the TSA certificate and verifies offline.
pub fn timestamp_request_der(sha256: &[u8; 32]) -> Vec<u8> {
    let mut der = Vec::with_capacity(59);
    der.extend_from_slice(&[0x30, 0x39]); // TimeStampReq SEQUENCE, 57 bytes
    der.extend_from_slice(&[0x02, 0x01, 0x01]); // version INTEGER 1
    der.extend_from_slice(&[0x30, 0x31]); // MessageImprint SEQUENCE, 49 bytes
    der.extend_from_slice(&[
        0x30, 0x0d, // AlgorithmIdentifier SEQUENCE
        0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, // OID sha256
        0x05, 0x00, // parameters NULL
    ]);
    der.extend_from_slice(&[0x04, 0x20]); // hashedMessage OCTET STRING, 32 bytes
    der.extend_from_slice(sha256);
    der.extend_from_slice(&[0x01, 0x01, 0xff]); // certReq BOOLEAN TRUE
    der
}

/// PKIStatus from a DER `TimeStampResp`: 0 = granted, 1 = granted with
/// modifications; anything else is a rejection.
pub fn response_status(der: &[u8]) -> Result<u64> {
    // TimeStampResp ::= SEQUENCE { status PKIStatusInfo, token OPTIONAL }
    // PKIStatusInfo ::= SEQUENCE { status INTEGER, ... }
    let (_, outer) = der_header(der).context("TimeStampResp")?;
    let (_, status_info) = der_header(outer).context("PKIStatusInfo")?;
    let (tag, value) = der_header(status_info).context("PKIStatus")?;
    ensure!(tag == 0x02, "PKIStatus is not an INTEGER");
    ensure!(
        !value.is_empty() && value.len() <= 8,
        "PKIStatus INTEGER out of range"
    );
    Ok(value.iter().fold(0u64, |n, b| (n << 8) | u64::from(*b)))
}

/// Splits one DER TLV: returns (tag, contents). Only definite lengths —
/// which is all DER allows.
fn der_header(bytes: &[u8]) -> Result<(u8, &[u8])> {
    let (&tag, rest) = bytes.split_first().context("empty DER")?;
    let (&first, rest) = rest.split_first().context("missing DER length")?;
    let (len, rest) = if first < 0x80 {
        (first as usize, rest)
    } else {
        let n = (first & 0x7f) as usize;
        ensure!((1..=8).contains(&n) && rest.len() >= n, "bad DER length");
        let len = rest[..n].iter().fold(0usize, |l, b| (l << 8) | *b as usize);
        (len, &rest[n..])
    };
    ensure!(rest.len() >= len, "truncated DER");
    Ok((tag, &rest[..len]))
}

pub struct TsaClient {
    url: String,
    http: reqwest::Client,
}

impl TsaClient {
    /// `None` when `ANCHOR_TSA_URL` is unset — external witnessing is
    /// opt-in per environment.
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("ANCHOR_TSA_URL").ok()?;
        if url.is_empty() {
            return None;
        }
        Some(Self {
            url,
            http: reqwest::Client::new(),
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Requests a timestamp token for the digest and returns the full DER
    /// `TimeStampResp` — stored opaquely as witness proof.
    pub async fn timestamp(&self, sha256: &[u8; 32]) -> Result<Vec<u8>> {
        let response = self
            .http
            .post(&self.url)
            .header("content-type", "application/timestamp-query")
            .body(timestamp_request_der(sha256))
            .send()
            .await
            .with_context(|| format!("TSA {} unreachable", self.url))?;
        ensure!(
            response.status().is_success(),
            "TSA {} returned HTTP {}",
            self.url,
            response.status()
        );
        let der = response.bytes().await?.to_vec();
        match response_status(&der)? {
            0 | 1 => Ok(der),
            status => bail!("TSA {} rejected the request (PKIStatus {status})", self.url),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The request bytes are a frozen wire format: pin them so a refactor
    /// cannot silently emit DER that TSAs reject.
    #[test]
    fn golden_request_der() {
        let der = timestamp_request_der(&[0xAB; 32]);
        assert_eq!(der.len(), 59);
        let hex: String = der.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "30390201013031300d060960864801650304020105000420\
             abababababababababababababababababababababababababababababababab\
             0101ff"
        );
    }

    #[test]
    fn parses_granted_and_rejected_statuses() {
        // Minimal TimeStampResp: SEQUENCE { SEQUENCE { INTEGER 0 } }
        let granted = [0x30, 0x05, 0x30, 0x03, 0x02, 0x01, 0x00];
        assert_eq!(response_status(&granted).unwrap(), 0);
        let rejected = [0x30, 0x05, 0x30, 0x03, 0x02, 0x01, 0x02];
        assert_eq!(response_status(&rejected).unwrap(), 2);
        // Long-form outer length is legal DER framing for big tokens.
        let long_form = [0x30, 0x81, 0x05, 0x30, 0x03, 0x02, 0x01, 0x01];
        assert_eq!(response_status(&long_form).unwrap(), 1);
    }

    #[test]
    fn rejects_malformed_responses() {
        assert!(response_status(&[]).is_err());
        assert!(response_status(&[0x30, 0x02, 0x05, 0x00]).is_err());
        assert!(response_status(&[0x30, 0x7f, 0x30]).is_err()); // truncated
    }
}
