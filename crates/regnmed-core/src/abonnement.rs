//! Abonnementsstatus (#65, docs/abonnement.md).
//!
//! Pure logic: given when the company was created and what coverage
//! exists, what is the status today? The rows live in the database
//! (`regnmed-db::abonnement`); the rule lives here, in one place,
//! testable without I/O.
//!
//! The principle from the issue is absolute and is repeated here because
//! the code below enforces it: **the hovedbok is never taken hostage.**
//! An expired abonnement blocks writing — like a locked periode — never
//! reading, and export always works.

use chrono::NaiveDate;

/// Trial period for new companies, counted from creation.
pub const PROVETID_DAGER: i64 = 30;

/// Payment deadline after coverage (or the trial) ran out: writing
/// still works, but the portal warns. Only after the deadline is it
/// blocked. Mirrors forfall plus a reasonable margin on the abonnement
/// faktura.
pub const FRIST_DAGER: i64 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// A row covers today's date.
    Aktiv,
    /// No coverage, but the company is inside its trial period.
    Prove { til: NaiveDate },
    /// Coverage (or the trial) is over, but the deadline is still
    /// running: writing works, the warning stands.
    Frist { sperres: NaiveDate },
    /// The deadline has passed: writing actions are refused.
    Sperret { siden: NaiveDate },
}

impl Status {
    /// Should writing actions be refused?
    pub fn sperret(&self) -> bool {
        matches!(self, Status::Sperret { .. })
    }

    /// Machine-readable name, used in `/me` and the portal.
    pub fn slug(&self) -> &'static str {
        match self {
            Status::Aktiv => "aktiv",
            Status::Prove { .. } => "prove",
            Status::Frist { .. } => "frist",
            Status::Sperret { .. } => "sperret",
        }
    }
}

/// The status as of today.
///
/// - `dekket_i_dag`: is there an abonnement row covering `idag`
///   (`valid_from <= idag` and `valid_to` null or `> idag` — `valid_to`
///   is EXCLUSIVE, as everywhere else).
/// - `siste_slutt`: the latest `valid_to` among the rows that have
///   expired, if any. The deadline runs from the later of this and the
///   end of the trial, so a company that cancelled after two years is
///   not blocked "14 days after the trial" long past.
pub fn status(
    opprettet: NaiveDate,
    dekket_i_dag: bool,
    siste_slutt: Option<NaiveDate>,
    idag: NaiveDate,
) -> Status {
    if dekket_i_dag {
        return Status::Aktiv;
    }
    let prove_til = opprettet + chrono::Days::new(PROVETID_DAGER as u64);
    if siste_slutt.is_none() && idag < prove_til {
        return Status::Prove { til: prove_til };
    }
    let grunnlag = siste_slutt.map_or(prove_til, |s| s.max(prove_til));
    let sperres = grunnlag + chrono::Days::new(FRIST_DAGER as u64);
    if idag < sperres {
        Status::Frist { sperres }
    } else {
        Status::Sperret { siden: sperres }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn coverage_means_active_whatever_the_age() {
        assert_eq!(
            status(d(2020, 1, 1), true, Some(d(2023, 1, 1)), d(2026, 7, 28)),
            Status::Aktiv
        );
    }

    #[test]
    fn a_new_company_is_in_its_trial_period() {
        let opprettet = d(2026, 7, 1);
        assert_eq!(
            status(opprettet, false, None, d(2026, 7, 28)),
            Status::Prove {
                til: d(2026, 7, 31)
            }
        );
        // The trial's last day is the day BEFORE `til` — the bound is
        // exclusive like every other.
        assert_eq!(
            status(opprettet, false, None, d(2026, 7, 30)),
            Status::Prove {
                til: d(2026, 7, 31)
            }
        );
        assert!(matches!(
            status(opprettet, false, None, d(2026, 7, 31)),
            Status::Frist { .. }
        ));
    }

    #[test]
    fn the_trial_becomes_a_deadline_and_then_a_block() {
        let opprettet = d(2026, 1, 1);
        let prove_til = d(2026, 1, 31);
        let sperres = d(2026, 2, 14);
        assert_eq!(
            status(opprettet, false, None, d(2026, 2, 1)),
            Status::Frist { sperres }
        );
        assert_eq!(
            status(opprettet, false, None, d(2026, 2, 13)),
            Status::Frist { sperres }
        );
        assert_eq!(
            status(opprettet, false, None, sperres),
            Status::Sperret { siden: sperres }
        );
        let _ = prove_til;
    }

    #[test]
    fn a_cancelled_abonnement_counts_from_its_own_end_not_the_trial() {
        // Company from 2024, abonnement that ran out 2026-07-01: the
        // deadline runs from that end date, not from the long-expired
        // trial period.
        let s = status(d(2024, 1, 1), false, Some(d(2026, 7, 1)), d(2026, 7, 10));
        assert_eq!(
            s,
            Status::Frist {
                sperres: d(2026, 7, 15)
            }
        );
        assert!(
            status(d(2024, 1, 1), false, Some(d(2026, 7, 1)), d(2026, 7, 15)).sperret(),
            "fristen er eksklusiv: sperret PÅ dagen"
        );
    }

    #[test]
    fn an_old_end_date_does_not_beat_the_trial_period() {
        // A row that ended BEFORE the trial was over (a short paid
        // period at start-up): the deadline runs from the later of the
        // two — the trial, in this case.
        let opprettet = d(2026, 7, 1);
        let s = status(opprettet, false, Some(d(2026, 7, 10)), d(2026, 8, 5));
        assert_eq!(
            s,
            Status::Frist {
                sperres: d(2026, 8, 14)
            }
        );
    }
}
