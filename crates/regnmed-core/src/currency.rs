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
pub struct Currency {
    /// ISO 4217 code, always upper case, never "NOK".
    pub code: String,
    /// Amount in the currency's smallest unit; same sign as the NOK amount.
    pub amount_cent: i64,
    /// NOK per currency unit, in micro-NOK.
    pub rate_micro: i64,
}

/// cent × rate → øre, half away from zero.
pub fn nok_ore(amount_cent: i64, rate_micro: i64) -> i64 {
    let product = i128::from(amount_cent) * i128::from(rate_micro);
    let rounded = (product.abs() + 500_000) / 1_000_000;
    i64::try_from(rounded).expect("NOK amount fits in i64") * product.signum() as i64
}

/// Parses a decimal rate ("11.6543" or "11,6543") into micro-NOK.
/// At most six decimals; more precision than Norges Bank publishes is
/// a unit mistake, not a rate.
pub fn parse_rate(s: &str) -> Option<i64> {
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

/// Formats micro-NOK back to a decimal string ("11.654300").
pub fn rate_str(rate_micro: i64) -> String {
    format!("{}.{:06}", rate_micro / 1_000_000, rate_micro % 1_000_000)
}

/// A well-formed currency code: three ASCII uppercase letters, not NOK
/// (NOK amounts are just amounts).
pub fn is_valid_currency_code(code: &str) -> bool {
    code.len() == 3 && code.chars().all(|c| c.is_ascii_uppercase()) && code != "NOK"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_rounds_half_away_from_zero() {
        // 100,00 EUR à 11,6543 = 1 165,43 kr.
        assert_eq!(nok_ore(10_000, 11_654_300), 116_543);
        // 1 cent at 11,6543 = 0,116543 kr → 12 øre (half up).
        assert_eq!(nok_ore(1, 11_654_300), 12);
        assert_eq!(nok_ore(-1, 11_654_300), -12, "symmetric around zero");
        // 5 cent at 0,10 kr = 0,5 øre → 1 øre.
        assert_eq!(nok_ore(5, 100_000), 1);
    }

    #[test]
    fn rate_parsing_and_formatting() {
        assert_eq!(parse_rate("11.6543"), Some(11_654_300));
        assert_eq!(parse_rate("11,6543"), Some(11_654_300));
        assert_eq!(parse_rate("10"), Some(10_000_000));
        assert_eq!(parse_rate("0.093"), Some(93_000), "SEK scale");
        assert_eq!(parse_rate("0"), None, "zero is not a rate");
        assert_eq!(parse_rate("1.1234567"), None, "too many decimals");
        assert_eq!(parse_rate("abc"), None);
        assert_eq!(rate_str(11_654_300), "11.654300");
        assert_eq!(parse_rate(&rate_str(93_000)), Some(93_000), "round trip");
    }

    #[test]
    fn currency_codes() {
        assert!(is_valid_currency_code("EUR") && is_valid_currency_code("SEK"));
        assert!(
            !is_valid_currency_code("NOK"),
            "NOK is the bookkeeping currency"
        );
        assert!(
            !is_valid_currency_code("eur")
                && !is_valid_currency_code("EURO")
                && !is_valid_currency_code("")
        );
    }
}
