//! SAF-T migration import: turns a parsed SAF-T file into real,
//! hash-chained history in ONE database transaction.
//!
//! Rules that keep the migration honest:
//! - Only into an **empty ledger** (chain head at genesis), or on top of
//!   **previous SAF-T imports alone** — several source systems (Conta)
//!   export one file per fiscal year, so history arrives as a series of
//!   files, oldest first. Migration still happens before day-to-day
//!   bookkeeping: one ordinary voucher in the ledger closes the door.
//!   Each import is all-or-nothing: any error rolls back the lot.
//! - A follow-up file must **continue exactly where the imported history
//!   stopped**: balance accounts (class 1–2) must open at the booked
//!   balance, and resultat accounts at the booked balance of the file's
//!   own fiscal year (the exporting system resets them at year end,
//!   while regnmed never posts year-end closings — udisponert resultat
//!   is derived in reports). Any difference refuses the import with the
//!   numbers named.
//! - Imported vouchers are posted through the normal posting path
//!   (`post_voucher_in`): our voucher numbers, our hash chain from
//!   genesis, into a dedicated `IMP` journal; the source system's
//!   transaction id is kept in the description.
//! - The file's opening balances become an `Åpningsbalanse` voucher
//!   (dated the day before history starts) and must sum to zero — a
//!   partial chart is refused, not papered over.
//! - Reskontro: an account is flagged (kunde/leverandør) only when
//!   *every* line on it carries the matching party; mixed accounts are
//!   imported without party links, with a warning. Non-digit party ids
//!   are renumbered (from 90000) with the mapping reported.

use anyhow::{Context, Result, bail, ensure};
use chrono::Days;
use regnmed_core::Ore;
use regnmed_core::saft_import::SaftFile;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::ledger::post_voucher_in;

#[derive(Debug, Default)]
pub struct ImportReport {
    pub accounts: usize,
    pub customers: usize,
    pub suppliers: usize,
    pub vouchers: usize,
    pub opening_posted: bool,
    /// Set on a follow-up import: the file's opening balances were
    /// checked against the already-imported history instead of being
    /// posted as a new Åpningsbalanse.
    pub opening_reconciled: bool,
    pub warnings: Vec<String>,
}

pub async fn import_saft(
    pool: &PgPool,
    company_id: Uuid,
    file: &SaftFile,
    created_by: &str,
) -> Result<ImportReport> {
    let mut report = ImportReport::default();
    let mut tx = pool.begin().await?;

    // Migration into a virgin ledger, or on top of prior imports only:
    // one file per fiscal year is a real export shape (Conta), so the
    // door stays open exactly as long as the ledger contains nothing but
    // IMP-journal vouchers. One ordinary voucher closes it for good.
    let last_seq: i64 = sqlx::query_scalar("select last_seq from chain_head where company_id = $1")
        .bind(company_id)
        .fetch_optional(&mut *tx)
        .await?
        .context("company has no chain head")?;
    let follow_up = last_seq > 0;
    if follow_up {
        let ordinary: i64 = sqlx::query_scalar(
            "select count(*) from voucher v join journal j on j.id = v.journal_id
             where v.company_id = $1 and j.code <> 'IMP'",
        )
        .bind(company_id)
        .fetch_one(&mut *tx)
        .await?;
        ensure!(
            ordinary == 0,
            "hovedboken inneholder {ordinary} bilag utenfor importjournalen — \
             SAF-T-import er bare mulig i et tomt selskap eller oppå tidligere \
             SAF-T-importer, før den løpende bokføringen starter"
        );
    }

    // Accounts: 4-digit NS 4102 only (the mapping wizard, #18, handles the rest).
    let bad: Vec<&str> = file
        .accounts
        .iter()
        .map(|a| a.account_id.as_str())
        .filter(|id| id.len() != 4 || !id.chars().all(|c| c.is_ascii_digit()))
        .collect();
    ensure!(
        bad.is_empty(),
        "accounts are not 4-digit NS 4102 ({}...) — use the kontoplan mapping (issue #18)",
        bad.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
    );

    if follow_up {
        reconcile_openings(&mut tx, company_id, file).await?;
        report.opening_reconciled = true;
    }
    for account in &file.accounts {
        sqlx::query(
            "insert into account (id, company_id, number, name) values ($1, $2, $3, $4)
             on conflict (company_id, number) do nothing",
        )
        .bind(Uuid::now_v7())
        .bind(company_id)
        .bind(&account.account_id)
        .bind(if account.name.is_empty() {
            "Importert konto"
        } else {
            &account.name
        })
        .execute(&mut *tx)
        .await?;
        report.accounts += 1;
    }

    // Parties: keep digit ids, renumber the rest from 90000.
    let mut party_map: HashMap<(char, String), String> = HashMap::new();
    let mut next_free = 90_000i64;
    for (tag, parties, kind) in [
        ('C', &file.customers, "kunde"),
        ('S', &file.suppliers, "leverandor"),
    ] {
        for party in parties.iter() {
            let party_no = if !party.source_id.is_empty()
                && party.source_id.chars().all(|c| c.is_ascii_digit())
            {
                party.source_id.clone()
            } else {
                next_free += 1;
                report.warnings.push(format!(
                    "part '{}' ({}) fikk nytt nummer {next_free}",
                    party.source_id, party.name
                ));
                next_free.to_string()
            };
            sqlx::query(
                "insert into party (id, company_id, party_no, kind, name, orgnr)
                 values ($1, $2, $3, $4, $5, $6)
                 on conflict (company_id, party_no) do nothing",
            )
            .bind(Uuid::now_v7())
            .bind(company_id)
            .bind(&party_no)
            .bind(kind)
            .bind(if party.name.is_empty() {
                "Importert part"
            } else {
                &party.name
            })
            .bind(party.orgnr.as_deref().filter(|o| o.len() == 9))
            .execute(&mut *tx)
            .await?;
            party_map.insert((tag, party.source_id.clone()), party_no);
            if tag == 'C' {
                report.customers += 1;
            } else {
                report.suppliers += 1;
            }
        }
    }

    // Reskontro analysis: flag an account only when every line on it
    // carries the matching party; mixed accounts lose their links.
    let mut with_party: HashMap<&str, (usize, usize, char)> = HashMap::new(); // (party, bare, tag)
    for transaction in &file.transactions {
        for line in &transaction.lines {
            let entry = with_party.entry(&line.account_id).or_insert((0, 0, 'C'));
            if line.customer_id.is_some() {
                entry.0 += 1;
                entry.2 = 'C';
            } else if line.supplier_id.is_some() {
                entry.0 += 1;
                entry.2 = 'S';
            } else {
                entry.1 += 1;
            }
        }
    }
    let mut flagged: HashMap<String, char> = HashMap::new();
    for (account, (partied, bare, tag)) in &with_party {
        if *partied > 0 && *bare == 0 {
            let kind = if *tag == 'C' { "kunde" } else { "leverandor" };
            sqlx::query(
                "update account set reskontro_kind = $3 where company_id = $1 and number = $2",
            )
            .bind(company_id)
            .bind(account)
            .bind(kind)
            .execute(&mut *tx)
            .await?;
            flagged.insert((*account).to_string(), *tag);
        } else if *partied > 0 {
            report.warnings.push(format!(
                "konto {account} har linjer både med og uten part — reskontro-kobling hoppet over"
            ));
        }
    }

    // Known VAT codes; unknown codes are dropped with one warning each.
    let known_codes: HashSet<String> = sqlx::query_scalar::<_, String>("select code from vat_code")
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();
    let mut dropped_codes: HashSet<String> = HashSet::new();

    let journal = "IMP";
    sqlx::query(
        "insert into journal (id, company_id, code, name) values ($1, $2, $3, 'Importert historikk')
         on conflict (company_id, code) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(journal)
    .execute(&mut *tx)
    .await?;

    // Opening balance voucher, dated the day before history starts. On a
    // follow-up import the balances are already in the ledger — the
    // file's openings were reconciled above, never posted again.
    let history_start = file
        .selection_start
        .or_else(|| file.transactions.iter().map(|t| t.date).min());
    let opening: Vec<&regnmed_core::saft_import::ImportAccount> = file
        .accounts
        .iter()
        .filter(|a| a.opening_ore != 0)
        .collect();
    if !follow_up && !opening.is_empty() {
        let start = history_start.context("file has opening balances but no dates")?;
        let sum: i64 = opening.iter().map(|a| a.opening_ore).sum();
        ensure!(
            sum == 0,
            "opening balances sum to {} øre, not zero — the export looks \
             partial; import the complete SAF-T file",
            sum
        );
        let date = start.checked_sub_days(Days::new(1)).context("date range")?;
        let draft = VoucherDraft {
            journal_code: journal.into(),
            voucher_date: date,
            description: "Åpningsbalanse fra SAF-T-import".into(),
            reverses: None,
            entries: opening
                .iter()
                .map(|a| EntryDraft {
                    account_number: a.account_id.clone(),
                    amount: Ore(a.opening_ore),
                    vat_code: None,
                    description: None,
                    party_no: None,
                    avdeling: None,
                    prosjekt: None,
                    valuta: None,
                })
                .collect(),
        };
        // Opening lines on reskontro-flagged accounts would demand a party;
        // keep those accounts unflagged-at-opening by posting first...
        // simpler: the flagging above only covers accounts seen in
        // transaction lines WITH parties on every line — opening lines on
        // such accounts do conflict, so un-flag any account the opening
        // touches and warn.
        for account in &opening {
            if flagged.remove(account.account_id.as_str()).is_some() {
                sqlx::query(
                    "update account set reskontro_kind = null where company_id = $1 and number = $2",
                )
                .bind(company_id)
                .bind(&account.account_id)
                .execute(&mut *tx)
                .await?;
                report.warnings.push(format!(
                    "konto {} har åpningsbalanse uten part — reskontro-flagg utsatt til etter migrering",
                    account.account_id
                ));
            }
        }
        post_voucher_in(&mut tx, company_id, &draft, created_by).await?;
        report.opening_posted = true;
    }

    // History, chain-posted in file order.
    for transaction in &file.transactions {
        ensure!(
            transaction.lines.len() >= 2,
            "transaksjon '{}' har færre enn to linjer",
            transaction.source_id
        );
        let description = if transaction.description.is_empty() {
            format!("Importert bilag [{}]", transaction.source_id)
        } else {
            format!("{} [{}]", transaction.description, transaction.source_id)
        };
        let draft = VoucherDraft {
            journal_code: journal.into(),
            voucher_date: transaction.date,
            description: crate_trunc(&description, 250),
            reverses: None,
            entries: transaction
                .lines
                .iter()
                .map(|line| {
                    let party_no = match flagged.get(line.account_id.as_str()) {
                        Some('C') => line
                            .customer_id
                            .as_ref()
                            .and_then(|id| party_map.get(&('C', id.clone())).cloned()),
                        Some('S') => line
                            .supplier_id
                            .as_ref()
                            .and_then(|id| party_map.get(&('S', id.clone())).cloned()),
                        _ => None,
                    };
                    let vat_code = line.tax_code.clone().filter(|code| {
                        let known = known_codes.contains(code);
                        if !known && dropped_codes.insert(code.clone()) {
                            report
                                .warnings
                                .push(format!("ukjent mva-kode '{code}' droppet under import"));
                        }
                        known
                    });
                    EntryDraft {
                        account_number: line.account_id.clone(),
                        amount: Ore(line.amount_ore),
                        vat_code,
                        description: line.description.clone().filter(|d| !d.is_empty()),
                        party_no,
                        avdeling: None,
                        prosjekt: None,
                        valuta: None,
                    }
                })
                .collect(),
        };
        post_voucher_in(&mut tx, company_id, &draft, created_by)
            .await
            .with_context(|| format!("transaksjon '{}'", transaction.source_id))?;
        report.vouchers += 1;
    }

    if report.accounts == 0 && report.vouchers == 0 {
        bail!("the file contained nothing to import");
    }
    tx.commit().await?;
    Ok(report)
}

fn crate_trunc(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// A follow-up file must continue exactly where the imported history
/// stopped, konto for konto:
///
/// - **Balance accounts (class 1–2)** open at the booked all-time
///   balance — they carry over the year end unchanged.
/// - **Resultat accounts (class 3–9)** are reset at year end by the
///   exporting system, but regnmed never posts year-end closings
///   (udisponert resultat is derived in reports), so their file opening
///   is compared against the booked entries of the file's own fiscal
///   year: zero at a year boundary, the year-to-date sum when a year is
///   delivered in several files.
///
/// Verified against a real Conta export (2026-08-06): balance accounts
/// carry over to the øre, resultat accounts open at zero, and the
/// openings therefore do NOT sum to zero — the prior year's result has
/// no counterpart in the file. That is why the first-import zero-sum
/// rule cannot apply here.
async fn reconcile_openings(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    file: &SaftFile,
) -> Result<()> {
    let start = file
        .selection_start
        .or_else(|| file.transactions.iter().map(|t| t.date).min())
        .context("oppfølgingsfilen har verken periodestart eller daterte transaksjoner")?;
    let (year_start, _) = regnmed_core::regnskapsar::regnskapsar_periode(
        regnmed_core::regnskapsar::regnskapsar(start),
    )
    .context("regnskapsår utenfor gyldig datoområde")?;

    // (all-time balance, balance within the file's fiscal year) per account.
    let booked: HashMap<String, (i64, i64)> = sqlx::query_as::<_, (String, i64, i64)>(
        "select a.number,
                coalesce(sum(e.amount_ore), 0)::bigint,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date >= $2), 0)::bigint
         from entry e
         join account a on a.id = e.account_id
         join voucher v on v.id = e.voucher_id
         where a.company_id = $1
         group by a.number",
    )
    .bind(company_id)
    .bind(year_start)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|(number, total, in_year)| (number, (total, in_year)))
    .collect();
    let file_opening: HashMap<&str, i64> = file
        .accounts
        .iter()
        .filter(|a| a.opening_ore != 0)
        .map(|a| (a.account_id.as_str(), a.opening_ore))
        .collect();

    let mut numbers: Vec<&str> = booked
        .keys()
        .map(String::as_str)
        .chain(file_opening.keys().copied())
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    let mut mismatches: Vec<String> = Vec::new();
    for number in numbers {
        let (total, in_year) = booked.get(number).copied().unwrap_or((0, 0));
        let expected = if number.starts_with('1') || number.starts_with('2') {
            total
        } else {
            in_year
        };
        let opening = file_opening.get(number).copied().unwrap_or(0);
        if opening != expected {
            mismatches.push(format!(
                "konto {number}: filens åpningsbalanse {opening} øre, bokført {expected} øre \
                 (avvik {} øre)",
                opening - expected
            ));
        }
    }
    let shown = mismatches.iter().take(10).cloned().collect::<Vec<_>>();
    let rest = mismatches.len().saturating_sub(shown.len());
    ensure!(
        mismatches.is_empty(),
        "filens åpningsbalanser stemmer ikke med den importerte historikken — \
         importer årsfilene i rekkefølge, eldste først: {}{}",
        shown.join("; "),
        if rest > 0 {
            format!("; … og {rest} kontoer til")
        } else {
            String::new()
        }
    );
    Ok(())
}
