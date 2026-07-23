//! Lovpålagte spesifikasjoner (bokføringsforskriften §3-1): saldobalanse,
//! kontospesifikasjon and bokføringsspesifikasjon, plus the saldo lines
//! that feed resultat/balanse in `regnmed-core::regnskap`.
//!
//! All of it is `SUM(amount_ore)` and ordered SELECTs over the immutable
//! ledger — never stored state, so the reports are correct the moment a
//! voucher is posted and reproducible for any historical period.

use anyhow::Result;
use chrono::NaiveDate;
use regnmed_core::regnskap::SaldoLine;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// One account over a period: inngående saldo, movement, utgående saldo.
#[derive(Debug)]
pub struct SaldobalanseRow {
    pub number: String,
    pub name: String,
    pub inngaende_ore: i64,
    pub debet_ore: i64,
    pub kredit_ore: i64,
    pub utgaende_ore: i64,
}

pub async fn saldobalanse(
    pool: &PgPool,
    company_id: Uuid,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<SaldobalanseRow>> {
    let rows = sqlx::query(
        "select a.number, a.name,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date < $2), 0)::bigint as inngaende,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date between $2 and $3
                                                     and e.amount_ore > 0), 0)::bigint as debet,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date between $2 and $3
                                                     and e.amount_ore < 0), 0)::bigint as kredit,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date <= $3), 0)::bigint as utgaende
         from account a
         join entry e on e.account_id = a.id
         join voucher v on v.id = e.voucher_id
         where a.company_id = $1 and v.voucher_date <= $3
         group by a.number, a.name
         having coalesce(sum(e.amount_ore) filter (where v.voucher_date <= $3), 0) <> 0
             or coalesce(sum(e.amount_ore) filter (where v.voucher_date between $2 and $3), 0) <> 0
             or coalesce(sum(e.amount_ore) filter (where v.voucher_date < $2), 0) <> 0
         order by a.number",
    )
    .bind(company_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| SaldobalanseRow {
            number: r.get("number"),
            name: r.get("name"),
            inngaende_ore: r.get("inngaende"),
            debet_ore: r.get("debet"),
            kredit_ore: r.get("kredit"),
            utgaende_ore: r.get("utgaende"),
        })
        .collect())
}

/// Saldo per account from day one through `to` (ledger sign) — the input
/// to resultat/balanse. For resultat, pass the period's `from` too.
pub async fn saldo_lines(
    pool: &PgPool,
    company_id: Uuid,
    from: Option<NaiveDate>,
    to: NaiveDate,
) -> Result<Vec<SaldoLine>> {
    let rows = sqlx::query(
        "select a.number, a.name, sum(e.amount_ore)::bigint as saldo
         from account a
         join entry e on e.account_id = a.id
         join voucher v on v.id = e.voucher_id
         where a.company_id = $1 and v.voucher_date <= $3
           and ($2::date is null or v.voucher_date >= $2)
         group by a.number, a.name
         order by a.number",
    )
    .bind(company_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| SaldoLine {
            number: r.get("number"),
            name: r.get("name"),
            saldo_ore: r.get("saldo"),
        })
        .collect())
}

/// One posting on one account, with the dokumentasjonshenvisning
/// (journal + bilagsnummer) the forskrift requires.
#[derive(Debug)]
pub struct KontoPost {
    pub number: String,
    pub account_name: String,
    pub journal_code: String,
    pub fiscal_year: i32,
    pub voucher_number: i64,
    pub voucher_date: NaiveDate,
    pub description: String,
    pub amount_ore: i64,
    /// Running saldo on the account, including this posting, from the
    /// period's inngående saldo.
    pub saldo_ore: i64,
    pub party_no: Option<String>,
}

/// Kontospesifikasjon: every posting per account in date/bilag order,
/// with running saldo seeded from the inngående balance. `account`
/// filters to one account when given.
pub async fn kontospesifikasjon(
    pool: &PgPool,
    company_id: Uuid,
    account: Option<&str>,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<KontoPost>> {
    let rows = sqlx::query(
        "select a.number, a.name as account_name, j.code as journal_code,
                v.fiscal_year, v.voucher_number, v.voucher_date,
                coalesce(e.description, v.description) as description,
                e.amount_ore, p.party_no,
                ib.saldo as inngaende,
                sum(e.amount_ore) over (partition by a.number
                    order by v.voucher_date, v.chain_seq, e.line_no)::bigint as bevegelse
         from entry e
         join voucher v on v.id = e.voucher_id
         join journal j on j.id = v.journal_id
         join account a on a.id = e.account_id
         left join party p on p.id = e.party_id
         left join lateral (
             select coalesce(sum(e2.amount_ore), 0)::bigint as saldo
             from entry e2 join voucher v2 on v2.id = e2.voucher_id
             where e2.account_id = a.id and v2.voucher_date < $3
         ) ib on true
         where v.company_id = $1
           and ($2::text is null or a.number = $2)
           and v.voucher_date between $3 and $4
         order by a.number, v.voucher_date, v.chain_seq, e.line_no",
    )
    .bind(company_id)
    .bind(account)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| KontoPost {
            number: r.get("number"),
            account_name: r.get("account_name"),
            journal_code: r.get("journal_code"),
            fiscal_year: r.get("fiscal_year"),
            voucher_number: r.get("voucher_number"),
            voucher_date: r.get("voucher_date"),
            description: r.get("description"),
            amount_ore: r.get("amount_ore"),
            saldo_ore: r.get::<i64, _>("inngaende") + r.get::<i64, _>("bevegelse"),
            party_no: r.get("party_no"),
        })
        .collect())
}

#[derive(Debug)]
pub struct BokforingLine {
    pub line_no: i32,
    pub account_number: String,
    pub account_name: String,
    pub amount_ore: i64,
    pub vat_code: Option<String>,
    pub description: Option<String>,
    pub party_no: Option<String>,
}

#[derive(Debug)]
pub struct BokforingVoucher {
    pub journal_code: String,
    pub fiscal_year: i32,
    pub voucher_number: i64,
    pub voucher_date: NaiveDate,
    pub description: String,
    pub lines: Vec<BokforingLine>,
}

/// Bokføringsspesifikasjon: every voucher in the period in posting
/// order (chain order — which is also the audit order), with all lines.
pub async fn bokforingsspesifikasjon(
    pool: &PgPool,
    company_id: Uuid,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<BokforingVoucher>> {
    let rows = sqlx::query(
        "select j.code as journal_code, v.fiscal_year, v.voucher_number, v.voucher_date,
                v.description as voucher_description, v.chain_seq,
                e.line_no, a.number as account_number, a.name as account_name,
                e.amount_ore, e.vat_code, e.description as line_description, p.party_no
         from voucher v
         join journal j on j.id = v.journal_id
         join entry e on e.voucher_id = v.id
         join account a on a.id = e.account_id
         left join party p on p.id = e.party_id
         where v.company_id = $1 and v.voucher_date between $2 and $3
         order by v.chain_seq, e.line_no",
    )
    .bind(company_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    let mut vouchers: Vec<BokforingVoucher> = Vec::new();
    let mut last_seq: Option<i64> = None;
    for row in &rows {
        let seq: i64 = row.get("chain_seq");
        if last_seq != Some(seq) {
            last_seq = Some(seq);
            vouchers.push(BokforingVoucher {
                journal_code: row.get("journal_code"),
                fiscal_year: row.get("fiscal_year"),
                voucher_number: row.get("voucher_number"),
                voucher_date: row.get("voucher_date"),
                description: row.get("voucher_description"),
                lines: Vec::new(),
            });
        }
        vouchers
            .last_mut()
            .expect("voucher pushed above")
            .lines
            .push(BokforingLine {
                line_no: row.get("line_no"),
                account_number: row.get("account_number"),
                account_name: row.get("account_name"),
                amount_ore: row.get("amount_ore"),
                vat_code: row.get("vat_code"),
                description: row.get("line_description"),
                party_no: row.get("party_no"),
            });
    }
    Ok(vouchers)
}
