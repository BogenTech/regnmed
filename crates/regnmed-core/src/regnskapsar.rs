//! Regnskapsåret — antakelsen, på ett sted (docs/regelverk.md, #52).
//!
//! **regnmed antar at regnskapsåret er kalenderåret.** Det er
//! hovedregelen i regnskapsloven §1-7 og situasjonen til så godt som
//! hele målgruppen. Avvikende regnskapsår er tillatt i definerte
//! tilfeller (sesongvirksomhet, konsern med utenlandsk morselskap), og
//! da stemmer ikke antakelsen.
//!
//! Antakelsen er ikke fjernet, men den er **samlet og navngitt**: den
//! dagen en kunde med avvikende regnskapsår dukker opp, er dette
//! stedet definisjonen endres, og docs/regelverk.md lister nøyaktig
//! hvilke andre steder som da må følge etter. Et spredt `.year()` er
//! en antakelse ingen finner igjen; en funksjon med navn er en
//! beslutning noen kan ta om igjen.
//!
//! Merk hva som IKKE hører hjemme her: **mva-terminer er
//! kalenderforankret uansett** (mval. §15-1 jf. sktfvf. §8-3). De
//! følger ikke regnskapsåret, og skal aldri hente perioden sin herfra.

use chrono::{Datelike, NaiveDate};

/// The fiscal year a date belongs to.
///
/// Today: the calendar year. This is what gap-free voucher numbering is
/// keyed by (`voucher_counter(journal_id, fiscal_year)`), so changing
/// it changes which counter a voucher draws its number from — never do
/// it for a company that already has vouchers in the affected period.
pub fn regnskapsar(dato: NaiveDate) -> i32 {
    dato.year()
}

/// First and last day of a fiscal year, inclusive — the period a
/// year-based report covers.
pub fn regnskapsar_periode(ar: i32) -> Option<(NaiveDate, NaiveDate)> {
    Some((
        NaiveDate::from_ymd_opt(ar, 1, 1)?,
        NaiveDate::from_ymd_opt(ar, 12, 31)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the assumption. If this test is ever changed, it must be
    /// because someone decided to support avvikende regnskapsår — not
    /// because a date happened to fall on the wrong side of something.
    #[test]
    fn regnskapsaret_er_kalenderaret() {
        let dato = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(regnskapsar(dato(2026, 1, 1)), 2026);
        assert_eq!(regnskapsar(dato(2026, 12, 31)), 2026);
        // Nyttårsaften og nyttårsdag hører til hvert sitt år, og hvert
        // sitt bilagsnummer-løp.
        assert_eq!(regnskapsar(dato(2027, 1, 1)), 2027);
    }

    #[test]
    fn perioden_dekker_hele_aret() {
        let (start, slutt) = regnskapsar_periode(2026).unwrap();
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        assert_eq!(slutt, NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
        assert_eq!(regnskapsar(start), 2026);
        assert_eq!(regnskapsar(slutt), 2026);
        // Skuddår har sin ekstra dag inne i perioden.
        let (_, slutt) = regnskapsar_periode(2028).unwrap();
        assert_eq!(slutt.ordinal(), 366);
    }
}
