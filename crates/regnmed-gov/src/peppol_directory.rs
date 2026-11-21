//! Our own public Peppol lookup (docs/efaktura.md §3.2).
//!
//! Recommand answers `isValid: false` both for "not registered" and for
//! "the lookup failed". Choosing e-mail on the second would break the
//! e-invoice duty, so this lookup keeps three answers apart.
//!
//! DNS NAPTR is hand-rolled over UDP so the workspace gains no resolver
//! crate. shortcut: a truncated answer is `Unknown`, add TCP if real SML
//! records ever exceed a UDP datagram.

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use regnmed_core::peppol::DirectoryAnswer;
use sha2::{Digest, Sha256};
use tokio::net::UdpSocket;

const PARTICIPANT_SCHEME: &str = "iso6523-actorid-upis";
const DOCUMENT_SCHEME: &str = "busdox-docid-qns";
const SML_ZONE: &str = "participant.sml.prod.tech.peppol.org";
const DNS_TIMEOUT: Duration = Duration::from_secs(2);
const DNS_TRIES: usize = 2;
const SMP_TIMEOUT: Duration = Duration::from_secs(10);
const NAPTR: u16 = 35;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Sml {
    Found(String),
    Absent,
}

/// Whether `receiver` is published for `doctype` (without its scheme).
pub async fn lookup(receiver: &str, doctype: &str) -> DirectoryAnswer {
    let resolvers = match system_resolvers() {
        Ok(resolvers) => resolvers,
        Err(_) => return DirectoryAnswer::Unknown,
    };
    match query_sml(&naptr_name(receiver), &resolvers).await {
        Ok(Sml::Absent) => DirectoryAnswer::Absent,
        Ok(Sml::Found(smp)) if is_public_smp_url(&smp) => {
            document_support(&smp, receiver, doctype).await
        }
        _ => DirectoryAnswer::Unknown,
    }
}

fn naptr_name(receiver: &str) -> String {
    let hash = Sha256::digest(receiver.to_lowercase().as_bytes());
    format!("{}.{PARTICIPANT_SCHEME}.{SML_ZONE}", base32(&hash)).to_lowercase()
}

fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let (mut buffer, mut bits, mut out) = (0u32, 0, String::new());
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn system_resolvers() -> Result<Vec<SocketAddr>> {
    let text = std::fs::read_to_string("/etc/resolv.conf").context("reading resolv.conf")?;
    let resolvers: Vec<_> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("nameserver"))
        .filter_map(|ip| ip.trim().parse().ok())
        .map(|ip| SocketAddr::new(ip, 53))
        .collect();
    ensure!(!resolvers.is_empty(), "no nameserver configured");
    Ok(resolvers)
}

async fn query_sml(name: &str, resolvers: &[SocketAddr]) -> Result<Sml> {
    let mut last = None;
    for _ in 0..DNS_TRIES {
        for resolver in resolvers {
            match query_once(name, *resolver).await {
                Ok(answer) => return Ok(answer),
                Err(e) => last = Some(e),
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("no resolver answered")))
}

async fn query_once(name: &str, resolver: SocketAddr) -> Result<Sml> {
    let bind: SocketAddr = if resolver.is_ipv4() {
        "0.0.0.0:0".parse()?
    } else {
        "[::]:0".parse()?
    };
    let socket = UdpSocket::bind(bind).await?;
    // connect() makes the kernel drop datagrams from any other source.
    socket.connect(resolver).await?;
    let id = u16::from_be_bytes(uuid::Uuid::new_v4().as_bytes()[..2].try_into()?);
    let query = encode_query(id, name)?;
    socket.send(&query).await?;
    let mut buffer = [0u8; 1500];
    let size = tokio::time::timeout(DNS_TIMEOUT, socket.recv(&mut buffer)).await??;
    parse_response(&buffer[..size], id, &query[12..])
}

fn encode_query(id: u16, name: &str) -> Result<Vec<u8>> {
    let mut packet = Vec::with_capacity(name.len() + 18);
    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.split('.') {
        ensure!(!label.is_empty() && label.len() <= 63, "invalid DNS label");
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0);
    packet.extend_from_slice(&NAPTR.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    Ok(packet)
}

fn parse_response(packet: &[u8], id: u16, question: &[u8]) -> Result<Sml> {
    ensure!(packet.len() >= 12, "short DNS answer");
    ensure!(
        u16::from_be_bytes([packet[0], packet[1]]) == id,
        "wrong DNS id"
    );
    ensure!(packet[2] & 0x80 != 0, "not a DNS response");
    ensure!(packet[2] & 0x02 == 0, "truncated DNS answer");
    ensure!(
        u16::from_be_bytes([packet[4], packet[5]]) == 1,
        "unexpected question count"
    );
    ensure!(
        packet.get(12..12 + question.len()) == Some(question),
        "the answer is for another question"
    );
    match packet[3] & 0x0f {
        0 => {}
        3 => return Ok(Sml::Absent),
        code => bail!("DNS error {code}"),
    }
    let answers = u16::from_be_bytes([packet[6], packet[7]]);
    let mut offset = 12 + question.len();
    let mut best: Option<(u16, u16, String)> = None;
    let mut saw_naptr = false;
    for _ in 0..answers {
        offset = skip_name(packet, offset)?;
        let header = packet
            .get(offset..offset + 10)
            .context("short DNS record")?;
        let kind = u16::from_be_bytes([header[0], header[1]]);
        let length = usize::from(u16::from_be_bytes([header[8], header[9]]));
        offset += 10;
        let data = packet
            .get(offset..offset + length)
            .context("short DNS data")?;
        offset += length;
        if kind != NAPTR {
            continue;
        }
        saw_naptr = true;
        let (order, preference, service, regexp) = parse_naptr(data)?;
        if !service.to_ascii_lowercase().contains("meta:smp") {
            continue;
        }
        let url = naptr_url(&regexp)?;
        if best
            .as_ref()
            .is_none_or(|(o, p, _)| (order, preference) < (*o, *p))
        {
            best = Some((order, preference, url));
        }
    }
    match best {
        Some((_, _, url)) => Ok(Sml::Found(url)),
        // A record that names no SMP is not an answer that it does not exist.
        None if saw_naptr => bail!("NAPTR record without an SMP"),
        None => Ok(Sml::Absent),
    }
}

fn skip_name(packet: &[u8], mut offset: usize) -> Result<usize> {
    loop {
        let length = *packet.get(offset).context("short DNS name")?;
        match length {
            0 => return Ok(offset + 1),
            1..=63 => offset += usize::from(length) + 1,
            192..=255 => {
                packet.get(offset + 1).context("short DNS pointer")?;
                return Ok(offset + 2);
            }
            _ => bail!("invalid DNS name"),
        }
    }
}

fn parse_naptr(data: &[u8]) -> Result<(u16, u16, String, String)> {
    ensure!(data.len() >= 4, "short NAPTR");
    let order = u16::from_be_bytes([data[0], data[1]]);
    let preference = u16::from_be_bytes([data[2], data[3]]);
    let mut offset = 4;
    let mut strings = Vec::with_capacity(3);
    for _ in 0..3 {
        let length = usize::from(*data.get(offset).context("short NAPTR string")?);
        let value = data
            .get(offset + 1..offset + 1 + length)
            .context("short NAPTR string")?;
        strings.push(std::str::from_utf8(value)?.to_string());
        offset += 1 + length;
    }
    let regexp = strings.pop().unwrap_or_default();
    let service = strings.pop().unwrap_or_default();
    Ok((order, preference, service, regexp))
}

/// The SML publishes the SMP as `!.*!<url>!` (ELMA, seen live) or `!^.*$!<url>!`.
fn naptr_url(regexp: &str) -> Result<String> {
    let delimiter = regexp.chars().next().context("empty NAPTR regexp")?;
    let parts: Vec<_> = regexp.split(delimiter).collect();
    ensure!(
        parts.len() >= 3 && matches!(parts[1], ".*" | "^.*$"),
        "unsupported NAPTR regexp"
    );
    Ok(parts[2].trim_end_matches('/').to_string())
}

/// The URL comes from DNS, so a worker must not be pointed at itself.
/// shortcut: a public name resolving to a private IP is not caught; add a
/// resolved-address check if workers ever expose unauthenticated services.
fn is_public_smp_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    matches!(parsed.scheme(), "http" | "https")
        && host.contains('.')
        && host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_err()
        && !host.ends_with(".localhost")
        && !host.ends_with(".local")
        && !host.ends_with(".internal")
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

async fn document_support(smp: &str, receiver: &str, doctype: &str) -> DirectoryAnswer {
    let url = format!(
        "{smp}/{PARTICIPANT_SCHEME}::{}/services/{DOCUMENT_SCHEME}::{}",
        percent_encode(receiver),
        percent_encode(doctype)
    );
    let client = reqwest::Client::builder()
        .timeout(SMP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build();
    let Ok(client) = client else {
        return DirectoryAnswer::Unknown;
    };
    match client.get(url).send().await {
        Ok(response) if response.status().is_success() => DirectoryAnswer::Supported,
        Ok(response) if response.status() == reqwest::StatusCode::NOT_FOUND => {
            DirectoryAnswer::MissingDocumentType
        }
        _ => DirectoryAnswer::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::StatusCode, routing::get};

    #[test]
    fn base32_matches_rfc4648_without_padding() {
        assert_eq!(base32(b"f"), "MY");
        assert_eq!(base32(b"foo"), "MZXW6");
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn the_sml_name_hashes_the_lowercased_participant() {
        let name = naptr_name("0192:ABC");
        assert_eq!(name, naptr_name("0192:abc"));
        assert!(name.ends_with(".iso6523-actorid-upis.participant.sml.prod.tech.peppol.org"));
        assert_eq!(name.split('.').next().unwrap().len(), 52);
    }

    fn answer(rcode: u8, records: &[(u16, u16, &str, &str)]) -> (Vec<u8>, Vec<u8>) {
        let query = encode_query(7, "a.example").unwrap();
        let mut packet = query.clone();
        packet[2] = 0x81;
        packet[3] = 0x80 | rcode;
        packet[7] = records.len() as u8;
        for (order, preference, service, regexp) in records {
            packet.extend_from_slice(&[0xc0, 12]);
            packet.extend_from_slice(&NAPTR.to_be_bytes());
            packet.extend_from_slice(&[0, 1, 0, 0, 0, 60]);
            let mut data = Vec::new();
            data.extend_from_slice(&order.to_be_bytes());
            data.extend_from_slice(&preference.to_be_bytes());
            data.extend_from_slice(&[1, b'U']);
            data.push(service.len() as u8);
            data.extend_from_slice(service.as_bytes());
            data.push(regexp.len() as u8);
            data.extend_from_slice(regexp.as_bytes());
            data.push(0);
            packet.extend_from_slice(&(data.len() as u16).to_be_bytes());
            packet.extend_from_slice(&data);
        }
        (packet, query[12..].to_vec())
    }

    #[test]
    fn dns_keeps_absent_and_failed_apart() {
        let (nxdomain, question) = answer(3, &[]);
        assert_eq!(
            parse_response(&nxdomain, 7, &question).unwrap(),
            Sml::Absent
        );
        let (nodata, question) = answer(0, &[]);
        assert_eq!(parse_response(&nodata, 7, &question).unwrap(), Sml::Absent);
        let (servfail, question) = answer(2, &[]);
        assert!(parse_response(&servfail, 7, &question).is_err());
    }

    #[test]
    fn dns_rejects_a_forged_or_cut_answer() {
        let (packet, question) = answer(0, &[(1, 1, "Meta:SMP", "!^.*$!https://smp.example!")]);
        assert!(parse_response(&packet, 8, &question).is_err());
        assert!(parse_response(&packet[..packet.len() - 3], 7, &question).is_err());
        let mut truncated = packet.clone();
        truncated[2] |= 0x02;
        assert!(parse_response(&truncated, 7, &question).is_err());
    }

    #[test]
    fn the_best_smp_record_wins() {
        let (packet, question) = answer(
            0,
            &[
                (20, 1, "Meta:SMP", "!^.*$!https://second.example/!"),
                (10, 1, "Meta:SMP", "!^.*$!https://first.example/!"),
            ],
        );
        assert_eq!(
            parse_response(&packet, 7, &question).unwrap(),
            Sml::Found("https://first.example".into())
        );
    }

    #[test]
    fn the_live_elma_record_shape_is_understood() {
        assert_eq!(
            naptr_url("!.*!https://smp.elma-smp.no/!").unwrap(),
            "https://smp.elma-smp.no"
        );
        assert!(naptr_url("!^0192.*$!https://x.example/!").is_err());
    }

    /// The public network, so it only runs by hand: `cargo test -p
    /// regnmed-gov --lib live_elma -- --ignored`.
    #[tokio::test]
    #[ignore = "needs public DNS and ELMA"]
    async fn live_elma_lookup() {
        let doctype = crate::recommand::INVOICE_DOCTYPE
            .strip_prefix("busdox-docid-qns::")
            .unwrap();
        assert_eq!(
            lookup("0192:991825827", doctype).await,
            DirectoryAnswer::Supported
        );
        assert_eq!(
            lookup("0192:000000000", doctype).await,
            DirectoryAnswer::Absent
        );
    }

    #[test]
    fn a_record_without_an_smp_is_not_absence() {
        let (packet, question) = answer(0, &[(1, 1, "Other", "!^.*$!https://x.example!")]);
        assert!(parse_response(&packet, 7, &question).is_err());
    }

    #[test]
    fn dns_cannot_point_the_worker_at_internal_hosts() {
        assert!(is_public_smp_url("https://smp.example.org"));
        for url in [
            "http://localhost",
            "http://127.0.0.1",
            "http://[::1]",
            "http://regnmed-api",
            "http://db.internal",
            "ftp://smp.example.org",
        ] {
            assert!(!is_public_smp_url(url), "{url}");
        }
    }

    #[tokio::test]
    async fn smp_status_decides_the_answer() {
        let app = Router::new().fallback(get(|uri: axum::http::Uri| async move {
            let path = uri.path();
            assert!(path.contains("iso6523-actorid-upis::0192%3A1"), "{path}");
            if path.contains("yes") {
                StatusCode::OK
            } else if path.contains("no") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_GATEWAY
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert_eq!(
            document_support(&base, "0192:1", "yes").await,
            DirectoryAnswer::Supported
        );
        assert_eq!(
            document_support(&base, "0192:1", "no").await,
            DirectoryAnswer::MissingDocumentType
        );
        assert_eq!(
            document_support(&base, "0192:1", "fail").await,
            DirectoryAnswer::Unknown
        );
    }
}
