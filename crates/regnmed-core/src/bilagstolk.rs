//! Bilagstolkning: forslag fra dokumentets egen tekst
//! (docs/bilagstolkning.md, #34).
//!
//! Målet er å spare tastetrykk, ikke å bokføre. Alt her er **forslag**,
//! og hvert forslag bærer sin egen begrunnelse («fant etter
//! «Å betale»») — regnskapsføreren skal kunne se hvorfor maskinen tror
//! noe, og overprøve det uten å lete.
//!
//! Trikset som gjør heuristikken tålelig presis uten modeller:
//! **kontrollsifrene vi allerede stoler på**. Et ni-sifret tall som
//! passerer orgnr-MOD11 er nesten sikkert et organisasjonsnummer; et
//! tall som passerer KID-MOD10/MOD11 ved siden av ordet «KID» er en
//! KID; elleve siffer som passerer kontonummer-MOD11 er et
//! kontonummer. Tilfeldige tall gjør ikke det.
//!
//! Ingen skytjeneste, ingen modell i kjernen — OCR for skannede bilder
//! hører til en valgfri sidecar (docs/frugality.md), og API-et
//! oppfører seg likt uten den: da mangler forslagene, ferdig.

use chrono::NaiveDate;

/// One suggested value with the reason we believe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Funn<T> {
    pub verdi: T,
    /// Human-readable provenance, shown in the UI next to the value.
    pub begrunnelse: String,
}

impl<T> Funn<T> {
    fn new(verdi: T, begrunnelse: impl Into<String>) -> Self {
        Self {
            verdi,
            begrunnelse: begrunnelse.into(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Forslag {
    pub orgnr: Option<Funn<String>>,
    pub fakturanr: Option<Funn<String>>,
    pub kid: Option<Funn<String>>,
    pub kontonummer: Option<Funn<String>>,
    pub dato: Option<Funn<NaiveDate>>,
    pub forfall: Option<Funn<NaiveDate>>,
    /// Brutto å betale.
    pub belop_ore: Option<Funn<i64>>,
    pub mva_ore: Option<Funn<i64>>,
}

fn digits_only(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Every run of digits, spaces and dots — the shapes Norwegian numbers
/// are written in ("915 933 149", "8601.11.17947", "1 234,50").
fn number_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in line.chars() {
        if c.is_ascii_digit() || (!current.is_empty() && (c == ' ' || c == '.' || c == ',')) {
            current.push(c);
        } else {
            if current.chars().any(|c| c.is_ascii_digit()) {
                tokens.push(current.trim_end_matches([' ', '.', ',']).to_string());
            }
            current = String::new();
        }
    }
    if current.chars().any(|c| c.is_ascii_digit()) {
        tokens.push(current.trim_end_matches([' ', '.', ',']).to_string());
    }
    tokens
}

/// "1 234,50" / "1.234,50" / "1234.50" / "1234" → øre. Refuses anything
/// with more than two decimals — an invoice line is not a rate.
fn parse_belop(token: &str) -> Option<i64> {
    let cleaned: String = token.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return None;
    }
    // The last separator decides; thousands separators come in pairs of
    // three digits behind them.
    let (whole, frac) = match cleaned.rfind([',', '.']) {
        Some(pos) => {
            let (w, f) = cleaned.split_at(pos);
            let f = &f[1..];
            if f.len() == 3 && cleaned.matches([',', '.']).count() >= 1 && !cleaned.contains(',') {
                // "1.234" — a thousands separator, not decimals.
                (cleaned.replace('.', ""), String::new())
            } else if f.len() > 2 {
                return None;
            } else {
                (w.replace(['.', ' '], "").replace(',', ""), f.to_string())
            }
        }
        None => (cleaned.clone(), String::new()),
    };
    let whole: i64 = whole.parse().ok()?;
    let frac: i64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<2}").parse().ok()?
    };
    Some(whole * 100 + frac)
}

fn parse_dato(token: &str) -> Option<NaiveDate> {
    let t = token.trim();
    for format in ["%d.%m.%Y", "%Y-%m-%d", "%d.%m.%y", "%d/%m/%Y", "%d.%m.%Y."] {
        if let Ok(date) = NaiveDate::parse_from_str(t, format) {
            return Some(date);
        }
    }
    None
}

/// Dates written with dots are cut out of the surrounding text.
fn date_candidates(line: &str) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || "./-".contains(chars[i])) {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            if let Some(date) = parse_dato(token.trim_end_matches(['.', '/', '-'])) {
                out.push(date);
            }
        } else {
            i += 1;
        }
    }
    out
}

fn normalized(line: &str) -> String {
    line.to_lowercase()
        .replace('ø', "o")
        .replace('æ', "ae")
        .replace('å', "a")
}

/// Reads a document's text and proposes what it can defend.
pub fn tolk(text: &str) -> Forslag {
    let mut forslag = Forslag::default();
    let lines: Vec<&str> = text.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let lower = normalized(line);
        // Labels sometimes sit above their value instead of beside it,
        // so an empty line falls through to the next one — but a line
        // that HAS numbers keeps to its own: reading across would let
        // the total below «mva» masquerade as the tax.
        let neste = lines.get(i + 1).copied().unwrap_or("");
        let egne = number_tokens(line);
        let tokens: Vec<String> = if egne.is_empty() {
            number_tokens(neste)
        } else {
            egne
        };

        // --- Identifikatorer, avgjort av kontrollsiffer ---
        for token in &tokens {
            let digits = digits_only(token);
            if forslag.orgnr.is_none()
                && digits.len() == 9
                && crate::orgnr::is_valid(&digits)
                && (lower.contains("org")
                    || lower.contains("foretaksregister")
                    || lower.contains("mva"))
            {
                forslag.orgnr = Some(Funn::new(
                    digits.clone(),
                    format!(
                        "ni siffer med gyldig kontrollsiffer nær «{}»",
                        stikkord(&lower)
                    ),
                ));
            }
            if forslag.kid.is_none() && lower.contains("kid") && crate::kid::is_valid(&digits) {
                forslag.kid = Some(Funn::new(digits.clone(), "gyldig KID nær «KID»"));
            }
            if forslag.kontonummer.is_none()
                && digits.len() == 11
                && crate::pain001::gyldig_kontonummer(&digits)
                && (lower.contains("konto") || lower.contains("bank"))
            {
                forslag.kontonummer = Some(Funn::new(
                    digits.clone(),
                    "elleve siffer med gyldig kontrollsiffer nær «konto»",
                ));
            }
        }

        // --- Fakturanummer ---
        if forslag.fakturanr.is_none()
            && (lower.contains("fakturanr")
                || lower.contains("faktura nr")
                || lower.contains("fakturanummer"))
            && let Some(token) = tokens.iter().find(|t| {
                let d = digits_only(t);
                (1..=20).contains(&d.len()) && !d.is_empty()
            })
        {
            forslag.fakturanr = Some(Funn::new(digits_only(token), "etter «fakturanr»"));
        }

        // --- Datoer ---
        let datoer = {
            let d = date_candidates(line);
            if d.is_empty() {
                date_candidates(neste)
            } else {
                d
            }
        };
        if forslag.forfall.is_none()
            && (lower.contains("forfall") || lower.contains("betalingsfrist"))
            && let Some(dato) = datoer.first()
        {
            forslag.forfall = Some(Funn::new(*dato, "etter «forfall»"));
        }
        if forslag.dato.is_none()
            && (lower.contains("fakturadato") || lower.contains("bilagsdato"))
            && let Some(dato) = datoer.first()
        {
            forslag.dato = Some(Funn::new(*dato, "etter «fakturadato»"));
        }

        // --- Beløp ---
        // «Å betale» er det eneste tallet på en faktura som betyr
        // nøyaktig én ting, så det vinner over «sum» og «total».
        let belop_stikkord: &[(&str, &str)] = &[
            ("a betale", "«å betale»"),
            ("belop a betale", "«beløp å betale»"),
            ("total inkl", "«total inkl. mva»"),
            ("totalt inkl", "«totalt inkl. mva»"),
            ("sum inkl", "«sum inkl. mva»"),
            ("totalsum", "«totalsum»"),
        ];
        for (nokkel, vist) in belop_stikkord {
            if lower.contains(nokkel)
                && let Some(belop) = tokens.iter().filter_map(|t| parse_belop(t)).next_back()
                && belop > 0
            {
                let sterkere = forslag
                    .belop_ore
                    .as_ref()
                    .is_none_or(|f| !f.begrunnelse.contains("å betale"));
                if sterkere {
                    forslag.belop_ore = Some(Funn::new(belop, format!("etter {vist}")));
                }
                break;
            }
        }
        if forslag.mva_ore.is_none()
            && (lower.contains("mva") || lower.contains("merverdiavgift"))
            && !lower.contains("orgnr")
            && !lower.contains("inkl")
            && let Some(belop) = tokens.iter().filter_map(|t| parse_belop(t)).next_back()
            && belop > 0
        {
            forslag.mva_ore = Some(Funn::new(belop, "etter «mva»"));
        }
    }
    forslag
}

/// The keyword we matched, for the provenance string.
fn stikkord(lower: &str) -> &'static str {
    if lower.contains("orgnr") || lower.contains("organisasjonsnummer") {
        "orgnr"
    } else if lower.contains("foretaksregister") {
        "Foretaksregisteret"
    } else {
        "mva"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKTURA: &str = "\
Handelshuset AS
Orgnr 974760673 MVA
Storgata 1, 0155 Oslo

FAKTURA
Fakturanr: 90210
Fakturadato: 30.06.2026
Forfallsdato: 30.07.2026

Kontorstoler 10 stk            2 000,00
Frakt                            250,00
Nettosum                       2 250,00
MVA 25 %                         562,50
Å betale                       2 812,50

Kontonummer: 8601.11.17947
KID: 1234567897
";

    #[test]
    fn leser_en_vanlig_norsk_faktura() {
        let f = tolk(FAKTURA);
        assert_eq!(f.orgnr.as_ref().unwrap().verdi, "974760673");
        assert_eq!(f.fakturanr.as_ref().unwrap().verdi, "90210");
        assert_eq!(
            f.dato.as_ref().unwrap().verdi,
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap()
        );
        assert_eq!(
            f.forfall.as_ref().unwrap().verdi,
            NaiveDate::from_ymd_opt(2026, 7, 30).unwrap()
        );
        assert_eq!(f.belop_ore.as_ref().unwrap().verdi, 2_812_50);
        assert_eq!(f.mva_ore.as_ref().unwrap().verdi, 562_50);
        assert_eq!(f.kontonummer.as_ref().unwrap().verdi, "86011117947");
        assert_eq!(f.kid.as_ref().unwrap().verdi, "1234567897");
    }

    #[test]
    fn hvert_funn_forteller_hvorfor() {
        let f = tolk(FAKTURA);
        assert!(
            f.belop_ore
                .as_ref()
                .unwrap()
                .begrunnelse
                .contains("å betale"),
            "{:?}",
            f.belop_ore
        );
        assert!(f.kid.as_ref().unwrap().begrunnelse.contains("KID"));
        assert!(
            f.orgnr
                .as_ref()
                .unwrap()
                .begrunnelse
                .contains("kontrollsiffer")
        );
    }

    #[test]
    fn tall_uten_gyldig_kontrollsiffer_foreslas_ikke() {
        let tekst = "Orgnr 123456789\nKID: 1111111111\nKontonummer: 12345678901\nBeskrivelse";
        let f = tolk(tekst);
        assert!(f.orgnr.is_none(), "ugyldig orgnr skal ikke foreslås");
        assert!(f.kid.is_none());
        assert!(f.kontonummer.is_none());
    }

    #[test]
    fn verdi_paa_linjen_under_etiketten_finnes_ogsaa() {
        let tekst = "Fakturanr\n55512\nForfallsdato\n01.09.2026\nÅ betale\n1 000,00";
        let f = tolk(tekst);
        assert_eq!(f.fakturanr.unwrap().verdi, "55512");
        assert_eq!(
            f.forfall.unwrap().verdi,
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()
        );
        assert_eq!(f.belop_ore.unwrap().verdi, 1_000_00);
    }

    #[test]
    fn a_betale_vinner_over_andre_summer() {
        let tekst = "Sum inkl mva 9 999,00\nÅ betale 1 234,50\nTotalsum 8 888,00";
        let f = tolk(tekst).belop_ore.unwrap();
        assert_eq!(f.verdi, 1_234_50);
        assert!(f.begrunnelse.contains("å betale"));
    }

    #[test]
    fn belopsformater() {
        assert_eq!(parse_belop("1 234,50"), Some(123_450));
        assert_eq!(parse_belop("1.234,50"), Some(123_450));
        assert_eq!(parse_belop("1234.50"), Some(123_450));
        assert_eq!(parse_belop("1234"), Some(123_400));
        assert_eq!(parse_belop("1.234"), Some(123_400), "tusenskille, ikke øre");
        assert_eq!(
            parse_belop("12,345"),
            None,
            "tre desimaler er ikke et beløp"
        );
    }

    #[test]
    fn tom_tekst_gir_tomt_forslag() {
        let f = tolk("");
        assert!(f.orgnr.is_none() && f.belop_ore.is_none() && f.dato.is_none());
    }
}
