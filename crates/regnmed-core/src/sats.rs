//! Satsregisteret, pure side: lookup by date and staleness detection
//! (docs/regelverk.md).
//!
//! Every regelverksstyrt sats (forsinkelsesrente, purregebyr, statens
//! km-sats, terskelverdier …) lives in the dated `sats` table; this
//! module answers "what was the sats on this date" — mirroring
//! [`crate::mva::rate_on`] — and "which domains look outdated", so the
//! yearly regelverksrevisjon is verified by the revisjonsrapport
//! instead of remembered by a human.

use chrono::NaiveDate;

#[derive(Debug, Clone)]
pub struct SatsPeriode {
    pub domene: String,
    pub valid_from: NaiveDate,
    /// Integer in the domain's unit (bp / øre / øre per km).
    pub verdi: i64,
}

/// The sats valid on `dato`, or `None` when the domain has no period
/// covering it (before its earliest verified date — never guessed).
pub fn sats_on(satser: &[SatsPeriode], domene: &str, dato: NaiveDate) -> Option<i64> {
    satser
        .iter()
        .filter(|s| s.domene == domene && s.valid_from <= dato)
        .max_by_key(|s| s.valid_from)
        .map(|s| s.verdi)
}

/// Expected update cadence per domain, in days, with slack for the
/// authorities publishing close to the effective date. Domains not
/// listed change rarely and are exempt from staleness monitoring.
const KADENSER: &[(&str, i64)] = &[
    // Fastsatt hvert halvår (1/1 og 1/7): stale when the newest period
    // started more than ~7 months ago.
    ("forsinkelsesrente", 215),
    ("standardkompensasjon", 215),
    // Fastsatt årlig: stale when the newest period started more than
    // ~13 months ago.
    ("inkassosats", 400),
    ("purregebyr_maks", 400),
    ("km_godtgjorelse", 400),
    ("km_godtgjorelse_trekkfri", 400),
];

#[derive(Debug, PartialEq, Eq)]
pub struct ForeldetSats {
    pub domene: String,
    /// Newest valid_from on record, or None when the domain is missing
    /// entirely (also a finding — the register should carry it).
    pub siste: Option<NaiveDate>,
}

/// Domains whose newest period is older than their known change
/// cadence — the machine's side of the yearly regelverksrevisjon.
pub fn foreldede_domener(satser: &[SatsPeriode], idag: NaiveDate) -> Vec<ForeldetSats> {
    KADENSER
        .iter()
        .filter_map(|(domene, maks_alder)| {
            let siste = satser
                .iter()
                .filter(|s| s.domene == *domene)
                .map(|s| s.valid_from)
                .max();
            let foreldet = match siste {
                Some(date) => (idag - date).num_days() > *maks_alder,
                None => true,
            };
            foreldet.then(|| ForeldetSats {
                domene: (*domene).to_string(),
                siste,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn periode(domene: &str, y: i32, m: u32, d: u32, verdi: i64) -> SatsPeriode {
        SatsPeriode {
            domene: domene.into(),
            valid_from: date(y, m, d),
            verdi,
        }
    }

    fn rente() -> Vec<SatsPeriode> {
        vec![
            periode("forsinkelsesrente", 2025, 1, 1, 1250),
            periode("forsinkelsesrente", 2025, 7, 1, 1225),
            periode("forsinkelsesrente", 2026, 1, 1, 1200),
            periode("forsinkelsesrente", 2026, 7, 1, 1225),
        ]
    }

    #[test]
    fn lookup_picks_the_period_covering_the_date() {
        let satser = rente();
        assert_eq!(
            sats_on(&satser, "forsinkelsesrente", date(2025, 6, 30)),
            Some(1250)
        );
        assert_eq!(
            sats_on(&satser, "forsinkelsesrente", date(2025, 7, 1)),
            Some(1225)
        );
        assert_eq!(
            sats_on(&satser, "forsinkelsesrente", date(2026, 12, 31)),
            Some(1225)
        );
        // Before the earliest verified period: None, never a guess.
        assert_eq!(
            sats_on(&satser, "forsinkelsesrente", date(2024, 12, 31)),
            None
        );
        assert_eq!(sats_on(&satser, "ukjent", date(2026, 1, 1)), None);
    }

    #[test]
    fn fresh_domains_are_not_flagged() {
        let mut satser = rente();
        for (domene, y) in [
            ("standardkompensasjon", 2026),
            ("inkassosats", 2026),
            ("purregebyr_maks", 2026),
            ("km_godtgjorelse", 2026),
            ("km_godtgjorelse_trekkfri", 2026),
        ] {
            satser.push(periode(domene, y, 1, 1, 1));
        }
        assert_eq!(foreldede_domener(&satser, date(2026, 7, 23)), vec![]);
    }

    #[test]
    fn stale_and_missing_domains_are_flagged() {
        // Only a 2025-07 forsinkelsesrente on record: by mid-2026 the
        // half-yearly cadence has been missed twice.
        let satser = vec![periode("forsinkelsesrente", 2025, 7, 1, 1225)];
        let funn = foreldede_domener(&satser, date(2026, 7, 23));
        assert!(
            funn.iter()
                .any(|f| f.domene == "forsinkelsesrente" && f.siste == Some(date(2025, 7, 1)))
        );
        // Every other monitored domain is missing entirely — also findings.
        assert!(
            funn.iter()
                .any(|f| f.domene == "inkassosats" && f.siste.is_none())
        );
        assert_eq!(funn.len(), KADENSER.len());
    }

    #[test]
    fn rare_change_thresholds_are_exempt() {
        // aktiveringsgrense from 2024 must never be "stale" in 2026.
        let satser = vec![periode("aktiveringsgrense", 2024, 1, 1, 3_000_000)];
        assert!(
            foreldede_domener(&satser, date(2026, 7, 23))
                .iter()
                .all(|f| f.domene != "aktiveringsgrense")
        );
    }
}
