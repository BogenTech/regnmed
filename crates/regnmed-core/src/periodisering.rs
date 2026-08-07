//! Periodisering (#87): spreading a cost or an income over the months it
//! belongs to — rskl. §4-1 nr. 2 og 3, opptjenings- og
//! sammenstillingsprinsippet.
//!
//! Rent og deterministisk som resten av kassen. Denne modulen eier ÉN
//! ting: hvordan totalen deles på månedene. Bokføringen, tabellen og
//! kjøringen ligger i regnmed-db, etter mønsteret fra avskrivningene
//! (#40) og de repeterende fakturaene (#30).
//!
//! **Periodisering flytter kostnad og inntekt, ALDRI merverdiavgift.**
//! Tidfestingen av avgiften følger salgsdokumentet (mval. §15-9): en
//! husleie betalt for et helt år er fradragsberettiget i sin helhet i
//! den terminen fakturaen hører hjemme, uansett hvordan kostnaden
//! fordeles i resultatet. Fordeler man avgiften med, blir mva-meldingen
//! feil — og feilen er stille, fordi resultatet ser riktigere ut.
//! Derfor tar funksjonene her et NETTOBELØP, og kalleren har allerede
//! skilt avgiften ut.

use chrono::NaiveDate;

/// Antall måneder fra og med `fra` til og med `til`, begge angitt som
/// (år, måned). 0 eller mindre når intervallet er tomt eller baklengs —
/// kalleren avviser det; her regnes det bare.
pub fn antall_maneder(fra: (i32, u32), til: (i32, u32)) -> i32 {
    (til.0 - fra.0) * 12 + til.1 as i32 - fra.1 as i32 + 1
}

/// Beløpet for måned nummer `maned_nr` (1-basert) av `antall`.
///
/// Samme kontrakt som [`crate::anlegg::manedsbelop`], og den er hele
/// poenget: alle månedene unntatt den siste får det avrundede
/// grunnbeløpet, og den siste tar resten. Da summerer delene EKSAKT til
/// totalen — ingen flyttall, ingen øre som blir borte eller oppstår.
///
/// Negative totaler (en inntekt i hovedbokens fortegn) håndteres av at
/// heltallsdivisjon i Rust trunkerer mot null: grunnbeløpet får riktig
/// fortegn, og siste måned tar resten uansett retning.
pub fn manedsbelop(total_ore: i64, antall: i32, maned_nr: i32) -> i64 {
    debug_assert!(antall > 0 && maned_nr >= 1 && maned_nr <= antall);
    let basis = total_ore / antall as i64;
    if maned_nr < antall {
        basis
    } else {
        total_ore - basis * (antall as i64 - 1)
    }
}

/// Siste dag i måneden — periodiseringsbilaget dateres månedsslutt, som
/// avskrivningene.
pub fn maned_slutt(ar: i32, maned: u32) -> NaiveDate {
    let (neste_ar, neste_maned) = if maned == 12 {
        (ar + 1, 1)
    } else {
        (ar, maned + 1)
    };
    NaiveDate::from_ymd_opt(neste_ar, neste_maned, 1).expect("gyldig dato") - chrono::Days::new(1)
}

/// Måneden `steg` måneder etter (år, måned), 0-basert: `steg = 0` gir
/// måneden selv.
pub fn maned_pluss(fra: (i32, u32), steg: i32) -> (i32, u32) {
    let total = fra.0 * 12 + fra.1 as i32 - 1 + steg;
    (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32)
}

/// Hele planen: én rad per måned med dens beløp og bilagsdato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Manedsrad {
    pub ar: i32,
    pub maned: u32,
    /// Bilagsdatoen: siste dag i måneden.
    pub dato: NaiveDate,
    pub belop_ore: i64,
}

/// Planen fra og med `fra` til og med `til`. Tom når intervallet er
/// baklengs — kalleren avviser det med en melding, denne dikter ikke opp
/// en måned.
pub fn plan(total_ore: i64, fra: (i32, u32), til: (i32, u32)) -> Vec<Manedsrad> {
    let antall = antall_maneder(fra, til);
    if antall <= 0 {
        return Vec::new();
    }
    (1..=antall)
        .map(|nr| {
            let (ar, maned) = maned_pluss(fra, nr - 1);
            Manedsrad {
                ar,
                maned,
                dato: maned_slutt(ar, maned),
                belop_ore: manedsbelop(total_ore, antall, nr),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn maneder_are_counted_inclusively_across_year_ends() {
        assert_eq!(antall_maneder((2026, 1), (2026, 1)), 1);
        assert_eq!(antall_maneder((2026, 1), (2026, 12)), 12);
        assert_eq!(antall_maneder((2026, 11), (2027, 2)), 4);
        assert!(antall_maneder((2026, 5), (2026, 4)) <= 0, "baklengs");
    }

    #[test]
    fn month_arithmetic_wraps_both_ways() {
        assert_eq!(maned_pluss((2026, 1), 0), (2026, 1));
        assert_eq!(maned_pluss((2026, 12), 1), (2027, 1));
        assert_eq!(maned_pluss((2026, 3), 22), (2028, 1));
        // Negative steps are not used by `plan`, but the arithmetic must
        // not silently produce month 0 if anyone reaches for them.
        assert_eq!(maned_pluss((2026, 1), -1), (2025, 12));
    }

    #[test]
    fn month_end_handles_february_and_leap_years() {
        assert_eq!(maned_slutt(2026, 1), d(2026, 1, 31));
        assert_eq!(maned_slutt(2026, 2), d(2026, 2, 28));
        assert_eq!(maned_slutt(2028, 2), d(2028, 2, 29), "skuddår");
        assert_eq!(maned_slutt(2026, 12), d(2026, 12, 31));
    }

    /// The property the whole feature rests on: the parts sum EXACTLY to
    /// the total. An øre invented or lost here would land in the
    /// resultat and never balance against the source bilag.
    #[test]
    fn the_parts_sum_exactly_to_the_total() {
        // 10 000,01 over 3 months is the classic case: 3333,33 × 3 is
        // one øre short, and the last month has to carry it.
        let p = plan(1_000_001, (2026, 1), (2026, 3));
        assert_eq!(p.iter().map(|r| r.belop_ore).sum::<i64>(), 1_000_001);
        assert_eq!(p[0].belop_ore, 333_333);
        assert_eq!(p[2].belop_ore, 333_335, "siste måned tar resten");

        // Exhaustive over awkward totals and lengths.
        for total in [
            1, 7, 99, 100, 101, -1, -7, -99, -101, 1_000_001, -1_000_001, 12_345_678,
        ] {
            for antall in 1..=36 {
                let sum: i64 = (1..=antall).map(|nr| manedsbelop(total, antall, nr)).sum();
                assert_eq!(sum, total, "total {total} over {antall} måneder");
            }
        }
    }

    #[test]
    fn a_plan_carries_month_ends_across_the_year_boundary() {
        let p = plan(120_000, (2026, 11), (2027, 2));
        assert_eq!(p.len(), 4);
        assert_eq!(p[0].dato, d(2026, 11, 30));
        assert_eq!(p[1].dato, d(2026, 12, 31));
        assert_eq!(p[2].dato, d(2027, 1, 31));
        assert_eq!(p[3].dato, d(2027, 2, 28));
        assert!(p.iter().all(|r| r.belop_ore == 30_000));
    }

    /// An income periodisering is negative in ledger signs, and the
    /// remainder must not drift the other way.
    #[test]
    fn income_signs_survive_the_split() {
        let p = plan(-1_000_001, (2026, 1), (2026, 3));
        assert_eq!(p.iter().map(|r| r.belop_ore).sum::<i64>(), -1_000_001);
        assert!(p.iter().all(|r| r.belop_ore < 0), "{p:?}");
    }

    #[test]
    fn a_backwards_interval_yields_no_plan() {
        assert!(plan(100_000, (2026, 5), (2026, 4)).is_empty());
    }
}
