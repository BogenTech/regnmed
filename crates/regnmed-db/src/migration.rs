//! Migration import, file tier (docs/migration.md, #19) — what SAF-T does
//! not carry: contact details and open reskontro items.
//!
//! Contacts are plain register data and are imported idempotently (run
//! the file twice, get the same register). Open items are something else
//! entirely: they become BILAG, and so the hovedbok's rules apply.
//!
//! Open items REPLACE the summary line on the reskontro account — they
//! are not laid on top of it. That is why the import requires the account
//! to stand at zero before it runs, and says so plainly with the actual
//! balance when it does not. The migration order is therefore:
//!
//! 1. Contacts (so the items have something to point at).
//! 2. Opening balance WITHOUT the reskontro accounts (docs/migration.md).
//! 3. Open items — one party line per item against the contra account,
//!    ONE bilag, one transaction.
//!
//! Afterwards the reskontro balance equals the sum of the open items
//! because they are the same rows — not because something was reconciled
//! after the fact.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::migration_csv::{ContactRow, OpenItemRow, PartyKind};
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use uuid::Uuid;

use crate::ledger::{PostedVoucher, post_voucher_in};

#[derive(Debug, Default)]
pub struct ContactsReport {
    pub created: usize,
    pub oppdatert: usize,
    pub warnings: Vec<String>,
}

/// Creates or updates parties from an imported contact list. Matching,
/// in order: orgnr, then the old system's number when it is numeric
/// (kundenr 10001 stays 10001 — continuity the accountant can see),
/// then the name. Contact details that fail validation are warned
/// about and skipped; ONE bad kontonummer must not stop the file.
pub async fn import_contacts(
    pool: &PgPool,
    company_id: Uuid,
    rader: &[ContactRow],
) -> Result<ContactsReport> {
    let mut report = ContactsReport::default();
    let mut indexes: HashMap<&str, PartyIndex> = HashMap::new();
    for rad in rader {
        let kind = rad.kind.as_str();
        if !indexes.contains_key(kind) {
            indexes.insert(kind, PartyIndex::load(pool, company_id, kind).await?);
        }
        let index = indexes.get_mut(kind).expect("just inserted");
        let existing = index.find(rad.number.as_deref(), &rad.name, rad.orgnumber.as_deref());
        let party_id = match existing {
            Some((id, _)) => {
                report.oppdatert += 1;
                id
            }
            None => {
                // A numeric vendor number becomes the party_no; anything
                // else (S-1, L-9) gets our own numbering, and the name
                // stays the link back.
                let party_no = rad
                    .number
                    .as_deref()
                    .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
                let created = crate::subledger::create_party(
                    pool,
                    company_id,
                    kind,
                    &rad.name,
                    rad.orgnumber.as_deref(),
                    party_no,
                )
                .await;
                match created {
                    Ok((id, party_no)) => {
                        report.created += 1;
                        index.add(id, &party_no, rad.orgnumber.as_deref(), &rad.name);
                        id
                    }
                    Err(e) => {
                        report
                            .warnings
                            .push(format!("{}: kunne ikke opprettes ({e})", rad.name));
                        continue;
                    }
                }
            }
        };
        if rad.address.is_none() && rad.email.is_none() && rad.bank_account.is_none() {
            continue;
        }
        let full = crate::settings::update_party_contact(
            pool,
            company_id,
            party_id,
            rad.address.as_deref(),
            rad.email.as_deref(),
            rad.bank_account.as_deref(),
        )
        .await;
        if let Err(e) = full {
            report.warnings.push(format!(
                "{}: kontaktinfo delvis hoppet over ({e})",
                rad.name
            ));
            // Retry without the fields most likely to be at fault, so a
            // bad kontonummer does not also lose the e-mail address.
            let _ = crate::settings::update_party_contact(
                pool,
                company_id,
                party_id,
                rad.address.as_deref(),
                None,
                None,
            )
            .await;
        }
    }
    Ok(report)
}

/// A company's parties of one kind, read once per file. Each row of an
/// import used to run its own lookup, and `lower(name) = ...` made that a
/// scan of every party, so a file of N rows against P parties cost N x P
/// (5 000 open items against 2 000 customers: 12.8 s for a preview that
/// writes nothing, 2026-11-09). The match order is unchanged: orgnr, then
/// a numeric vendor number as party_no, then the name ignoring case.
struct PartyIndex {
    by_orgnumber: HashMap<String, (Uuid, String)>,
    by_number: HashMap<String, (Uuid, String)>,
    by_name: HashMap<String, (Uuid, String)>,
    /// Highest numeric party_no, for numbering parties created mid-file.
    max_number: Option<i64>,
}

impl PartyIndex {
    async fn load<'e, E: sqlx::PgExecutor<'e>>(
        executor: E,
        company_id: Uuid,
        kind: &str,
    ) -> Result<Self> {
        let rows = sqlx::query(
            "select id, party_no, orgnr, name from party
             where company_id = $1 and kind = $2 order by party_no",
        )
        .bind(company_id)
        .bind(kind)
        .fetch_all(executor)
        .await?;
        let mut index = PartyIndex {
            by_orgnumber: HashMap::new(),
            by_number: HashMap::new(),
            by_name: HashMap::new(),
            max_number: None,
        };
        for r in rows {
            let orgnumber: Option<String> = r.get("orgnr");
            index.add(
                r.get("id"),
                r.get::<&str, _>("party_no"),
                orgnumber.as_deref(),
                r.get("name"),
            );
        }
        Ok(index)
    }

    fn add(&mut self, id: Uuid, party_no: &str, orgnumber: Option<&str>, name: &str) {
        let value = (id, party_no.to_string());
        if let Some(o) = orgnumber.filter(|o| !o.is_empty()) {
            self.by_orgnumber
                .entry(o.to_string())
                .or_insert(value.clone());
        }
        self.by_number
            .entry(party_no.to_string())
            .or_insert(value.clone());
        self.by_name.entry(name.to_lowercase()).or_insert(value);
        if let Ok(n) = party_no.parse::<i64>() {
            self.max_number = Some(self.max_number.map_or(n, |m| m.max(n)));
        }
    }

    /// The party a row refers to, as (id, party_no).
    fn find(
        &self,
        number: Option<&str>,
        name: &str,
        orgnumber: Option<&str>,
    ) -> Option<(Uuid, String)> {
        let numeric = number.filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        orgnumber
            .filter(|o| !o.is_empty())
            .and_then(|o| self.by_orgnumber.get(o))
            .or_else(|| numeric.and_then(|n| self.by_number.get(n)))
            .or_else(|| self.by_name.get(&name.to_lowercase()))
            .cloned()
    }
}

#[derive(Debug)]
pub struct OpenItemsPlan {
    pub count: usize,
    /// Signed as it will hit the reskontro account.
    pub sum_ore: i64,
    /// Parties the file references that do not exist yet — they will be
    /// created by the import, listed here so nobody is surprised.
    pub nye_parter: Vec<String>,
    /// The account's balance right now; must be zero to import.
    pub account_balance_ore: i64,
    pub warnings: Vec<String>,
}

fn line_amount(kind: PartyKind, amount_ore: i64) -> i64 {
    match kind {
        // A customer owing us is a debit; a supplier we owe is a credit.
        // Credit notes keep their own sign through the negation.
        PartyKind::Customer => amount_ore,
        PartyKind::Supplier => -amount_ore,
    }
}

/// What the import would do — the preview the portal shows before
/// anyone commits. Reads only.
pub async fn plan_open_items(
    pool: &PgPool,
    company_id: Uuid,
    kind: PartyKind,
    account: &str,
    rader: &[OpenItemRow],
) -> Result<OpenItemsPlan> {
    let balance: Option<i64> = sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company_id)
    .bind(account)
    .fetch_optional(pool)
    .await?;
    let account_balance_ore = balance.context("no such account")?;

    let index = PartyIndex::load(pool, company_id, kind.as_str()).await?;
    let mut nye_parter = Vec::new();
    let mut warnings = Vec::new();
    for rad in rader {
        let found = index.find(
            Some(rad.party.as_str()),
            rad.party_name.as_deref().unwrap_or(&rad.party),
            None,
        );
        if found.is_none() {
            let name = rad.party_name.clone().unwrap_or_else(|| rad.party.clone());
            if !nye_parter.contains(&name) {
                nye_parter.push(name);
            }
        }
    }
    if !nye_parter.is_empty() {
        warnings.push(format!(
            "{} part(er) finnes ikke og opprettes ved import — importer kontaktlisten først \
             hvis du vil ha adresser og kontonumre med",
            nye_parter.len()
        ));
    }
    Ok(OpenItemsPlan {
        count: rader.len(),
        sum_ore: rader.iter().map(|r| line_amount(kind, r.amount_ore)).sum(),
        nye_parter,
        account_balance_ore,
        warnings,
    })
}

#[derive(Debug)]
pub struct OpenItemsReport {
    pub posted: PostedVoucher,
    pub count: usize,
    pub sum_ore: i64,
    pub opprettede_parter: usize,
    pub warnings: Vec<String>,
}

/// Posts the open items as ONE voucher: one party-carrying line per
/// item on the reskontro account, balanced against `motkonto` — all in
/// one transaction, so a file either lands whole or not at all.
pub async fn import_open_items(
    pool: &PgPool,
    company_id: Uuid,
    kind: PartyKind,
    account: &str,
    contra_account: &str,
    date: NaiveDate,
    rader: &[OpenItemRow],
    created_by: &str,
) -> Result<OpenItemsReport> {
    ensure!(!rader.is_empty(), "filen inneholder ingen åpne poster");
    ensure!(
        account != contra_account,
        "konto og motkonto kan ikke være den samme"
    );

    let mut tx = pool.begin().await?;
    let balance: Option<i64> = sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company_id)
    .bind(account)
    .fetch_optional(&mut *tx)
    .await?;
    let balance = balance.context("no such account")?;
    if balance != 0 {
        bail!(
            "konto {account} har allerede saldo {} øre — åpne poster ERSTATTER samlelinjen, \
             de legges ikke oppå den. Utelat kontoen fra åpningsbalansen (eller reverser \
             samlelinjen) og kjør importen på nytt",
            balance
        );
    }

    // The account must carry the reskontro flag for party lines to be
    // accepted — the opening balance defers the flag on purpose
    // (crate::opening), and this is where it comes back.
    let updated =
        sqlx::query("update account set reskontro_kind = $3 where company_id = $1 and number = $2")
            .bind(company_id)
            .bind(account)
            .bind(kind.as_str())
            .execute(&mut *tx)
            .await?
            .rows_affected();
    ensure!(updated == 1, "no account {account}");

    let mut warnings = Vec::new();
    let mut opprettede_parter = 0usize;
    let mut entries = Vec::with_capacity(rader.len() + 1);
    let mut index = PartyIndex::load(&mut *tx, company_id, kind.as_str()).await?;
    for rad in rader {
        let name = rad.party_name.clone().unwrap_or_else(|| rad.party.clone());
        let party_no = match index.find(Some(rad.party.as_str()), &name, None) {
            Some((_, no)) => no,
            None => {
                let (id, no) =
                    create_party_in(&mut tx, company_id, kind, &name, &rad.party, &index).await?;
                index.add(id, &no, None, &name);
                opprettede_parter += 1;
                no
            }
        };
        let mut text = rad.document.clone().unwrap_or_else(|| "Åpen post".into());
        if let Some(kid) = &rad.kid {
            text = format!("{text} (KID {kid})");
        }
        if let Some(due_date) = rad.due_date {
            text = format!("{text} forfall {due_date}");
        }
        entries.push(EntryDraft {
            account_number: account.to_string(),
            amount: Ore(line_amount(kind, rad.amount_ore)),
            vat_code: None,
            description: Some(text),
            party_no: Some(party_no),
            department: None,
            project: None,
            currency: None,
        });
    }
    let sum_ore: i64 = entries.iter().map(|e| e.amount.0).sum();
    entries.push(EntryDraft {
        account_number: contra_account.to_string(),
        amount: Ore(-sum_ore),
        vat_code: None,
        description: Some("Motpost åpne poster ved migrering".into()),
        party_no: None,
        department: None,
        project: None,
        currency: None,
    });

    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: format!(
            "Åpne poster {} ved migrering",
            match kind {
                PartyKind::Customer => "kunder",
                PartyKind::Supplier => "leverandører",
            }
        ),
        reverses: None,
        entries,
    };
    draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
    let posted = post_voucher_in(&mut tx, company_id, &draft, created_by).await?;

    // A supplier's due date and KID are what the payment list pays by
    // (docs/betaling.md): kept as payable terms on each imported item, not
    // only in the text, so «Velg forfalte» and the overdue figures see
    // them. The party lines are posted in file order, one per row.
    if kind == PartyKind::Supplier {
        let lines: Vec<Uuid> = sqlx::query_scalar(
            "select id from entry where voucher_id = $1 and party_id is not null order by line_no",
        )
        .bind(posted.id)
        .fetch_all(&mut *tx)
        .await?;
        ensure!(
            lines.len() == rader.len(),
            "importen ga ikke én linje per post"
        );
        for (entry_id, rad) in lines.into_iter().zip(rader) {
            let kid = match rad.kid.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
                Some(k) if regnmed_core::kid::is_valid(k) => Some(k),
                Some(k) => {
                    warnings.push(format!(
                        "KID {k} på {} er ugyldig og ble ikke lagret (står i teksten)",
                        rad.document.as_deref().unwrap_or("en post")
                    ));
                    None
                }
                None => None,
            };
            if rad.due_date.is_some() || kid.is_some() {
                crate::payments::insert_terms(
                    &mut *tx,
                    company_id,
                    entry_id,
                    rad.due_date,
                    kid,
                    created_by,
                )
                .await?;
            }
        }
    }
    tx.commit().await?;

    if opprettede_parter > 0 {
        warnings.push(format!(
            "{opprettede_parter} part(er) ble opprettet fra postlisten uten kontaktopplysninger"
        ));
    }
    Ok(OpenItemsReport {
        posted,
        count: rader.len(),
        sum_ore,
        opprettede_parter,
        warnings,
    })
}

async fn create_party_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    kind: PartyKind,
    name: &str,
    vendor_ref: &str,
    index: &PartyIndex,
) -> Result<(Uuid, String)> {
    let numeric = (!vendor_ref.is_empty() && vendor_ref.chars().all(|c| c.is_ascii_digit()))
        .then(|| vendor_ref.to_string());
    let party_no = match numeric {
        Some(no) => no,
        None => {
            let first: i64 = match kind {
                PartyKind::Customer => 10_000,
                PartyKind::Supplier => 20_000,
            };
            index.max_number.map_or(first, |m| m + 1).to_string()
        }
    };
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into party (id, company_id, party_no, kind, name)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(company_id)
    .bind(&party_no)
    .bind(kind.as_str())
    .bind(name)
    .execute(&mut **tx)
    .await
    .context("creating party from open items")?;
    Ok((id, party_no))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The order the per-row SQL used: orgnr beats a numeric vendor
    /// number, which beats the name; names match ignoring case; a
    /// non-numeric vendor number is never read as a party_no.
    #[test]
    fn the_index_matches_orgnumber_then_number_then_name() {
        let (a, b, c) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let mut index = PartyIndex {
            by_orgnumber: HashMap::new(),
            by_number: HashMap::new(),
            by_name: HashMap::new(),
            max_number: None,
        };
        index.add(a, "10001", Some("915933149"), "Hansen AS");
        index.add(b, "10002", None, "Lille Bakeri");
        index.add(c, "S-9", None, "Øvre Ørsta");
        let id = |r: Option<(Uuid, String)>| r.map(|(id, _)| id);
        assert_eq!(
            id(index.find(Some("10002"), "Lille Bakeri", Some("915933149"))),
            Some(a)
        );
        assert_eq!(id(index.find(Some("10002"), "Hansen AS", None)), Some(b));
        assert_eq!(id(index.find(None, "LILLE BAKERI", None)), Some(b));
        assert_eq!(id(index.find(None, "øvre ørsta", None)), Some(c));
        assert_eq!(id(index.find(Some("S-9"), "Ukjent", None)), None);
        assert_eq!(index.max_number, Some(10002), "S-9 is not a number");
    }
}
