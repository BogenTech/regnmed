//! Kontoplan management and manual bilagsføring (docs/hovedbok.md).
//!
//! Accounts are master data: the NUMBER is what postings reference and
//! what the hash chain covers, so a number can never change — but names
//! are editable and accounts deactivate rather than delete. The standard
//! catalog (Skatteetaten's grouping list, vendored in regnmed-core) is
//! served alongside so the portal can offer every code a regnskapsfører
//! knows, without seeding 254 rows into every company: an account exists
//! in the company only once someone adds it or posts to it.

use anyhow::{Context, Result, bail, ensure};
use regnmed_core::voucher::VoucherDraft;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::{PostedVoucher, post_voucher_in};

pub struct KontoRow {
    pub number: String,
    pub name: String,
    pub vat_code: Option<String>,
    pub active: bool,
    pub reskontro_kind: Option<String>,
    /// Lifetime balance (SUM of entries, øre) — computed, never stored.
    pub saldo_ore: i64,
    pub posteringer: i64,
}

/// Every account the company has, with computed balances. Accounts with
/// zero postings are included — a freshly added account must be visible,
/// or "add account" looks like it did nothing.
pub async fn list_accounts(pool: &PgPool, company_id: Uuid) -> Result<Vec<KontoRow>> {
    let rows = sqlx::query(
        "select a.number, a.name, a.vat_code, a.active, a.reskontro_kind,
                coalesce(sum(e.amount_ore), 0)::bigint as saldo_ore,
                count(e.id)::bigint as posteringer
         from account a
         left join entry e on e.account_id = a.id
         where a.company_id = $1
         group by a.id
         order by a.number",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| KontoRow {
            number: r.get("number"),
            name: r.get("name"),
            vat_code: r.get("vat_code"),
            active: r.get("active"),
            reskontro_kind: r.get("reskontro_kind"),
            saldo_ore: r.get("saldo_ore"),
            posteringer: r.get("posteringer"),
        })
        .collect())
}

/// Add an account to the company's kontoplan. The name may be the
/// standard one (resolved by the caller) or the company's own — custom
/// accounts are first-class, they just need a name.
pub async fn create_account(
    pool: &PgPool,
    company_id: Uuid,
    number: &str,
    name: &str,
) -> Result<()> {
    ensure!(
        number.len() == 4 && number.chars().all(|c| c.is_ascii_digit()),
        "kontonummer må være fire sifre"
    );
    ensure!(!name.trim().is_empty(), "kontoen må ha et navn");
    let result = sqlx::query(
        "insert into account (id, company_id, number, name) values ($1, $2, $3, $4)
         on conflict (company_id, number) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(number)
    .bind(name.trim())
    .execute(pool)
    .await?;
    ensure!(
        result.rows_affected() == 1,
        "konto {number} finnes allerede i kontoplanen"
    );
    Ok(())
}

/// Rename and/or (de)activate. The number is permanent — it is what the
/// ledger's hash chain references. Deactivating stops NEW postings
/// (post_voucher requires an active account); history is untouched.
pub async fn update_account(
    pool: &PgPool,
    company_id: Uuid,
    number: &str,
    name: Option<&str>,
    active: Option<bool>,
) -> Result<()> {
    if let Some(n) = name {
        ensure!(!n.trim().is_empty(), "kontoen må ha et navn");
    }
    let result = sqlx::query(
        "update account set name = coalesce($3, name), active = coalesce($4, active)
         where company_id = $1 and number = $2",
    )
    .bind(company_id)
    .bind(number)
    .bind(name.map(str::trim))
    .bind(active)
    .execute(pool)
    .await?;
    ensure!(result.rows_affected() == 1, "ingen konto {number}");
    Ok(())
}

pub struct BilagLinje {
    pub account: String,
    pub account_name: String,
    pub amount_ore: i64,
    pub vat_code: Option<String>,
    pub description: Option<String>,
    pub party_no: Option<String>,
    pub avdeling: Option<String>,
    pub prosjekt: Option<String>,
}

pub struct BilagMedLinjer {
    pub voucher_id: Uuid,
    pub journal_code: String,
    pub fiscal_year: i32,
    pub voucher_number: i64,
    pub voucher_date: chrono::NaiveDate,
    pub description: String,
    pub lines: Vec<BilagLinje>,
}

pub struct BilagSide {
    pub total: i64,
    pub vouchers: Vec<BilagMedLinjer>,
}

/// The hovedbok browsing surface: vouchers newest-first with their
/// lines, paged and filtered SERVER-SIDE. This is deliberately not the
/// statutory bokføringsspesifikasjon — that report is complete by
/// definition; this is the screen you scroll. The filter reads the
/// whole bilag: number label, date, description, and every line's
/// account number/name.
pub async fn list_vouchers_paged(
    pool: &PgPool,
    company_id: Uuid,
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
    sok: Option<&str>,
    limit: i64,
    offset: i64,
    med_linjer: bool,
) -> Result<BilagSide> {
    let sok = sok.map(str::trim).filter(|s| !s.is_empty());
    let filter = "v.company_id = $1
         and ($2::date is null or v.voucher_date >= $2)
         and ($3::date is null or v.voucher_date <= $3)
         and ($4::text is null
              or v.description ilike '%' || $4 || '%'
              or v.fiscal_year::text || '-' || v.voucher_number::text like $4 || '%'
              or v.voucher_date::text like '%' || $4 || '%'
              or exists (select 1 from entry e join account a on a.id = e.account_id
                         where e.voucher_id = v.id
                           and (a.number like $4 || '%' or a.name ilike '%' || $4 || '%')))";

    let total: i64 = sqlx::query_scalar(&format!("select count(*) from voucher v where {filter}"))
        .bind(company_id)
        .bind(from)
        .bind(to)
        .bind(sok)
        .fetch_one(pool)
        .await?;

    let headers = sqlx::query(&format!(
        "select v.id, j.code as journal_code, v.fiscal_year, v.voucher_number,
                v.voucher_date, v.description
         from voucher v join journal j on j.id = v.journal_id
         where {filter}
         order by v.chain_seq desc limit $5 offset $6"
    ))
    .bind(company_id)
    .bind(from)
    .bind(to)
    .bind(sok)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let ids: Vec<Uuid> = headers.iter().map(|r| r.get("id")).collect();
    let mut vouchers: Vec<BilagMedLinjer> = headers
        .into_iter()
        .map(|r| BilagMedLinjer {
            voucher_id: r.get("id"),
            journal_code: r.get("journal_code"),
            fiscal_year: r.get("fiscal_year"),
            voucher_number: r.get("voucher_number"),
            voucher_date: r.get("voucher_date"),
            description: r.get("description"),
            lines: Vec::new(),
        })
        .collect();

    if !med_linjer {
        return Ok(BilagSide { total, vouchers });
    }

    let lines = sqlx::query(
        "select e.voucher_id, e.line_no, a.number, a.name, e.amount_ore, e.vat_code,
                e.description, p.party_no, da.code as avdeling, dp.code as prosjekt
         from entry e
         join account a on a.id = e.account_id
         left join party p on p.id = e.party_id
         left join dimension da on da.id = e.avdeling_id
         left join dimension dp on dp.id = e.prosjekt_id
         where e.voucher_id = any($1)
         order by e.line_no",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    for row in lines {
        let vid: Uuid = row.get("voucher_id");
        if let Some(v) = vouchers.iter_mut().find(|v| v.voucher_id == vid) {
            v.lines.push(BilagLinje {
                account: row.get("number"),
                account_name: row.get("name"),
                amount_ore: row.get("amount_ore"),
                vat_code: row.get("vat_code"),
                description: row.get("description"),
                party_no: row.get("party_no"),
                avdeling: row.get("avdeling"),
                prosjekt: row.get("prosjekt"),
            });
        }
    }

    Ok(BilagSide { total, vouchers })
}

/// Post a free-form manual voucher. Everything post_voucher already
/// enforces applies (period lock, active accounts, dims, reskontro,
/// double entry); what this adds is the attestering boundary: when the
/// company runs an active attestering policy and the voucher is at or
/// over the beløpsgrense, a manual posting would BYPASS the intern
/// kontroll (#47) — attestation targets inbox documents. Fail closed and
/// point at the innboks instead of quietly opening a side door.
pub async fn post_manual_voucher(
    pool: &PgPool,
    company_id: Uuid,
    draft: &VoucherDraft,
    created_by: &str,
) -> Result<PostedVoucher> {
    let mut tx = pool.begin().await?;
    if let Some(policy) = crate::attestering::current_policy(&mut *tx, company_id).await?
        && policy.aktiv
    {
        let debetsum: i64 = draft.entries.iter().map(|e| e.amount.0.max(0)).sum();
        if policy
            .belopsgrense_ore
            .is_none_or(|grense| debetsum >= grense)
        {
            bail!(
                "attesteringspolicyen krever attestering for bilag på dette beløpet — \
                 last dokumentet opp i innboksen og bokfør det derfra"
            );
        }
    }
    let posted = post_voucher_in(&mut tx, company_id, draft, created_by)
        .await
        .context("bokføringen feilet")?;
    tx.commit().await?;
    Ok(posted)
}
