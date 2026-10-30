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
pub struct RatePeriod {
    pub domain: String,
    pub valid_from: NaiveDate,
    /// Integer in the domain's unit (bp / øre / øre per km).
    pub value: i64,
}

/// The rate valid on `date`, or `None` when the domain has no period
/// covering it (before its earliest verified date — never guessed).
pub fn value_on(rates: &[RatePeriod], domain: &str, date: NaiveDate) -> Option<i64> {
    rates
        .iter()
        .filter(|s| s.domain == domain && s.valid_from <= date)
        .max_by_key(|s| s.valid_from)
        .map(|s| s.value)
}

/// Expected update cadence per domain, in days, with slack for the
/// authorities publishing close to the effective date. Domains not
/// listed change rarely and are exempt from staleness monitoring.
const CADENCES: &[(&str, i64)] = &[
    // Set every half year (1/1 and 1/7): stale when the newest period
    // started more than ~7 months ago.
    ("forsinkelsesrente", 215),
    ("standardkompensasjon", 215),
    // Set annually: stale when the newest period started more than
    // ~13 months ago.
    ("inkassosats", 400),
    ("purregebyr_maks", 400),
    ("km_godtgjorelse", 400),
    ("km_godtgjorelse_trekkfri", 400),
];

#[derive(Debug, PartialEq, Eq)]
pub struct StaleRate {
    pub domain: String,
    /// Newest valid_from on record, or None when the domain is missing
    /// entirely (also a finding — the register should carry it).
    pub latest: Option<NaiveDate>,
}

/// Domains whose newest period is older than their known change
/// cadence — the machine's side of the yearly regelverksrevisjon.
pub fn stale_domains(rates: &[RatePeriod], today: NaiveDate) -> Vec<StaleRate> {
    CADENCES
        .iter()
        .filter_map(|(domain, max_age)| {
            let latest = rates
                .iter()
                .filter(|s| s.domain == *domain)
                .map(|s| s.valid_from)
                .max();
            let stale = match latest {
                Some(date) => (today - date).num_days() > *max_age,
                None => true,
            };
            stale.then(|| StaleRate {
                domain: (*domain).to_string(),
                latest,
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

    fn period(domain: &str, y: i32, m: u32, d: u32, value: i64) -> RatePeriod {
        RatePeriod {
            domain: domain.into(),
            valid_from: date(y, m, d),
            value,
        }
    }

    fn interest() -> Vec<RatePeriod> {
        vec![
            period("forsinkelsesrente", 2025, 1, 1, 1250),
            period("forsinkelsesrente", 2025, 7, 1, 1225),
            period("forsinkelsesrente", 2026, 1, 1, 1200),
            period("forsinkelsesrente", 2026, 7, 1, 1225),
        ]
    }

    #[test]
    fn lookup_picks_the_period_covering_the_date() {
        let rates = interest();
        assert_eq!(
            value_on(&rates, "forsinkelsesrente", date(2025, 6, 30)),
            Some(1250)
        );
        assert_eq!(
            value_on(&rates, "forsinkelsesrente", date(2025, 7, 1)),
            Some(1225)
        );
        assert_eq!(
            value_on(&rates, "forsinkelsesrente", date(2026, 12, 31)),
            Some(1225)
        );
        // Before the earliest verified period: None, never a guess.
        assert_eq!(
            value_on(&rates, "forsinkelsesrente", date(2024, 12, 31)),
            None
        );
        assert_eq!(value_on(&rates, "ukjent", date(2026, 1, 1)), None);
    }

    #[test]
    fn fresh_domains_are_not_flagged() {
        let mut rates = interest();
        for (domain, y) in [
            ("standardkompensasjon", 2026),
            ("inkassosats", 2026),
            ("purregebyr_maks", 2026),
            ("km_godtgjorelse", 2026),
            ("km_godtgjorelse_trekkfri", 2026),
        ] {
            rates.push(period(domain, y, 1, 1, 1));
        }
        assert_eq!(stale_domains(&rates, date(2026, 7, 23)), vec![]);
    }

    #[test]
    fn stale_and_missing_domains_are_flagged() {
        // Only a 2025-07 forsinkelsesrente on record: by mid-2026 the
        // half-yearly cadence has been missed twice.
        let rates = vec![period("forsinkelsesrente", 2025, 7, 1, 1225)];
        let findings = stale_domains(&rates, date(2026, 7, 23));
        assert!(
            findings
                .iter()
                .any(|f| f.domain == "forsinkelsesrente" && f.latest == Some(date(2025, 7, 1)))
        );
        // Every other monitored domain is missing entirely — also findings.
        assert!(
            findings
                .iter()
                .any(|f| f.domain == "inkassosats" && f.latest.is_none())
        );
        assert_eq!(findings.len(), CADENCES.len());
    }

    #[test]
    fn rare_change_thresholds_are_exempt() {
        // aktiveringsgrense from 2024 must never be "stale" in 2026.
        let rates = vec![period("aktiveringsgrense", 2024, 1, 1, 3_000_000)];
        assert!(
            stale_domains(&rates, date(2026, 7, 23))
                .iter()
                .all(|f| f.domain != "aktiveringsgrense")
        );
    }
}
