//! Inbound e-mail: the address and the sender rule (docs/epost-inn.md, #35).
//!
//! Two pure decisions that must come out the same every time, and
//! therefore live here rather than in a query: how a receiving address is
//! formed, and whether a sender is on the company's list.

/// Local-part of a company's inbound address: `bilag-<navn>-<tilfeldig>`.
///
/// The name part is there for the humans (the address should be readable
/// over the phone); the random tail is there because the address is a
/// **capability**: whoever knows it can deliver something into the
/// innboks, so it must not be guessable from the company name.
pub fn local_part(firmanavn: &str, tilfeldig: &str) -> String {
    let slug: String = firmanavn
        .to_lowercase()
        .replace('ø', "o")
        .replace('æ', "ae")
        .replace('å', "a")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug: String = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join("-");
    let slug: String = slug.chars().take(24).collect();
    let slug = slug.trim_matches('-');
    let tail: String = tilfeldig
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .take(10)
        .collect();
    if slug.is_empty() {
        format!("bilag-{tail}")
    } else {
        format!("bilag-{slug}-{tail}")
    }
}

/// Normalizes an address for comparison: `"Ola <POST@Grossisten.no>"` →
/// `"post@grossisten.no"`. Display names and angle brackets are how
/// real mail arrives; the comparison must not care.
pub fn normaliser_avsender(raw: &str) -> String {
    let inner = match (raw.rfind('<'), raw.rfind('>')) {
        (Some(start), Some(end)) if end > start => &raw[start + 1..end],
        _ => raw,
    };
    inner.trim().to_lowercase()
}

/// Whether a sender is on the company's allow-list. An entry is either
/// a full address (`post@grossisten.no`) or a whole domain
/// (`@grossisten.no`) — nothing else, because a wildcard nobody can
/// read is a security hole with a friendly face.
pub fn avsender_tillatt(avsender: &str, liste: &[String]) -> bool {
    let avsender = normaliser_avsender(avsender);
    let domene = avsender.rfind('@').map(|at| &avsender[at..]);
    liste.iter().any(|entry| {
        let entry = entry.trim().to_lowercase();
        if let Some(entry_domain) = entry.strip_prefix('@') {
            domene.is_some_and(|d| d == format!("@{entry_domain}"))
        } else {
            entry == avsender
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_is_readable_but_not_guessable() {
        let a = local_part("Purredemo AS", "a1b2c3d4e5");
        assert_eq!(a, "bilag-purredemo-as-a1b2c3d4e5");
        // Norwegian characters and punctuation survive as something a
        // godtar.
        assert_eq!(
            local_part("Bråten & Sønn AS", "xyz123"),
            "bilag-braten-sonn-as-xyz123"
        );
        assert!(local_part("", "abc123").starts_with("bilag-"));
    }

    #[test]
    fn adressen_holder_seg_innenfor_lengden_skjemaet_krever() {
        let a = local_part(
            "Det Aller Lengste Firmanavnet I Hele Kongeriket Norge AS",
            "abcdefghij",
        );
        assert!(a.len() <= 63, "{a} ({} tegn)", a.len());
        assert!(
            a.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        );
    }

    #[test]
    fn the_sender_is_read_out_of_a_display_name() {
        assert_eq!(
            normaliser_avsender("Ola Nordmann <POST@Grossisten.no>"),
            "post@grossisten.no"
        );
        assert_eq!(
            normaliser_avsender(" post@grossisten.no "),
            "post@grossisten.no"
        );
    }

    #[test]
    fn the_list_accepts_an_address_and_a_domain() {
        let liste = vec![
            "post@grossisten.no".to_string(),
            "@utleiebygg.no".to_string(),
        ];
        assert!(avsender_tillatt("Post@Grossisten.no", &liste));
        assert!(avsender_tillatt("faktura@utleiebygg.no", &liste));
        assert!(
            !avsender_tillatt("noen@ukjent.no", &liste),
            "ukjent avsender slipper ikke inn"
        );
        assert!(
            !avsender_tillatt("post@grossisten.no.svindel.no", &liste),
            "domenet må matche helt, ikke bare begynne likt"
        );
        assert!(
            !avsender_tillatt("post@grossisten.no", &[]),
            "tom liste slipper ingen"
        );
    }
}
