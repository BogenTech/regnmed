//! Norwegian organisasjonsnummer validation (MOD11 with weights
//! 3,2,7,6,5,4,3,2 over the first eight digits). Every orgnr regnmed
//! accepts — companies, firms, parties — should pass this before any
//! registry lookup.

pub fn is_valid(orgnr: &str) -> bool {
    if orgnr.len() != 9 || !orgnr.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let digits: Vec<u32> = orgnr.chars().map(|c| c.to_digit(10).unwrap()).collect();
    const WEIGHTS: [u32; 8] = [3, 2, 7, 6, 5, 4, 3, 2];
    let sum: u32 = digits[..8].iter().zip(WEIGHTS).map(|(d, w)| d * w).sum();
    let check = match sum % 11 {
        0 => 0,
        1 => return false, // no valid check digit exists
        rest => 11 - rest,
    };
    digits[8] == check
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_real_and_rejects_wrong() {
        assert!(is_valid("923609016"), "Equinor ASA");
        assert!(is_valid("974760673"), "Brønnøysundregistrene");
        assert!(!is_valid("923609017"));
        assert!(!is_valid("12345678"));
        assert!(!is_valid("12345678a"));
    }
}
