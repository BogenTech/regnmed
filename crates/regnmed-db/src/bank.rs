//! Bank reconciliation persistence: statement import (idempotent on the
//! bank's statement id), auto-matching via the pure engine in
//! `regnmed_core::bank`, manual match/unmatch, and the reconciliation
//! status — where "unmatched" is always computed from the absence of a
//! match row, never stored state.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::bank::{BankTx, OpenEntry, propose_matches, suggest_entries};
use regnmed_core::camt053::Camt053Statement;
use regnmed_core::money::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Matching window: bank booking date vs voucher date.
const MATCH_WINDOW_DAYS: i64 = 5;

#[derive(Debug)]
pub struct ImportSummary {
    pub statement_id: Uuid,
    pub transactions: usize,
    pub auto_matched: usize,
}

/// Imports a parsed camt.053 statement for a company's bank account and
/// auto-matches its transactions against open ledger entries.
pub async fn import_statement(
    pool: &PgPool,
    company_id: Uuid,
    account_number: &str,
    statement: &Camt053Statement,
    imported_by: &str,
) -> Result<ImportSummary> {
    ensure!(
        !statement.statement_ref.is_empty(),
        "statement has no id (Stmt/Id)"
    );
    let account_id: Uuid =
        sqlx::query("select id from account where company_id = $1 and number = $2 and active")
            .bind(company_id)
            .bind(account_number)
            .fetch_optional(pool)
            .await?
            .with_context(|| format!("no active account {account_number} for this company"))?
            .get("id");

    let mut tx = pool.begin().await?;
    let statement_id = Uuid::now_v7();
    let inserted = sqlx::query(
        "insert into bank_statement (id, company_id, account_id, statement_ref, iban,
                                     from_date, to_date, opening_ore, closing_ore, imported_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         on conflict (company_id, statement_ref) do nothing",
    )
    .bind(statement_id)
    .bind(company_id)
    .bind(account_id)
    .bind(&statement.statement_ref)
    .bind(&statement.iban)
    .bind(statement.from_date)
    .bind(statement.to_date)
    .bind(statement.opening_ore)
    .bind(statement.closing_ore)
    .bind(imported_by)
    .execute(&mut *tx)
    .await?;
    ensure!(
        inserted.rows_affected() == 1,
        "statement {} is already imported",
        statement.statement_ref
    );

    for transaction in &statement.transactions {
        sqlx::query(
            "insert into bank_transaction (id, statement_id, booking_date, amount_ore,
                                           description, reference)
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(statement_id)
        .bind(transaction.booking_date)
        .bind(transaction.amount_ore)
        .bind(&transaction.description)
        .bind(&transaction.reference)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    let auto_matched = auto_match(pool, company_id, account_id, imported_by).await?;
    Ok(ImportSummary {
        statement_id,
        transactions: statement.transactions.len(),
        auto_matched,
    })
}

/// Runs the matching engine over everything currently unmatched on the
/// account and records the proposals as 'auto' matches.
async fn auto_match(
    pool: &PgPool,
    company_id: Uuid,
    account_id: Uuid,
    matched_by: &str,
) -> Result<usize> {
    let bank_txs: Vec<BankTx> = unmatched_bank_rows(pool, company_id, Some(account_id))
        .await?
        .into_iter()
        .map(|r| BankTx {
            id: r.id,
            booking_date: r.booking_date,
            amount_ore: r.amount_ore,
        })
        .collect();

    let entries: Vec<OpenEntry> = sqlx::query(
        "select e.id, v.voucher_date, e.amount_ore
         from entry e
         join voucher v on v.id = e.voucher_id
         where v.company_id = $1 and e.account_id = $2
           and not exists (select 1 from bank_match m where m.entry_id = e.id)",
    )
    .bind(company_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| OpenEntry {
        entry_id: r.get("id"),
        date: r.get("voucher_date"),
        amount_ore: r.get("amount_ore"),
    })
    .collect();

    let proposals = propose_matches(&bank_txs, &entries, MATCH_WINDOW_DAYS);
    for proposal in &proposals {
        sqlx::query(
            "insert into bank_match (bank_transaction_id, entry_id, method, matched_by)
             values ($1, $2, 'auto', $3)",
        )
        .bind(proposal.bank_tx_id)
        .bind(proposal.entry_id)
        .bind(matched_by)
        .execute(pool)
        .await?;
    }
    Ok(proposals.len())
}

/// Manual match, guarded: both sides must belong to the company (and the
/// entry to the same account as the bank transaction's statement), and
/// the amounts must be EQUAL — a 45 kr fee is never reconciled against a
/// 4 367 kr payment, however the dropdown was left. The unique
/// constraints reject double-matching.
pub async fn manual_match(
    pool: &PgPool,
    company_id: Uuid,
    bank_transaction_id: Uuid,
    entry_id: Uuid,
    matched_by: &str,
) -> Result<()> {
    let valid: bool = sqlx::query_scalar(
        "select exists (
             select 1
             from bank_transaction t
             join bank_statement s on s.id = t.statement_id
             join entry e on e.id = $3 and e.account_id = s.account_id
             join voucher v on v.id = e.voucher_id and v.company_id = s.company_id
             where t.id = $2 and s.company_id = $1
         )",
    )
    .bind(company_id)
    .bind(bank_transaction_id)
    .bind(entry_id)
    .fetch_one(pool)
    .await?;
    ensure!(
        valid,
        "bank transaction and entry must belong to the same company and bank account"
    );
    let (bank_ore, entry_ore): (i64, i64) = sqlx::query_as(
        "select t.amount_ore, e.amount_ore from bank_transaction t, entry e
         where t.id = $1 and e.id = $2",
    )
    .bind(bank_transaction_id)
    .bind(entry_id)
    .fetch_one(pool)
    .await?;
    ensure!(
        bank_ore == entry_ore,
        "beløpene er ulike (bank {}, hovedbok {}) — bare like beløp kan kobles",
        Ore(bank_ore),
        Ore(entry_ore)
    );

    sqlx::query(
        "insert into bank_match (bank_transaction_id, entry_id, method, matched_by)
         values ($1, $2, 'manual', $3)",
    )
    .bind(bank_transaction_id)
    .bind(entry_id)
    .bind(matched_by)
    .execute(pool)
    .await
    .context("already matched — unmatch first")?;
    Ok(())
}

pub async fn unmatch(pool: &PgPool, company_id: Uuid, bank_transaction_id: Uuid) -> Result<()> {
    let removed = sqlx::query(
        "delete from bank_match m
         using bank_transaction t, bank_statement s
         where m.bank_transaction_id = $2
           and t.id = m.bank_transaction_id
           and s.id = t.statement_id and s.company_id = $1",
    )
    .bind(company_id)
    .bind(bank_transaction_id)
    .execute(pool)
    .await?;
    ensure!(removed.rows_affected() == 1, "no such match");
    Ok(())
}

#[derive(Debug)]
pub struct UnmatchedBankRow {
    pub id: Uuid,
    pub booking_date: NaiveDate,
    pub amount_ore: i64,
    pub description: String,
    pub reference: Option<String>,
}

async fn unmatched_bank_rows(
    pool: &PgPool,
    company_id: Uuid,
    account_id: Option<Uuid>,
) -> Result<Vec<UnmatchedBankRow>> {
    let rows = sqlx::query(
        "select t.id, t.booking_date, t.amount_ore, t.description, t.reference
         from bank_transaction t
         join bank_statement s on s.id = t.statement_id
         where s.company_id = $1
           and ($2::uuid is null or s.account_id = $2)
           and not exists (select 1 from bank_match m where m.bank_transaction_id = t.id)
         order by t.booking_date, t.amount_ore",
    )
    .bind(company_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| UnmatchedBankRow {
            id: r.get("id"),
            booking_date: r.get("booking_date"),
            amount_ore: r.get("amount_ore"),
            description: r.get("description"),
            reference: r.get("reference"),
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct UnmatchedEntryRow {
    pub entry_id: Uuid,
    pub voucher_date: NaiveDate,
    pub voucher_label: String,
    pub amount_ore: i64,
    pub description: Option<String>,
}

#[derive(Debug)]
pub struct StatementRow {
    pub id: Uuid,
    pub statement_ref: String,
    pub from_date: Option<NaiveDate>,
    pub to_date: Option<NaiveDate>,
    pub closing_ore: Option<i64>,
    pub transactions: i64,
    pub imported_by: String,
    pub imported_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug)]
pub struct MatchedRow {
    pub bank_transaction_id: Uuid,
    pub booking_date: NaiveDate,
    pub amount_ore: i64,
    pub description: String,
    pub voucher_label: String,
    pub method: String,
}

#[derive(Debug)]
pub struct ReconciliationStatus {
    pub account_number: String,
    pub ledger_balance_ore: i64,
    /// The ledger on the latest statement's end date — the number the
    /// bank's closing balance is compared with. Today's balance would
    /// include everything posted after the statement.
    pub ledger_at_statement_ore: Option<i64>,
    pub statement_closing_ore: Option<i64>,
    pub statement_to_date: Option<NaiveDate>,
    pub matched_count: i64,
    pub unmatched_bank: Vec<UnmatchedBankRow>,
    /// Per unmatched bank transaction, in the same order: open entries
    /// with exactly its amount, nearest date first (at most three).
    pub suggestions: Vec<Vec<UnmatchedEntryRow>>,
    /// Open entries WITHIN the statements' period (a month either side):
    /// postings the bank has not shown yet. History before the first
    /// statement is not reconciliation work and is left out.
    pub unmatched_entries: Vec<UnmatchedEntryRow>,
    pub statements: Vec<StatementRow>,
    /// The latest matches, newest bank date first — where a wrong
    /// pairing is undone.
    pub recent_matches: Vec<MatchedRow>,
}

/// The company's bank accounts: active 19xx accounts in its chart, with
/// what the reconciliation page needs to pick one.
#[derive(Debug)]
pub struct BankAccountRow {
    pub number: String,
    pub name: String,
    pub balance_ore: i64,
    pub last_statement_to: Option<NaiveDate>,
    pub unmatched_bank: i64,
}

pub async fn bank_accounts(pool: &PgPool, company_id: Uuid) -> Result<Vec<BankAccountRow>> {
    let rows = sqlx::query(
        "select a.number, a.name,
                coalesce((select sum(e.amount_ore) from entry e where e.account_id = a.id), 0)::bigint as balance,
                (select max(s.to_date) from bank_statement s where s.account_id = a.id) as last_to,
                (select count(*) from bank_transaction t
                   join bank_statement s on s.id = t.statement_id
                  where s.account_id = a.id
                    and not exists (select 1 from bank_match m where m.bank_transaction_id = t.id)) as unmatched
         from account a
         where a.company_id = $1 and a.active and a.number like '19%'
         order by a.number",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| BankAccountRow {
            number: r.get("number"),
            name: r.get("name"),
            balance_ore: r.get("balance"),
            last_statement_to: r.get("last_to"),
            unmatched_bank: r.get("unmatched"),
        })
        .collect())
}

fn entry_row(r: &sqlx::postgres::PgRow) -> UnmatchedEntryRow {
    UnmatchedEntryRow {
        entry_id: r.get("id"),
        voucher_date: r.get("voucher_date"),
        voucher_label: format!(
            "{}-{}",
            r.get::<i32, _>("fiscal_year"),
            r.get::<i64, _>("voucher_number")
        ),
        amount_ore: r.get("amount_ore"),
        description: r.get("description"),
    }
}

async fn account_id_of(pool: &PgPool, company_id: Uuid, account_number: &str) -> Result<Uuid> {
    Ok(
        sqlx::query("select id from account where company_id = $1 and number = $2")
            .bind(company_id)
            .bind(account_number)
            .fetch_optional(pool)
            .await?
            .with_context(|| format!("kontoen {account_number} finnes ikke i kontoplanen"))?
            .get("id"),
    )
}

pub async fn reconciliation_status(
    pool: &PgPool,
    company_id: Uuid,
    account_number: &str,
) -> Result<ReconciliationStatus> {
    let account_id = account_id_of(pool, company_id, account_number).await?;

    let ledger_balance_ore: i64 = sqlx::query_scalar(
        "select coalesce(sum(amount_ore), 0)::bigint from entry where account_id = $1",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await?;

    let latest = sqlx::query(
        "select closing_ore, to_date from bank_statement
         where company_id = $1 and account_id = $2
         order by to_date desc nulls last limit 1",
    )
    .bind(company_id)
    .bind(account_id)
    .fetch_optional(pool)
    .await?;
    let statement_to_date: Option<NaiveDate> = latest.as_ref().and_then(|r| r.get("to_date"));
    let ledger_at_statement_ore: Option<i64> = match statement_to_date {
        Some(to) => Some(
            sqlx::query_scalar(
                "select coalesce(sum(e.amount_ore), 0)::bigint
                 from entry e join voucher v on v.id = e.voucher_id
                 where e.account_id = $1 and v.voucher_date <= $2",
            )
            .bind(account_id)
            .bind(to)
            .fetch_one(pool)
            .await?,
        ),
        None => None,
    };

    let matched_count: i64 = sqlx::query_scalar(
        "select count(*) from bank_match m
         join bank_transaction t on t.id = m.bank_transaction_id
         join bank_statement s on s.id = t.statement_id
         where s.company_id = $1 and s.account_id = $2",
    )
    .bind(company_id)
    .bind(account_id)
    .fetch_one(pool)
    .await?;

    let unmatched_bank = unmatched_bank_rows(pool, company_id, Some(account_id)).await?;

    // The period the statements cover, from their declared dates or, for
    // a CSV without them, from the transactions themselves.
    let (period_from, period_to): (Option<NaiveDate>, Option<NaiveDate>) = sqlx::query_as(
        "select least(min(s.from_date), min(t.booking_date)), greatest(max(s.to_date), max(t.booking_date))
         from bank_statement s left join bank_transaction t on t.statement_id = s.id
         where s.company_id = $1 and s.account_id = $2",
    )
    .bind(company_id)
    .bind(account_id)
    .fetch_one(pool)
    .await?;

    let unmatched_entries: Vec<UnmatchedEntryRow> = match (period_from, period_to) {
        (Some(from), Some(to)) => sqlx::query(
            "select e.id, v.voucher_date, v.fiscal_year, v.voucher_number,
                    e.amount_ore, coalesce(e.description, v.description) as description
             from entry e
             join voucher v on v.id = e.voucher_id
             where v.company_id = $1 and e.account_id = $2
               and v.voucher_date between $3::date - 31 and $4::date + 31
               and not exists (select 1 from bank_match m where m.entry_id = e.id)
             order by v.voucher_date, v.voucher_number",
        )
        .bind(company_id)
        .bind(account_id)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await?
        .iter()
        .map(entry_row)
        .collect(),
        _ => Vec::new(),
    };

    // Suggestions look at EVERY open entry with the right amount — a
    // posting dated far from the bank date is still the right one if it
    // is the only one — but only those amounts are fetched.
    let amounts: Vec<i64> = unmatched_bank.iter().map(|t| t.amount_ore).collect();
    let candidates: Vec<UnmatchedEntryRow> = sqlx::query(
        "select e.id, v.voucher_date, v.fiscal_year, v.voucher_number,
                e.amount_ore, coalesce(e.description, v.description) as description
         from entry e
         join voucher v on v.id = e.voucher_id
         where v.company_id = $1 and e.account_id = $2 and e.amount_ore = any($3)
           and not exists (select 1 from bank_match m where m.entry_id = e.id)",
    )
    .bind(company_id)
    .bind(account_id)
    .bind(&amounts)
    .fetch_all(pool)
    .await?
    .iter()
    .map(entry_row)
    .collect();
    let open: Vec<OpenEntry> = candidates
        .iter()
        .map(|c| OpenEntry {
            entry_id: c.entry_id,
            date: c.voucher_date,
            amount_ore: c.amount_ore,
        })
        .collect();
    let suggestions = unmatched_bank
        .iter()
        .map(|t| {
            let tx = BankTx {
                id: t.id,
                booking_date: t.booking_date,
                amount_ore: t.amount_ore,
            };
            suggest_entries(&tx, &open, 3)
                .into_iter()
                .filter_map(|o| {
                    candidates
                        .iter()
                        .find(|c| c.entry_id == o.entry_id)
                        .cloned()
                })
                .collect()
        })
        .collect();

    let statements = sqlx::query(
        "select s.id, s.statement_ref, s.from_date, s.to_date, s.closing_ore, s.imported_by,
                s.created_at, (select count(*) from bank_transaction t where t.statement_id = s.id) as n
         from bank_statement s
         where s.company_id = $1 and s.account_id = $2
         order by s.to_date desc nulls last, s.created_at desc",
    )
    .bind(company_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| StatementRow {
        id: r.get("id"),
        statement_ref: r.get("statement_ref"),
        from_date: r.get("from_date"),
        to_date: r.get("to_date"),
        closing_ore: r.get("closing_ore"),
        transactions: r.get("n"),
        imported_by: r.get("imported_by"),
        imported_at: r.get("created_at"),
    })
    .collect();

    let recent_matches = sqlx::query(
        "select t.id, t.booking_date, t.amount_ore, t.description, m.method,
                v.fiscal_year, v.voucher_number
         from bank_match m
         join bank_transaction t on t.id = m.bank_transaction_id
         join bank_statement s on s.id = t.statement_id
         join entry e on e.id = m.entry_id
         join voucher v on v.id = e.voucher_id
         where s.company_id = $1 and s.account_id = $2
         order by t.booking_date desc, t.id
         limit 50",
    )
    .bind(company_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| MatchedRow {
        bank_transaction_id: r.get("id"),
        booking_date: r.get("booking_date"),
        amount_ore: r.get("amount_ore"),
        description: r.get("description"),
        voucher_label: format!(
            "{}-{}",
            r.get::<i32, _>("fiscal_year"),
            r.get::<i64, _>("voucher_number")
        ),
        method: r.get("method"),
    })
    .collect();

    Ok(ReconciliationStatus {
        account_number: account_number.to_string(),
        ledger_balance_ore,
        ledger_at_statement_ore,
        // CSV statements carry no balances — absent, never zero.
        statement_closing_ore: latest.as_ref().and_then(|r| r.get("closing_ore")),
        statement_to_date,
        matched_count,
        unmatched_bank,
        suggestions,
        unmatched_entries,
        statements,
        recent_matches,
    })
}

/// Open entries on the bank account matching a free search — voucher
/// number ("2026-14"), text, or amount in kroner — for the bank line the
/// suggestions did not cover. At most 20, newest first.
pub async fn search_open_entries(
    pool: &PgPool,
    company_id: Uuid,
    account_number: &str,
    query: &str,
) -> Result<Vec<UnmatchedEntryRow>> {
    let account_id = account_id_of(pool, company_id, account_number).await?;
    let q = query.trim();
    let as_ore = regnmed_core::money::parse_kroner(q);
    let rows = sqlx::query(
        "select e.id, v.voucher_date, v.fiscal_year, v.voucher_number,
                e.amount_ore, coalesce(e.description, v.description) as description
         from entry e
         join voucher v on v.id = e.voucher_id
         where v.company_id = $1 and e.account_id = $2
           and not exists (select 1 from bank_match m where m.entry_id = e.id)
           and ($3 = ''
                or (v.fiscal_year || '-' || v.voucher_number) = $3
                or coalesce(e.description, v.description) ilike '%' || $3 || '%'
                or ($4::bigint is not null and abs(e.amount_ore) = abs($4)))
         order by v.voucher_date desc, v.voucher_number desc
         limit 20",
    )
    .bind(company_id)
    .bind(account_id)
    .bind(q)
    .bind(as_ore)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(entry_row).collect())
}

/// A bank line with no posting behind it — a fee, interest, a card
/// charge — posted straight from the statement and matched in the SAME
/// transaction: bank account against `counter_account`, dated the bank
/// booking date, the statement line as its documentation. No VAT: bank
/// services and interest are outside it; anything with VAT belongs in
/// the innboks with its receipt.
pub async fn post_from_bank_line(
    pool: &PgPool,
    company_id: Uuid,
    bank_transaction_id: Uuid,
    counter_account: &str,
    description: &str,
    created_by: &str,
) -> Result<crate::ledger::PostedVoucher> {
    let mut tx = pool.begin().await?;
    let line = sqlx::query(
        "select t.booking_date, t.amount_ore, t.description, s.statement_ref, a.number as bank_account
         from bank_transaction t
         join bank_statement s on s.id = t.statement_id
         join account a on a.id = s.account_id
         where t.id = $1 and s.company_id = $2
         for update of t",
    )
    .bind(bank_transaction_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such bank transaction")?;
    let already: bool = sqlx::query_scalar(
        "select exists (select 1 from bank_match where bank_transaction_id = $1)",
    )
    .bind(bank_transaction_id)
    .fetch_one(&mut *tx)
    .await?;
    ensure!(!already, "banklinjen er allerede koblet");
    let amount: i64 = line.get("amount_ore");
    let bank_account: String = line.get("bank_account");
    ensure!(
        counter_account != bank_account,
        "motkontoen kan ikke være bankkontoen selv"
    );
    let bank_text: String = line.get("description");
    let text = if description.trim().is_empty() {
        bank_text.clone()
    } else {
        description.trim().to_string()
    };
    let entry = |account: &str, ore: i64, note: Option<String>| EntryDraft {
        account_number: account.to_string(),
        amount: Ore(ore),
        vat_code: None,
        description: note,
        party_no: None,
        department: None,
        project: None,
        currency: None,
    };
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: line.get("booking_date"),
        description: if text.is_empty() {
            "Fra kontoutskrift".into()
        } else {
            text
        },
        reverses: None,
        entries: vec![
            entry(
                &bank_account,
                amount,
                Some(format!(
                    "Kontoutskrift {}: {bank_text}",
                    line.get::<String, _>("statement_ref")
                )),
            ),
            entry(counter_account, -amount, None),
        ],
    };
    crate::chart_of_accounts::refuse_if_attestation_required(&mut tx, company_id, &draft).await?;
    let posted = crate::ledger::post_voucher_in(&mut tx, company_id, &draft, created_by)
        .await
        .context("bokføringen feilet")?;
    let bank_entry: Uuid = sqlx::query_scalar(
        "select e.id from entry e join account a on a.id = e.account_id
         where e.voucher_id = $1 and a.number = $2",
    )
    .bind(posted.id)
    .bind(&bank_account)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "insert into bank_match (bank_transaction_id, entry_id, method, matched_by)
         values ($1, $2, 'manual', $3)",
    )
    .bind(bank_transaction_id)
    .bind(bank_entry)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(posted)
}
