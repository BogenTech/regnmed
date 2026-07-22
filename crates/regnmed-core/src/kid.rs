//! KID (kundeidentifikasjon) check digits: MOD10 (Luhn) and MOD11.
//!
//! A KID's last digit is a check digit over the preceding digits. The
//! issuer picks the scheme; a receiver without that knowledge accepts a
//! KID when either scheme validates ([`is_valid`]). Generation
//! ([`check_digit_mod10`]/[`check_digit_mod11`]) serves faktura: MOD11
//! cannot always produce a digit (remainder 1 has none) — issuers skip
//! such numbers or use MOD10.

/// MOD10/Luhn: weights 2,1,2,1,… from the right over the base digits.
pub fn check_digit_mod10(base: &str) -> Option<u8> {
    let mut sum = 0u32;
    for (index, ch) in base.chars().rev().enumerate() {
        let digit = ch.to_digit(10)?;
        let weighted = if index % 2 == 0 { digit * 2 } else { digit };
        sum += weighted / 10 + weighted % 10;
    }
    Some(((10 - sum % 10) % 10) as u8)
}

/// MOD11: weights 2,3,4,5,6,7 cyclically from the right. Remainder 1 has
/// no valid check digit (banks reject such base numbers); remainder 0
/// gives 0.
pub fn check_digit_mod11(base: &str) -> Option<u8> {
    let mut sum = 0u32;
    for (index, ch) in base.chars().rev().enumerate() {
        let digit = ch.to_digit(10)?;
        sum += digit * (2 + (index as u32 % 6));
    }
    match sum % 11 {
        0 => Some(0),
        1 => None,
        rest => Some((11 - rest) as u8),
    }
}

fn valid_with(kid: &str, scheme: fn(&str) -> Option<u8>) -> bool {
    if kid.len() < 2 || !kid.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let (base, check) = kid.split_at(kid.len() - 1);
    scheme(base) == check.parse().ok()
}

pub fn is_valid_mod10(kid: &str) -> bool {
    valid_with(kid, check_digit_mod10)
}

pub fn is_valid_mod11(kid: &str) -> bool {
    valid_with(kid, check_digit_mod11)
}

/// Valid under either scheme — the receiver-side check. KIDs are 2–25
/// digits.
pub fn is_valid(kid: &str) -> bool {
    kid.len() <= 25 && (is_valid_mod10(kid) || is_valid_mod11(kid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod10_check_digit_and_validation() {
        // 123456 → weighted sum 24 → check digit 6.
        assert_eq!(check_digit_mod10("123456"), Some(6));
        assert!(is_valid_mod10("1234566"));
        assert!(!is_valid_mod10("1234567"));
    }

    #[test]
    fn mod11_check_digit_and_validation() {
        // 123456 → weighted sum 77 → remainder 0 → check digit 0.
        assert_eq!(check_digit_mod11("123456"), Some(0));
        assert!(is_valid_mod11("1234560"));
        assert!(!is_valid_mod11("1234561"));
        // 6 → weighted sum 12 → remainder 1 → no valid check digit.
        assert_eq!(check_digit_mod11("6"), None);
    }

    #[test]
    fn receiver_side_accepts_either_scheme() {
        assert!(is_valid("1234566"), "mod10");
        assert!(is_valid("1234560"), "mod11");
        assert!(!is_valid("1234561"));
        assert!(!is_valid("12A45"));
        assert!(!is_valid("7"));
    }
}
