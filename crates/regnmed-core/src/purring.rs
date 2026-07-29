//! Payment follow-up, pure side: forsinkelsesrente, purring rules and
//! deterministic document rendering (docs/purring.md).
//!
//! All regelverk is pulled as data from the satsregister
//! ([`crate::sats`]) — this module knows *how* the rules are applied
//! (forsinkelsesrenteloven, inkassoforskriften, inkassoloven §9), never
//! which numbers are in force. Interest is computed in integer øre, and
//! the document renders deterministically so an archived claim can be
//! reproduced byte for byte.

use std::fmt;

use chrono::NaiveDate;

use crate::sats::{SatsPeriode, sats_on};
use crate::voucher::{EntryDraft, VoucherDraft};
use crate::{LedgerError, Ore};

/// The purretrapp. The steps are one-way: a claim can never go back to a
/// milder step, and inkasso itself is outside regnmed (it requires a
/// licence).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Steg {
    Paminnelse,
    Purring,
    Inkassovarsel,
}

impl Steg {
    pub fn as_str(self) -> &'static str {
        match self {
            Steg::Paminnelse => "paminnelse",
            Steg::Purring => "purring",
            Steg::Inkassovarsel => "inkassovarsel",
        }
    }

    pub fn parse(s: &str) -> Option<Steg> {
        match s {
            "paminnelse" => Some(Steg::Paminnelse),
            "purring" => Some(Steg::Purring),
            "inkassovarsel" => Some(Steg::Inkassovarsel),
            _ => None,
        }
    }

    pub fn tittel(self) -> &'static str {
        match self {
            Steg::Paminnelse => "Betalingspåminnelse",
            Steg::Purring => "Purring",
            Steg::Inkassovarsel => "Inkassovarsel",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PurringError {
    /// The send date is not after forfall — the claim is not yet due.
    IkkeForfalt {
        forfall: NaiveDate,
    },
    /// No forsinkelsesrente rate covers this day (before the earliest
    /// verified period) — we never guess a rate.
    ManglerRentesats {
        dato: NaiveDate,
    },
    /// The purretrapp is one-way.
    StegTilbake {
        siste: Steg,
        forsokt: Steg,
    },
    /// A betalingspåminnelse carries no gebyr; with one it is a purring.
    GebyrPaPaminnelse,
    /// Inkassoforskriften §1-2: gebyr no earlier than 14 days after forfall.
    GebyrForTidlig {
        tidligst: NaiveDate,
    },
    GebyrOverMaks {
        maks_ore: i64,
    },
    /// At most two gebyr-bearing steps per claim (inkassoforskriften §1-2).
    ForMangeGebyr,
    NegativtBelop,
    FristForSendedato,
    /// Inkassoloven §9: a payment deadline of at least 14 days.
    FristForKort {
        minst: NaiveDate,
    },
    RenteOverflow,
}

impl fmt::Display for PurringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PurringError::IkkeForfalt { forfall } => {
                write!(f, "fakturaen forfaller {forfall} og er ikke forfalt")
            }
            PurringError::ManglerRentesats { dato } => {
                write!(f, "ingen forsinkelsesrentesats dekker {dato}")
            }
            PurringError::StegTilbake { siste, forsokt } => write!(
                f,
                "purretrappen er enveis: {} er allerede sendt, kan ikke sende {}",
                siste.tittel(),
                forsokt.tittel()
            ),
            PurringError::GebyrPaPaminnelse => {
                write!(
                    f,
                    "en betalingspåminnelse er gebyrfri — bruk steget purring"
                )
            }
            PurringError::GebyrForTidlig { tidligst } => write!(
                f,
                "purregebyr tidligst 14 dager etter forfall, dvs. {tidligst} (inkassoforskriften §1-2)"
            ),
            PurringError::GebyrOverMaks { maks_ore } => {
                write!(f, "gebyret overstiger maksimalsatsen {} kr", Ore(*maks_ore))
            }
            PurringError::ForMangeGebyr => write!(
                f,
                "maksimalt to gebyrbelagte purringer/varsler per krav (inkassoforskriften §1-2)"
            ),
            PurringError::NegativtBelop => write!(f, "beløp kan ikke være negative"),
            PurringError::FristForSendedato => {
                write!(f, "betalingsfristen må være etter sendedatoen")
            }
            PurringError::FristForKort { minst } => write!(
                f,
                "et inkassovarsel krever minst 14 dagers betalingsfrist, tidligst {minst} (inkassoloven §9)"
            ),
            PurringError::RenteOverflow => write!(f, "rentebeløpet gikk over i64"),
        }
    }
}

impl std::error::Error for PurringError {}

/// One rate period of the interest run, as shown in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RentePeriode {
    pub fra: NaiveDate,
    pub til: NaiveDate,
    pub sats_bp: i64,
    pub dager: i64,
    pub rente_ore: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenteBeregning {
    pub perioder: Vec<RentePeriode>,
    pub sum_ore: i64,
}

/// `beløp × sats × dager / (10000 × 365)`, avrundet halvt vekk fra null.
fn rente_for_segment(belop_ore: i64, sats_bp: i64, dager: i64) -> Option<i64> {
    let teller = i128::from(belop_ore) * i128::from(sats_bp) * i128::from(dager);
    let nevner: i128 = 10_000 * 365;
    let avrundet = (2 * teller.abs() + nevner) / (2 * nevner);
    i64::try_from(avrundet * teller.signum()).ok()
}

/// Forsinkelsesrente under forsinkelsesrenteloven §2: interest runs from
/// the day after forfall through `til`, at whichever rate was in force on
/// each single day (actual days / 365). Every rate period is rounded on
/// its own, so the spesifikasjon in the document sums exactly to the
/// total.
pub fn forsinkelsesrente(
    belop_ore: i64,
    forfall: NaiveDate,
    til: NaiveDate,
    satser: &[SatsPeriode],
) -> Result<RenteBeregning, PurringError> {
    if belop_ore < 0 {
        return Err(PurringError::NegativtBelop);
    }
    let mut perioder = Vec::new();
    let mut sum: i64 = 0;
    let mut dag = forfall.succ_opt().expect("date in range");
    while dag <= til {
        let sats = sats_on(satser, "forsinkelsesrente", dag)
            .ok_or(PurringError::ManglerRentesats { dato: dag })?;
        let neste_skifte = satser
            .iter()
            .filter(|s| s.domene == "forsinkelsesrente" && s.valid_from > dag)
            .map(|s| s.valid_from)
            .min();
        let slutt = match neste_skifte {
            Some(skifte) if skifte <= til => skifte.pred_opt().expect("date in range"),
            _ => til,
        };
        let dager = (slutt - dag).num_days() + 1;
        let rente = rente_for_segment(belop_ore, sats, dager).ok_or(PurringError::RenteOverflow)?;
        sum = sum.checked_add(rente).ok_or(PurringError::RenteOverflow)?;
        perioder.push(RentePeriode {
            fra: dag,
            til: slutt,
            sats_bp: sats,
            dager,
            rente_ore: rente,
        });
        dag = slutt.succ_opt().expect("date in range");
    }
    Ok(RenteBeregning {
        perioder,
        sum_ore: sum,
    })
}

/// A previously sent step, in the shape the rule checks need.
#[derive(Debug, Clone, Copy)]
pub struct TidligereSkritt {
    pub steg: Steg,
    pub gebyr_ore: i64,
}

/// The rule check before a new step is recorded. `maks_gebyr_ore` is the
/// rate valid on the send date (purregebyr_maks or, for
/// næringsdrivende debtors, standardkompensasjon) — the caller does the
/// lookup, the rules are applied here.
pub fn valider_steg(
    steg: Steg,
    sent_date: NaiveDate,
    frist_date: NaiveDate,
    forfall: NaiveDate,
    gebyr_ore: i64,
    maks_gebyr_ore: i64,
    historikk: &[TidligereSkritt],
) -> Result<(), PurringError> {
    if gebyr_ore < 0 {
        return Err(PurringError::NegativtBelop);
    }
    if sent_date <= forfall {
        return Err(PurringError::IkkeForfalt { forfall });
    }
    if let Some(siste) = historikk.iter().map(|s| s.steg).max()
        && steg < siste
    {
        return Err(PurringError::StegTilbake {
            siste,
            forsokt: steg,
        });
    }
    if frist_date <= sent_date {
        return Err(PurringError::FristForSendedato);
    }
    if steg == Steg::Inkassovarsel {
        let minst = sent_date + chrono::Days::new(14);
        if frist_date < minst {
            return Err(PurringError::FristForKort { minst });
        }
    }
    if gebyr_ore > 0 {
        if steg == Steg::Paminnelse {
            return Err(PurringError::GebyrPaPaminnelse);
        }
        let tidligst = forfall + chrono::Days::new(14);
        if sent_date < tidligst {
            return Err(PurringError::GebyrForTidlig { tidligst });
        }
        if gebyr_ore > maks_gebyr_ore {
            return Err(PurringError::GebyrOverMaks {
                maks_ore: maks_gebyr_ore,
            });
        }
        if historikk.iter().filter(|s| s.gebyr_ore > 0).count() >= 2 {
            return Err(PurringError::ForMangeGebyr);
        }
    }
    Ok(())
}

/// The bilag when gebyr and/or interest is claimed: debit the reskontro
/// receivable (with the customer — the claim becomes an open item on the
/// same reskontro as the faktura), credit the revenue accounts. Called
/// only when the sum is positive; a step with neither gebyr nor interest
/// posts nothing.
#[allow(clippy::too_many_arguments)]
pub fn build_krav_voucher(
    journal_code: &str,
    sent_date: NaiveDate,
    steg: Steg,
    invoice_no: i64,
    party_no: &str,
    receivable_account: &str,
    gebyr_account: &str,
    gebyr_ore: i64,
    rente_account: &str,
    rente_ore: i64,
) -> Result<VoucherDraft, LedgerError> {
    let mut entries = vec![EntryDraft {
        account_number: receivable_account.to_string(),
        amount: Ore(gebyr_ore + rente_ore),
        vat_code: None,
        description: None,
        party_no: Some(party_no.to_string()),
        avdeling: None,
        prosjekt: None,
        valuta: None,
    }];
    if gebyr_ore > 0 {
        entries.push(EntryDraft {
            account_number: gebyr_account.to_string(),
            amount: Ore(-gebyr_ore),
            vat_code: None,
            description: Some("Purregebyr".into()),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
    }
    if rente_ore > 0 {
        entries.push(EntryDraft {
            account_number: rente_account.to_string(),
            amount: Ore(-rente_ore),
            vat_code: None,
            description: Some("Forsinkelsesrente".into()),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
    }
    let draft = VoucherDraft {
        journal_code: journal_code.to_string(),
        voucher_date: sent_date,
        description: format!("{} faktura {}", steg.tittel(), invoice_no),
        reverses: None,
        entries,
    };
    draft.validate()?;
    Ok(draft)
}

/// Everything the document needs, gathered by the persistence layer. No
/// clock and
/// ingen oppslag her — samme input gir samme bytes for alltid.
#[derive(Debug, Clone)]
pub struct PurringDokument {
    pub steg: Steg,
    pub selskap: String,
    pub orgnr: String,
    pub kunde_navn: String,
    pub kunde_nr: String,
    pub faktura_no: i64,
    pub faktura_dato: NaiveDate,
    pub forfall: NaiveDate,
    pub sent_date: NaiveDate,
    pub frist_date: NaiveDate,
    pub restbelop_ore: i64,
    /// The interest spesifikasjon when interest is claimed; empty otherwise.
    pub rente: RenteBeregning,
    pub gebyr_ore: i64,
    pub kid: String,
}

fn prosent(bp: i64) -> String {
    format!("{},{:02} %", bp / 100, bp % 100)
}

/// Deterministic plain-text rendering of the claim, stored when the step
/// is recorded and reissuable forever (`?format=tekst`).
pub fn render_dokument(dok: &PurringDokument) -> String {
    let mut out = String::with_capacity(1024);
    let mut line = |s: &str| {
        out.push_str(s);
        out.push('\n');
    };
    let tittel = dok.steg.tittel().to_uppercase();
    line(&tittel);
    line(&"=".repeat(tittel.chars().count()));
    line("");
    line(&format!("Fra:    {} ({})", dok.selskap, dok.orgnr));
    line(&format!(
        "Til:    {} (kunde {})",
        dok.kunde_navn, dok.kunde_nr
    ));
    line(&format!("Sendt:  {}", dok.sent_date));
    line("");
    line(&format!(
        "Faktura {} av {} forfalt {}.",
        dok.faktura_no, dok.faktura_dato, dok.forfall
    ));
    line("");
    let mut total = dok.restbelop_ore;
    line(&format!(
        "Utestående beløp:                        {:>12} kr",
        Ore(dok.restbelop_ore).to_string()
    ));
    if dok.rente.sum_ore > 0 {
        line("Forsinkelsesrente (forsinkelsesrenteloven §2):");
        for p in &dok.rente.perioder {
            line(&format!(
                "  {} – {}  {:>8}  {:>3} dager  {:>10} kr",
                p.fra,
                p.til,
                prosent(p.sats_bp),
                p.dager,
                Ore(p.rente_ore).to_string()
            ));
        }
        total += dok.rente.sum_ore;
    }
    if dok.gebyr_ore > 0 {
        line(&format!(
            "Purregebyr (inkassoforskriften §1-2):    {:>12} kr",
            Ore(dok.gebyr_ore).to_string()
        ));
        total += dok.gebyr_ore;
    }
    line("");
    line(&format!(
        "Å betale:                                {:>12} kr",
        Ore(total).to_string()
    ));
    line(&format!("Betales innen:  {}", dok.frist_date));
    line(&format!("KID:            {}", dok.kid));
    if dok.steg == Steg::Inkassovarsel {
        line("");
        line("Dette er et inkassovarsel etter inkassoloven §9. Betales ikke");
        line("kravet innen fristen, kan det uten ytterligere varsel sendes");
        line("til inkasso. Videre inndriving skjer hos foretak med");
        line("inkassobevilling — regnmed utfører ikke inkasso.");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn satser() -> Vec<SatsPeriode> {
        [
            (date(2025, 1, 1), 1250),
            (date(2025, 7, 1), 1225),
            (date(2026, 1, 1), 1200),
        ]
        .into_iter()
        .map(|(valid_from, verdi)| SatsPeriode {
            domene: "forsinkelsesrente".into(),
            valid_from,
            verdi,
        })
        .collect()
    }

    #[test]
    fn interest_is_segmented_across_a_rate_change_and_is_pinned() {
        // 10 000 kr, forfall 15.6.2025, krav per 15.7.2025: 15 dager à
        // 12,50 % + 15 dager à 12,25 %, faktiske dager / 365.
        let beregning =
            forsinkelsesrente(1_000_000, date(2025, 6, 15), date(2025, 7, 15), &satser()).unwrap();
        assert_eq!(beregning.perioder.len(), 2);
        assert_eq!(
            beregning.perioder[0].fra,
            date(2025, 6, 16),
            "fra dagen etter forfall"
        );
        assert_eq!(beregning.perioder[0].til, date(2025, 6, 30));
        assert_eq!(beregning.perioder[0].dager, 15);
        assert_eq!(
            beregning.perioder[0].rente_ore, 5137,
            "10000 × 12,5 % × 15/365"
        );
        assert_eq!(beregning.perioder[1].fra, date(2025, 7, 1));
        assert_eq!(beregning.perioder[1].dager, 15);
        assert_eq!(beregning.perioder[1].rente_ore, 5034);
        assert_eq!(beregning.sum_ore, 10_171, "periodene summerer til totalen");
    }

    #[test]
    fn no_interest_through_the_forfall_day_itself() {
        let beregning =
            forsinkelsesrente(1_000_000, date(2025, 6, 15), date(2025, 6, 15), &satser()).unwrap();
        assert_eq!(beregning.sum_ore, 0);
        assert!(beregning.perioder.is_empty());
    }

    #[test]
    fn a_missing_rate_fails_loudly_never_guesses() {
        let err = forsinkelsesrente(1_000_000, date(2024, 12, 15), date(2025, 1, 15), &satser())
            .unwrap_err();
        assert_eq!(
            err,
            PurringError::ManglerRentesats {
                dato: date(2024, 12, 16)
            }
        );
    }

    #[test]
    fn a_negative_amount_is_rejected() {
        assert_eq!(
            forsinkelsesrente(-1, date(2025, 6, 15), date(2025, 7, 15), &satser()).unwrap_err(),
            PurringError::NegativtBelop
        );
    }

    fn ok_steg(
        steg: Steg,
        sent: NaiveDate,
        frist: NaiveDate,
        gebyr: i64,
        historikk: &[TidligereSkritt],
    ) -> Result<(), PurringError> {
        valider_steg(steg, sent, frist, date(2026, 1, 10), gebyr, 3800, historikk)
    }

    #[test]
    fn the_step_rules_enforce_the_statutory_requirements() {
        let ingen = &[][..];
        // Påminnelse the day after forfall, without gebyr: fine.
        assert_eq!(
            ok_steg(
                Steg::Paminnelse,
                date(2026, 1, 11),
                date(2026, 1, 25),
                0,
                ingen
            ),
            Ok(())
        );
        // Not yet due.
        assert!(matches!(
            ok_steg(
                Steg::Purring,
                date(2026, 1, 10),
                date(2026, 1, 30),
                0,
                ingen
            ),
            Err(PurringError::IkkeForfalt { .. })
        ));
        // A gebyr on a påminnelse is not a thing.
        assert_eq!(
            ok_steg(
                Steg::Paminnelse,
                date(2026, 1, 25),
                date(2026, 2, 10),
                3500,
                ingen
            ),
            Err(PurringError::GebyrPaPaminnelse)
        );
        // Gebyr before 14 days after forfall (inkassoforskriften §1-2).
        assert_eq!(
            ok_steg(
                Steg::Purring,
                date(2026, 1, 20),
                date(2026, 2, 5),
                3500,
                ingen
            ),
            Err(PurringError::GebyrForTidlig {
                tidligst: date(2026, 1, 24)
            })
        );
        // Gebyr over maksimalsatsen.
        assert_eq!(
            ok_steg(
                Steg::Purring,
                date(2026, 1, 24),
                date(2026, 2, 10),
                3900,
                ingen
            ),
            Err(PurringError::GebyrOverMaks { maks_ore: 3800 })
        );
        // The purretrapp is one-way.
        let etter_varsel = &[TidligereSkritt {
            steg: Steg::Inkassovarsel,
            gebyr_ore: 0,
        }][..];
        assert!(matches!(
            ok_steg(
                Steg::Purring,
                date(2026, 2, 1),
                date(2026, 2, 15),
                0,
                etter_varsel
            ),
            Err(PurringError::StegTilbake { .. })
        ));
        // Maks to gebyrbelagte skritt.
        let to_gebyr = &[
            TidligereSkritt {
                steg: Steg::Purring,
                gebyr_ore: 3500,
            },
            TidligereSkritt {
                steg: Steg::Purring,
                gebyr_ore: 3500,
            },
        ][..];
        assert_eq!(
            ok_steg(
                Steg::Inkassovarsel,
                date(2026, 3, 1),
                date(2026, 3, 16),
                3800,
                to_gebyr
            ),
            Err(PurringError::ForMangeGebyr)
        );
        // A third step without gebyr is still allowed.
        assert_eq!(
            ok_steg(
                Steg::Inkassovarsel,
                date(2026, 3, 1),
                date(2026, 3, 16),
                0,
                to_gebyr
            ),
            Ok(())
        );
    }

    #[test]
    fn an_inkassovarsel_requires_at_least_a_14_day_deadline() {
        assert_eq!(
            ok_steg(
                Steg::Inkassovarsel,
                date(2026, 2, 1),
                date(2026, 2, 14),
                0,
                &[]
            ),
            Err(PurringError::FristForKort {
                minst: date(2026, 2, 15)
            })
        );
        assert_eq!(
            ok_steg(
                Steg::Inkassovarsel,
                date(2026, 2, 1),
                date(2026, 2, 15),
                0,
                &[]
            ),
            Ok(())
        );
    }

    #[test]
    fn a_deadline_before_the_send_date_is_rejected() {
        assert_eq!(
            ok_steg(Steg::Purring, date(2026, 2, 1), date(2026, 2, 1), 0, &[]),
            Err(PurringError::FristForSendedato)
        );
    }

    #[test]
    fn the_claim_bilag_balances_and_carries_the_customer() {
        let draft = build_krav_voucher(
            "GL",
            date(2026, 2, 1),
            Steg::Purring,
            7,
            "10000",
            "1500",
            "3950",
            3800,
            "8050",
            10_171,
        )
        .unwrap();
        assert_eq!(draft.description, "Purring faktura 7");
        assert_eq!(draft.entries.len(), 3);
        assert_eq!(draft.entries[0].amount, Ore(13_971));
        assert_eq!(draft.entries[0].party_no.as_deref(), Some("10000"));
        assert_eq!(draft.entries.iter().map(|e| e.amount.0).sum::<i64>(), 0);
        // Bare rente, intet gebyr: to linjer.
        let bare_rente = build_krav_voucher(
            "GL",
            date(2026, 2, 1),
            Steg::Inkassovarsel,
            7,
            "10000",
            "1500",
            "3950",
            0,
            "8050",
            500,
        )
        .unwrap();
        assert_eq!(bare_rente.entries.len(), 2);
    }

    fn dokument(steg: Steg) -> PurringDokument {
        PurringDokument {
            steg,
            selskap: "Demo AS".into(),
            orgnr: "999888777".into(),
            kunde_navn: "Kari Kunde".into(),
            kunde_nr: "10000".into(),
            faktura_no: 7,
            faktura_dato: date(2025, 6, 1),
            forfall: date(2025, 6, 15),
            sent_date: date(2025, 7, 15),
            frist_date: date(2025, 7, 29),
            restbelop_ore: 1_000_000,
            rente: forsinkelsesrente(1_000_000, date(2025, 6, 15), date(2025, 7, 15), &satser())
                .unwrap(),
            gebyr_ore: 3500,
            kid: "000000071".into(),
        }
    }

    #[test]
    fn the_document_renders_deterministically_with_its_spesifikasjon() {
        let a = render_dokument(&dokument(Steg::Purring));
        assert_eq!(a, render_dokument(&dokument(Steg::Purring)));
        assert!(a.starts_with("PURRING\n=======\n"));
        assert!(a.contains("Faktura 7 av 2025-06-01 forfalt 2025-06-15."));
        assert!(a.contains("12,50 %"));
        assert!(a.contains("15 dager"));
        assert!(a.contains("Purregebyr (inkassoforskriften §1-2)"));
        // 10 000,00 + 101,71 rente + 35,00 gebyr.
        assert!(a.contains("10136,71"));
        assert!(a.contains("KID:            000000071"));
        assert!(
            !a.contains("inkassoloven"),
            "purring er ikke et inkassovarsel"
        );
    }

    #[test]
    fn the_inkassovarsel_carries_the_statutory_text_and_stops_there() {
        let text = render_dokument(&dokument(Steg::Inkassovarsel));
        assert!(text.starts_with("INKASSOVARSEL\n=============\n"));
        assert!(text.contains("inkassoloven §9"));
        assert!(text.contains("regnmed utfører ikke inkasso"));
    }
}
