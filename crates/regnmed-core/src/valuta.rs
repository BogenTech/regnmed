//! Flervaluta, pure side (docs/valuta.md, #44).
//!
//! The bookkeeping currency is NOK; documents carry the transaction
//! currency. Everything is integers: currency amounts in the currency's
//! smallest unit (cent), rates in micro-NOK per currency unit
//! (11,6543 kr/EUR → 11_654_300). Conversion rounds half away from zero
//! per amount — sums are sums of rounded parts, never the other way round
//! (documented in docs/valuta.md).
//!
//! The currency information on a posting line is evidence and is covered
//! by hash format v4 ([`crate::hash`]).

/// Currency information on a posting line (hash format v4): what the
/// transaction was denominated in, and the rate it was booked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Valuta {
    /// ISO 4217 code, always upper case, never "NOK".
    pub valuta: String,
    /// Amount in the currency's smallest unit; same sign as the NOK amount.
    pub belop_cent: i64,
    /// NOK per valutaenhet i mikro-NOK.
    pub kurs_micro: i64,
}

/// cent × rate → øre, half away from zero.
pub fn nok_ore(belop_cent: i64, kurs_micro: i64) -> i64 {
    let product = i128::from(belop_cent) * i128::from(kurs_micro);
    let rounded = (product.abs() + 500_000) / 1_000_000;
    i64::try_from(rounded).expect("NOK amount fits in i64") * product.signum() as i64
}

/// Parses a decimal kurs ("11.6543" or "11,6543") into mikro-NOK.
/// At most six decimals; more precision than Norges Bank publishes is
/// a unit mistake, not a rate.
pub fn parse_kurs(s: &str) -> Option<i64> {
    let s = s.trim().replace(',', ".");
    let (whole, frac) = match s.split_once('.') {
        Some((w, f)) => (w, f),
        None => (s.as_str(), ""),
    };
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    if frac.len() > 6 || !whole.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let mut frac_val: i64 = if frac.is_empty() {
        0
    } else {
        frac.parse().ok()?
    };
    for _ in frac.len()..6 {
        frac_val *= 10;
    }
    let kurs = whole.checked_mul(1_000_000)?.checked_add(frac_val)?;
    (kurs > 0).then_some(kurs)
}

/// Formats mikro-NOK back to a decimal string ("11.654300").
pub fn kurs_str(kurs_micro: i64) -> String {
    format!("{}.{:06}", kurs_micro / 1_000_000, kurs_micro % 1_000_000)
}

/// A well-formed currency code: three ASCII uppercase letters, not NOK
/// (NOK amounts are just amounts).
pub fn gyldig_valutakode(code: &str) -> bool {
    code.len() == 3 && code.chars().all(|c| c.is_ascii_uppercase()) && code != "NOK"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omregning_avrunder_halvt_vekk() {
        // 100,00 EUR à 11,6543 = 1 165,43 kr.
        assert_eq!(nok_ore(10_000, 11_654_300), 116_543);
        // 1 cent at 11,6543 = 0,116543 kr → 12 øre (half up).
        assert_eq!(nok_ore(1, 11_654_300), 12);
        assert_eq!(nok_ore(-1, 11_654_300), -12, "symmetrisk rundt null");
        // 5 cent at 0,10 kr = 0,5 øre → 1 øre.
        assert_eq!(nok_ore(5, 100_000), 1);
    }

    #[test]
    fn rate_parsing_and_formatting() {
        assert_eq!(parse_kurs("11.6543"), Some(11_654_300));
        assert_eq!(parse_kurs("11,6543"), Some(11_654_300));
        assert_eq!(parse_kurs("10"), Some(10_000_000));
        assert_eq!(parse_kurs("0.093"), Some(93_000), "SEK-nivå");
        assert_eq!(parse_kurs("0"), None, "null er ingen kurs");
        assert_eq!(parse_kurs("1.1234567"), None, "for mange desimaler");
        assert_eq!(parse_kurs("abc"), None);
        assert_eq!(kurs_str(11_654_300), "11.654300");
        assert_eq!(parse_kurs(&kurs_str(93_000)), Some(93_000), "rundtur");
    }

    #[test]
    fn valutakoder() {
        assert!(gyldig_valutakode("EUR") && gyldig_valutakode("SEK"));
        assert!(!gyldig_valutakode("NOK"), "NOK er bokføringsvalutaen");
        assert!(!gyldig_valutakode("eur") && !gyldig_valutakode("EURO") && !gyldig_valutakode(""));
    }
}
