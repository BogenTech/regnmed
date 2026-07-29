//! Fødselsnummer and D-nummer (docs/aksjonaer.md, #43): the check digits
//! and the birth date carried inside the number.
//!
//! The aksjonærregisteroppgave identifies personal shareholders by
//! fødselsnummer, whereas **the aksjeeierbok under aksjeloven §4-5 is to
//! contain only the birth date**. That is not a detail: one is a filing
//! to Skatteetaten, the other a register anyone has a right to inspect.
//! So the derivation fødselsnummer → birth date lives here, letting the
//! aksjeeierbok show exactly what the law asks for and not one digit more.
//!
//! The check digits are MOD11 in two rounds (the same family as orgnr and
//! KID). We validate them because a number with a wrong check digit is a
//! typo we can catch before it becomes a filing.
//!
//! **Note what this is NOT:** a valid check digit proves the number is
//! well-formed, not that the person exists. Looking a person up in
//! Folkeregisteret is a separate service with its own legal basis, and is
//! not done here.

use chrono::NaiveDate;

const K1: [u32; 9] = [3, 7, 6, 1, 8, 9, 4, 5, 2];
const K2: [u32; 10] = [5, 4, 3, 2, 7, 6, 5, 4, 3, 2];

fn digits(nummer: &str) -> Option<Vec<u32>> {
    if nummer.len() != 11 || !nummer.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(nummer.chars().map(|c| c.to_digit(10).unwrap()).collect())
}

/// Both control digits check out, and the number carries a real date.
///
/// Accepts D-nummer (day + 40) — a foreign shareholder registered in
/// Norway has one, and the oppgave uses the same field for it.
pub fn is_valid(nummer: &str) -> bool {
    let Some(d) = digits(nummer) else {
        return false;
    };
    let check = |weights: &[u32], n: usize| -> Option<u32> {
        let sum: u32 = d[..n].iter().zip(weights).map(|(x, w)| x * w).sum();
        match sum % 11 {
            0 => Some(0),
            1 => None, // ingen gyldig kontrollsiffer finnes
            rest => Some(11 - rest),
        }
    };
    if check(&K1, 9) != Some(d[9]) || check(&K2, 10) != Some(d[10]) {
        return false;
    }
    fodselsdato(nummer).is_some()
}

/// The birth date encoded in the number — what aksjeeierboken shows.
///
/// Three offsets the raw digits don't tell you, all of them real
/// numbers in circulation:
/// - **D-nummer** adds 40 to the day (01 → 41), for people without a
///   permanent Norwegian personnummer. A foreign shareholder has one.
/// - **H-nummer** adds 40 to the month — a help number issued by the
///   health service when identity is unconfirmed.
/// - **Syntetisk nummer** adds 80 to the month. This is Skatteetatens
///   own convention for test persons (Tenor), and their published
///   RF-1086 example uses one. We must read them: submissions to the
///   test environment are *required* to use synthetic data, so a parser
///   that rejected them could never be tested against the real API.
///
/// The **century** comes from the individnummer (digits 7-9) read
/// together with the two-digit year, per Skatteetatens rules. A number
/// falling outside every range is not a birth number — 750-899 with
/// year 54-99 is unallocated, and says so by returning None.
pub fn fodselsdato(nummer: &str) -> Option<NaiveDate> {
    let d = digits(nummer)?;
    let num = |slice: &[u32]| slice.iter().fold(0u32, |acc, x| acc * 10 + x);
    let mut dag = num(&d[0..2]);
    let mut maned = num(&d[2..4]);
    let ar2 = num(&d[4..6]);
    let individ = num(&d[6..9]);

    // D-nummer: 40 has been added to the day.
    if dag > 40 {
        dag -= 40;
    }
    // Synthetic (80) before H-nummer (40) — the order is unambiguous
    // because a month is never above 12 to begin with.
    if maned > 80 {
        maned -= 80;
    } else if maned > 40 {
        maned -= 40;
    }

    let arhundre = match (individ, ar2) {
        (0..=499, _) => 1900,
        (500..=749, 54..=99) => 1800,
        (500..=999, 0..=39) => 2000,
        (900..=999, 40..=99) => 1900,
        _ => return None,
    };
    NaiveDate::from_ymd_opt((arhundre + ar2) as i32, maned, dag)
}

/// True for a D-nummer rather than an ordinary fødselsnummer.
pub fn er_dnummer(nummer: &str) -> bool {
    digits(nummer).is_some_and(|d| d[0] * 10 + d[1] > 40)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dato(y: i32, m: u32, d: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(y, m, d)
    }

    /// Synthetic test numbers from Skatteetaten's Tenor test data set.
    /// The first appears in the agency's own RF-1086 example file. They
    /// are constructed for the purpose and are not real people — 80 has
    /// been added to the month.
    const TENOR: [&str; 3] = ["26829398612", "08888797336", "25927898821"];

    #[test]
    fn check_digits_hold_for_skatteetatens_own_test_numbers() {
        for n in TENOR {
            assert!(is_valid(n), "{n}");
        }
    }

    #[test]
    fn typos_are_rejected() {
        // Ett siffer endret bakerst bryter kontrollrunden.
        assert!(!is_valid("26829398613"));
        assert!(!is_valid("2682939861"));
        assert!(!is_valid("2682939861a"));
        assert!(!is_valid(""));
    }

    #[test]
    fn a_synthetic_month_reads_as_the_real_month() {
        // 26.82.93 is 26 February 1993 with +80 on the month.
        assert_eq!(fodselsdato("26829398612"), dato(1993, 2, 26));
        assert_eq!(fodselsdato("08888797336"), dato(1987, 8, 8));
        assert_eq!(fodselsdato("25927898821"), dato(1978, 12, 25));
        assert!(!er_dnummer("26829398612"));
    }

    #[test]
    fn dnummer_subtracts_40_from_the_day() {
        assert!(er_dnummer("41019010110"));
        assert_eq!(fodselsdato("41019010110"), dato(1990, 1, 1));
        assert!(is_valid("41019010110"));
    }

    #[test]
    fn hnummer_subtracts_40_from_the_month() {
        assert_eq!(fodselsdato("01419010029"), dato(1990, 1, 1));
    }

    /// The century sits in the individnummer, not in the year — this is
    /// the rule that keeps a shareholder born in 1905 and one born in
    /// 2005 from being confused for each other.
    #[test]
    fn the_century_comes_from_the_individnummer() {
        // 500-749 with year 54-99 → the 1800s.
        assert_eq!(fodselsdato("01016050012"), dato(1860, 1, 1));
        // 500-999 with year 00-39 → the 2000s.
        assert_eq!(fodselsdato("01010550048"), dato(2005, 1, 1));
        // 900-999 with year 40-99 → the 1900s.
        assert_eq!(fodselsdato("01016090073"), dato(1960, 1, 1));
    }

    #[test]
    fn an_individnummer_out_of_range_yields_no_date() {
        // 750-899 with year 54-99 is assigned to no century at all.
        // The check digits are fine — it is the date rule that says no.
        assert_eq!(fodselsdato("01016075015"), None);
        assert!(!is_valid("01016075015"));
    }

    #[test]
    fn an_impossible_date_is_rejected_even_with_valid_check_digits() {
        // There is no 31 February.
        assert_eq!(fodselsdato("31029010059"), None);
        assert!(!is_valid("31029010059"));
    }
}
