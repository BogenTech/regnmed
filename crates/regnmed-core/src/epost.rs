//! E-post-inn: adressen og avsenderregelen (docs/epost-inn.md, #35).
//!
//! To rene avgjørelser som må være like hver gang, og som derfor bor
//! her i stedet for i en spørring: hvordan en mottaksadresse dannes, og
//! om en avsender står på selskapets liste.

/// Local-part of a company's inbound address: `bilag-<navn>-<tilfeldig>`.
///
/// Navnedelen er der for menneskene (adressen skal kunne leses opp over
/// telefon), den tilfeldige halen er der fordi adressen er en
/// **kapabilitet**: den som kjenner den kan levere noe i innboksen, så
/// den må ikke kunne gjettes ut fra firmanavnet.
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
    fn adressen_er_lesbar_men_ikke_gjettbar() {
        let a = local_part("Purredemo AS", "a1b2c3d4e5");
        assert_eq!(a, "bilag-purredemo-as-a1b2c3d4e5");
        // Norske tegn og skilletegn overlever som noe en mailserver
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
    fn avsender_leses_ut_av_visningsnavn() {
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
    fn listen_godtar_adresse_og_domene() {
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
