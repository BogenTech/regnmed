//! Betalingsliste og remittering (docs/betaling.md, #33).
//!
//! The list is built from OPEN leverandør-poster (reskontro remainders
//! — computed, never stored), creditor data is snapshotted onto the
//! run items at creation, approval renders and STORES the pain.001
//! file (hash-checked on every download, reproducible forever), and
//! settlement posts the utbetalingsbilag and closes every item's
//! reskontro-rest in ONE transaction — the bank import then matches
//! the debit against that voucher like any other.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::hash::{sha256, truncate_to_micros};
use regnmed_core::pain001::{Betaling, Pain001Input, gyldig_kontonummer, normaliser_kontonummer};
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::post_voucher_in;

#[derive(Debug)]
pub struct PayableItem {
    pub entry_id: Uuid,
    pub voucher_label: String,
    pub date: NaiveDate,
    pub description: Option<String>,
    pub party_no: String,
    pub party_name: String,
    pub bank_account: Option<String>,
    /// What we owe on this post (positive).
    pub belop_ore: i64,
    /// Already part of an utkast/godkjent run.
    pub i_kjoring: bool,
}

/// Open leverandør-poster: credit remainders on leverandør-reskontro
/// accounts — what the company owes, post for post.
pub async fn payable_items(pool: &PgPool, company_id: Uuid) -> Result<Vec<PayableItem>> {
    let rows = sqlx::query(
        "select e.id, v.fiscal_year, v.voucher_number, v.voucher_date,
                coalesce(e.description, v.description) as description,
                p.party_no, p.name as party_name, p.bank_account,
                e.amount_ore
                - coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_a = e.id), 0)::bigint
                + coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_b = e.id), 0)::bigint as remaining_ore,
                exists (select 1 from payment_run_item i
                        join payment_run r on r.id = i.run_id
                        where i.entry_id = e.id
                          and r.status in ('utkast', 'godkjent')) as i_kjoring
         from entry e
         join voucher v on v.id = e.voucher_id
         join party p on p.id = e.party_id
         where v.company_id = $1 and p.kind = 'leverandor'
         order by v.voucher_date, v.voucher_number",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .filter(|r| r.get::<i64, _>("remaining_ore") < 0)
        .map(|r| PayableItem {
            entry_id: r.get("id"),
            voucher_label: format!(
                "{}-{}",
                r.get::<i32, _>("fiscal_year"),
                r.get::<i64, _>("voucher_number")
            ),
            date: r.get("voucher_date"),
            description: r.get("description"),
            party_no: r.get("party_no"),
            party_name: r.get("party_name"),
            bank_account: r.get("bank_account"),
            belop_ore: -r.get::<i64, _>("remaining_ore"),
            i_kjoring: r.get("i_kjoring"),
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct PaymentItemDraft {
    pub entry_id: Uuid,
    /// None = the whole open remainder.
    pub belop_ore: Option<i64>,
    pub kid: Option<String>,
    pub melding: Option<String>,
}

/// Creates a betalingsliste (status utkast): every item validated
/// against its OPEN remainder, the creditor's name and kontonummer
/// snapshotted onto the row. Approving is a separate audited action.
pub async fn create_run(
    pool: &PgPool,
    company_id: Uuid,
    items: &[PaymentItemDraft],
    debitor_konto: Option<&str>,
    execution_date: NaiveDate,
    created_by: &str,
) -> Result<Uuid> {
    ensure!(!items.is_empty(), "betalingslisten er tom");
    let debitor = match debitor_konto {
        Some(konto) => normaliser_kontonummer(konto),
        None => {
            let konto: Option<String> =
                sqlx::query_scalar("select bank_account from company where id = $1")
                    .bind(company_id)
                    .fetch_one(pool)
                    .await?;
            konto
                .map(|k| normaliser_kontonummer(&k))
                .context("selskapet mangler kontonummer — sett det under Firmaopplysninger")?
        }
    };
    ensure!(
        gyldig_kontonummer(&debitor),
        "debitorkonto {debitor} er ikke et gyldig kontonummer"
    );

    let payable = payable_items(pool, company_id).await?;
    let mut tx = pool.begin().await?;
    let run_id = Uuid::now_v7();
    sqlx::query(
        "insert into payment_run (id, company_id, debitor_konto, execution_date, created_by)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(run_id)
    .bind(company_id)
    .bind(&debitor)
    .bind(execution_date)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    for (i, item) in items.iter().enumerate() {
        let open = payable
            .iter()
            .find(|p| p.entry_id == item.entry_id)
            .with_context(|| format!("linje {}: ingen åpen leverandørpost", i + 1))?;
        let belop = item.belop_ore.unwrap_or(open.belop_ore);
        ensure!(
            belop > 0 && belop <= open.belop_ore,
            "linje {}: beløpet må være 1..{} øre",
            i + 1,
            open.belop_ore
        );
        let konto = open
            .bank_account
            .as_deref()
            .map(normaliser_kontonummer)
            .filter(|k| gyldig_kontonummer(k))
            .with_context(|| {
                format!(
                    "linje {}: leverandør {} mangler gyldig kontonummer",
                    i + 1,
                    open.party_no
                )
            })?;
        if let Some(kid) = &item.kid {
            ensure!(
                regnmed_core::kid::is_valid(kid),
                "linje {}: ugyldig KID {kid}",
                i + 1
            );
        }
        sqlx::query(
            "insert into payment_run_item (id, run_id, entry_id, belop_ore,
                                           kreditor_navn, kreditor_konto, kid, melding)
             values ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(Uuid::now_v7())
        .bind(run_id)
        .bind(item.entry_id)
        .bind(belop)
        .bind(&open.party_name)
        .bind(&konto)
        .bind(&item.kid)
        .bind(
            item.melding
                .clone()
                .or_else(|| open.description.clone())
                .filter(|_| item.kid.is_none()),
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(run_id)
}

/// Approves the list for export: renders the pain.001 file, stores it
/// with its SHA-256 and flips utkast → godkjent, in one transaction.
/// The approver is recorded separately from the creator (four-eyes
/// friendly; enforcement is #47 attestering).
pub async fn approve_run(
    pool: &PgPool,
    company_id: Uuid,
    run_id: Uuid,
    approved_by: &str,
) -> Result<[u8; 32]> {
    let mut tx = pool.begin().await?;
    let run = sqlx::query(
        "select r.status, r.debitor_konto, r.execution_date, c.name as company_name
         from payment_run r join company c on c.id = r.company_id
         where r.id = $1 and r.company_id = $2 for update of r",
    )
    .bind(run_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such payment run")?;
    let status: String = run.get("status");
    ensure!(status == "utkast", "kjøringen er {status}");

    let items = run_items(&mut tx, run_id).await?;
    ensure!(!items.is_empty(), "kjøringen har ingen linjer");
    let approved_at = truncate_to_micros(chrono::Utc::now());
    let input = Pain001Input {
        msg_id: format!("regnmed-{run_id}"),
        created: approved_at,
        avsender_navn: run.get("company_name"),
        debitor_konto: run.get("debitor_konto"),
        execution_date: run.get("execution_date"),
        betalinger: items,
    };
    let xml = regnmed_core::pain001::render(&input);
    let digest = sha256(xml.as_bytes());
    sqlx::query(
        "update payment_run set status = 'godkjent', approved_by = $3, approved_at = $4,
                                file = $5, file_sha256 = $6
         where id = $1 and company_id = $2",
    )
    .bind(run_id)
    .bind(company_id)
    .bind(approved_by)
    .bind(approved_at)
    .bind(xml.as_bytes())
    .bind(digest.as_slice())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(digest)
}

async fn run_items(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: Uuid,
) -> Result<Vec<Betaling>> {
    let rows = sqlx::query(
        "select id, belop_ore, kreditor_navn, kreditor_konto, kid, melding
         from payment_run_item where run_id = $1 order by created_at, id",
    )
    .bind(run_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Betaling {
            end_to_end_id: format!("regnmed-{}", r.get::<Uuid, _>("id")),
            belop_ore: r.get("belop_ore"),
            kreditor_navn: r.get("kreditor_navn"),
            kreditor_konto: r.get("kreditor_konto"),
            kid: r.get("kid"),
            melding: r.get("melding"),
        })
        .collect())
}

/// The stored pain.001 file, integrity-checked on every download.
pub async fn run_file(pool: &PgPool, company_id: Uuid, run_id: Uuid) -> Result<(String, Vec<u8>)> {
    let row = sqlx::query(
        "select file, file_sha256 from payment_run
         where id = $1 and company_id = $2 and file is not null",
    )
    .bind(run_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("ingen fil — kjøringen er ikke godkjent")?;
    let file: Vec<u8> = row.get("file");
    let stored: Vec<u8> = row.get("file_sha256");
    ensure!(
        sha256(&file).to_vec() == stored,
        "payment file fails integrity check"
    );
    Ok((format!("pain001-{run_id}.xml"), file))
}

#[derive(Debug)]
pub struct SettledRun {
    pub voucher_number: i64,
    pub fiscal_year: i32,
}

/// Records the kjøring as executed: ONE utbetalingsbilag (debet each
/// leverandør-post's account with its party, kredit the ledger bank
/// account) and a reskontro match per item — everything in one
/// transaction, so the open posts close exactly when the payment is
/// booked. The bank import's debit then matches this voucher through
/// the ordinary engine, closing the circle.
pub async fn settle_run(
    pool: &PgPool,
    company_id: Uuid,
    run_id: Uuid,
    dato: NaiveDate,
    bank_konto: &str,
    settled_by: &str,
) -> Result<SettledRun> {
    let mut tx = pool.begin().await?;
    let status: String = sqlx::query_scalar(
        "select status from payment_run where id = $1 and company_id = $2 for update",
    )
    .bind(run_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such payment run")?;
    ensure!(
        status == "godkjent",
        "bare godkjente kjøringer kan registreres utbetalt (kjøringen er {status})"
    );

    let items = sqlx::query(
        "select i.id, i.entry_id, i.belop_ore, a.number as account_number, p.party_no
         from payment_run_item i
         join entry e on e.id = i.entry_id
         join account a on a.id = e.account_id
         join party p on p.id = e.party_id
         where i.run_id = $1
         order by i.created_at, i.id",
    )
    .bind(run_id)
    .fetch_all(&mut *tx)
    .await?;
    ensure!(!items.is_empty(), "kjøringen har ingen linjer");

    let total: i64 = items.iter().map(|r| r.get::<i64, _>("belop_ore")).sum();
    let mut entries: Vec<EntryDraft> = items
        .iter()
        .map(|r| EntryDraft {
            account_number: r.get("account_number"),
            amount: Ore(r.get::<i64, _>("belop_ore")),
            vat_code: None,
            description: Some("Remittering".into()),
            party_no: Some(r.get("party_no")),
            avdeling: None,
            prosjekt: None,
            valuta: None,
        })
        .collect();
    entries.push(EntryDraft {
        account_number: bank_konto.to_string(),
        amount: Ore(-total),
        vat_code: None,
        description: Some(format!("Betalingskjøring {run_id}")),
        party_no: None,
        avdeling: None,
        prosjekt: None,
        valuta: None,
    });
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: dato,
        description: format!("Utbetaling betalingsliste {run_id}"),
        reverses: None,
        entries,
    };
    draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
    let posted = post_voucher_in(&mut tx, company_id, &draft, settled_by).await?;

    // Match each debit line against its original open post.
    for (line_no, item) in items.iter().enumerate() {
        let debit_entry: Uuid =
            sqlx::query_scalar("select id from entry where voucher_id = $1 and line_no = $2")
                .bind(posted.id)
                .bind((line_no + 1) as i32)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query(
            "insert into reskontro_match (id, entry_a, entry_b, amount_ore, matched_by)
             values ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(debit_entry)
        .bind(item.get::<Uuid, _>("entry_id"))
        .bind(item.get::<i64, _>("belop_ore"))
        .bind(settled_by)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "update payment_run set status = 'utbetalt', settled_voucher_id = $3, settled_at = now()
         where id = $1 and company_id = $2",
    )
    .bind(run_id)
    .bind(company_id)
    .bind(posted.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(SettledRun {
        voucher_number: posted.voucher_number,
        fiscal_year: posted.fiscal_year,
    })
}

/// Cancels an utkast (one-way, audited). Approved runs are never
/// cancelled — the file exists; what happened in the bank is a
/// settlement question.
pub async fn cancel_run(
    pool: &PgPool,
    company_id: Uuid,
    run_id: Uuid,
    cancelled_by: &str,
) -> Result<()> {
    let updated = sqlx::query(
        "update payment_run set status = 'annullert', annullert_by = $3, annullert_at = now()
         where id = $1 and company_id = $2 and status = 'utkast'",
    )
    .bind(run_id)
    .bind(company_id)
    .bind(cancelled_by)
    .execute(pool)
    .await?;
    if updated.rows_affected() != 1 {
        bail!("kjøringen finnes ikke eller er ikke lenger utkast");
    }
    Ok(())
}

#[derive(Debug)]
pub struct PaymentRunRow {
    pub id: Uuid,
    pub status: String,
    pub execution_date: NaiveDate,
    pub sum_ore: i64,
    pub antall: i64,
    pub created_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub approved_by: Option<String>,
    pub settled_voucher: Option<String>,
}

pub async fn list_payment_runs(pool: &PgPool, company_id: Uuid) -> Result<Vec<PaymentRunRow>> {
    let rows = sqlx::query(
        "select r.id, r.status, r.execution_date, r.created_by, r.created_at, r.approved_by,
                v.fiscal_year, v.voucher_number,
                coalesce((select sum(i.belop_ore) from payment_run_item i
                          where i.run_id = r.id), 0)::bigint as sum_ore,
                (select count(*) from payment_run_item i where i.run_id = r.id) as antall
         from payment_run r
         left join voucher v on v.id = r.settled_voucher_id
         where r.company_id = $1
         order by r.created_at desc
         limit 100",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| PaymentRunRow {
            id: r.get("id"),
            status: r.get("status"),
            execution_date: r.get("execution_date"),
            sum_ore: r.get("sum_ore"),
            antall: r.get("antall"),
            created_by: r.get("created_by"),
            created_at: r.get("created_at"),
            approved_by: r.get("approved_by"),
            settled_voucher: match (
                r.get::<Option<i32>, _>("fiscal_year"),
                r.get::<Option<i64>, _>("voucher_number"),
            ) {
                (Some(year), Some(no)) => Some(format!("{year}-{no}")),
                _ => None,
            },
        })
        .collect())
}
