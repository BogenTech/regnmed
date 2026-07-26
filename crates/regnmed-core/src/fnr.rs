//! Fødselsnummer og D-nummer (docs/aksjonaer.md, #43): kontrollsifrene
//! og fødselsdatoen som ligger inne i nummeret.
//!
//! Aksjonærregisteroppgaven identifiserer personlige aksjonærer med
//! fødselsnummer, mens **aksjeeierboken etter aksjeloven §4-5 bare skal
//! inneholde fødselsdato**. Det er ikke en detalj: det ene er en
//! innrapportering til Skatteetaten, det andre er et register enhver
//! har innsynsrett i. Derfor bor utledningen fødselsnummer →
//! fødselsdato her, slik at aksjeeierboken kan vise akkurat det loven
//! ber om og ikke ett siffer mer.
//!
//! Kontrollsifrene er MOD11 med to runder (samme familie som orgnr og
//! KID). Vi validerer dem fordi et nummer med feil kontrollsiffer er en
//! tastefeil vi kan fange før den blir en innrapportering.
//!
//! **Merk hva dette IKKE er:** et gyldig kontrollsiffer beviser at
//! nummeret er velformet, ikke at personen finnes. Oppslag mot
//! Folkeregisteret er en egen tjeneste med egne hjemler, og gjøres ikke
//! her.

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

    // D-nummer: dagen er lagt 40 til.
    if dag > 40 {
        dag -= 40;
    }
    // Syntetisk (80) før H-nummer (40) — rekkefølgen er entydig fordi
    // en måned aldri er over 12 i utgangspunktet.
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

    /// Syntetiske testnumre fra Skatteetatens Tenor-testdatasett. Det
    /// første står i etatens egen RF-1086-eksempelfil. De er konstruert
    /// for formålet og er ikke ekte personer — måneden er lagt 80 til.
    const TENOR: [&str; 3] = ["26829398612", "08888797336", "25927898821"];

    #[test]
    fn kontrollsifrene_stemmer_for_skatteetatens_egne_testnumre() {
        for n in TENOR {
            assert!(is_valid(n), "{n}");
        }
    }

    #[test]
    fn tastefeil_avvises() {
        // Ett siffer endret bakerst bryter kontrollrunden.
        assert!(!is_valid("26829398613"));
        assert!(!is_valid("2682939861"));
        assert!(!is_valid("2682939861a"));
        assert!(!is_valid(""));
    }

    #[test]
    fn syntetisk_maned_leses_som_ekte_maned() {
        // 26.82.93 er den 26. februar 1993 med +80 på måneden.
        assert_eq!(fodselsdato("26829398612"), dato(1993, 2, 26));
        assert_eq!(fodselsdato("08888797336"), dato(1987, 8, 8));
        assert_eq!(fodselsdato("25927898821"), dato(1978, 12, 25));
        assert!(!er_dnummer("26829398612"));
    }

    #[test]
    fn dnummer_trekker_fra_40_paa_dagen() {
        assert!(er_dnummer("41019010110"));
        assert_eq!(fodselsdato("41019010110"), dato(1990, 1, 1));
        assert!(is_valid("41019010110"));
    }

    #[test]
    fn hnummer_trekker_fra_40_paa_maneden() {
        assert_eq!(fodselsdato("01419010029"), dato(1990, 1, 1));
    }

    /// Århundret ligger i individnummeret, ikke i årstallet — dette er
    /// regelen som gjør at en aksjonær født i 1905 og en født i 2005
    /// ikke blandes sammen.
    #[test]
    fn arhundret_kommer_fra_individnummeret() {
        // 500-749 med årstall 54-99 → 1800-tallet.
        assert_eq!(fodselsdato("01016050012"), dato(1860, 1, 1));
        // 500-999 med årstall 00-39 → 2000-tallet.
        assert_eq!(fodselsdato("01010550048"), dato(2005, 1, 1));
        // 900-999 med årstall 40-99 → 1900-tallet.
        assert_eq!(fodselsdato("01016090073"), dato(1960, 1, 1));
    }

    #[test]
    fn individnummer_utenfor_rekkevidde_er_ingen_dato() {
        // 750-899 med årstall 54-99 er ikke tildelt noe århundre.
        // Kontrollsifrene stemmer — det er datoregelen som sier nei.
        assert_eq!(fodselsdato("01016075015"), None);
        assert!(!is_valid("01016075015"));
    }

    #[test]
    fn umulig_dato_avvises_selv_med_riktige_kontrollsiffer() {
        // 31. februar finnes ikke.
        assert_eq!(fodselsdato("31029010059"), None);
        assert!(!is_valid("31029010059"));
    }
}
