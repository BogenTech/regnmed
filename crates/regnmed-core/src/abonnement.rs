//! Abonnementsstatus (#65, docs/abonnement.md).
//!
//! Ren logikk: gitt når selskapet ble opprettet og hvilken dekning som
//! finnes, hva er statusen i dag? Radene bor i databasen
//! (`regnmed-db::abonnement`); regelen bor her, ett sted, testbar uten
//! I/O.
//!
//! Prinsippet fra saken er ufravikelig og gjentas her fordi koden under
//! håndhever det: **hovedboken tas aldri som gissel.** Et utløpt
//! abonnement sperrer skriving — som en låst periode — aldri lesing,
//! og eksport virker alltid.

use chrono::NaiveDate;

/// Prøvetid for nye selskaper, regnet fra opprettelsen.
pub const PROVETID_DAGER: i64 = 30;

/// Betalingsfrist etter at dekningen (eller prøvetiden) løp ut:
/// skriving virker, men portalen varsler. Først etter fristen sperres
/// det. Speiler forfall + rimelig margin på abonnementsfakturaen.
pub const FRIST_DAGER: i64 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// En rad dekker dagens dato.
    Aktiv,
    /// Ingen dekning, men selskapet er innenfor prøvetiden.
    Prove { til: NaiveDate },
    /// Dekningen (eller prøvetiden) er ute, men fristen løper ennå:
    /// skriving virker, varselet står.
    Frist { sperres: NaiveDate },
    /// Fristen er ute: skrivende handlinger avvises.
    Sperret { siden: NaiveDate },
}

impl Status {
    /// Skal skrivende handlinger avvises?
    pub fn sperret(&self) -> bool {
        matches!(self, Status::Sperret { .. })
    }

    /// Maskinlesbart navn, brukt i `/me` og portalen.
    pub fn slug(&self) -> &'static str {
        match self {
            Status::Aktiv => "aktiv",
            Status::Prove { .. } => "prove",
            Status::Frist { .. } => "frist",
            Status::Sperret { .. } => "sperret",
        }
    }
}

/// Statusen i dag.
///
/// - `dekket_i_dag`: finnes en abonnementsrad som dekker `idag`
///   (`valid_from <= idag` og `valid_to` er null eller `> idag` —
///   `valid_to` er EKSKLUSIV, som overalt ellers).
/// - `siste_slutt`: siste `valid_to` blant radene som er utløpt, om
///   noen. Fristen regnes fra det seneste av denne og prøvetidens
///   slutt, så et selskap som sa opp etter to år ikke sperres «14 dager
///   etter prøvetiden» for lengst.
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
    fn dekning_gir_aktiv_uansett_alder() {
        assert_eq!(
            status(d(2020, 1, 1), true, Some(d(2023, 1, 1)), d(2026, 7, 28)),
            Status::Aktiv
        );
    }

    #[test]
    fn nytt_selskap_er_i_provetid() {
        let opprettet = d(2026, 7, 1);
        assert_eq!(
            status(opprettet, false, None, d(2026, 7, 28)),
            Status::Prove {
                til: d(2026, 7, 31)
            }
        );
        // Prøvetidens siste dag er dagen FØR `til` — grensen er
        // eksklusiv som alle andre.
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
    fn provetid_gar_over_i_frist_og_saa_sperre() {
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
    fn oppsagt_abonnement_far_frist_fra_egen_slutt_ikke_provetiden() {
        // Selskap fra 2024, abonnement som løp ut 2026-07-01: fristen
        // regnes fra sluttdatoen, ikke fra den for lengst utløpte
        // prøvetiden.
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
    fn gammel_slutt_vinner_ikke_over_provetiden() {
        // En rad som løp ut FØR prøvetiden var over (kort betalt
        // periode ved oppstart): fristen regnes fra det seneste av de
        // to — prøvetiden i dette tilfellet.
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
