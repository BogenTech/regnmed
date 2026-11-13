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

pub struct AccountRow {
    pub number: String,
    pub name: String,
    pub vat_code: Option<String>,
    pub active: bool,
    pub subledger_kind: Option<String>,
    /// Lifetime balance (SUM of entries, øre) — computed, never stored.
    pub balance_ore: i64,
    pub postings: i64,
}

/// Every account the company has, with computed balances. Accounts with
/// zero postings are included — a freshly added account must be visible,
/// or "add account" looks like it did nothing.
pub async fn list_accounts(pool: &PgPool, company_id: Uuid) -> Result<Vec<AccountRow>> {
    let rows = sqlx::query(
        "select a.number, a.name, a.vat_code, a.active, a.reskontro_kind,
                coalesce(sum(t.debit_ore + t.credit_ore), 0)::bigint as saldo_ore,
                coalesce(sum(t.entry_count), 0)::bigint as posteringer
         from account a
         left join account_month_total t on t.account_id = a.id
         where a.company_id = $1
         group by a.id
         order by a.number",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AccountRow {
            number: r.get("number"),
            name: r.get("name"),
            vat_code: r.get("vat_code"),
            active: r.get("active"),
            subledger_kind: r.get("reskontro_kind"),
            balance_ore: r.get("saldo_ore"),
            postings: r.get("posteringer"),
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

pub struct VoucherLine {
    pub account: String,
    pub account_name: String,
    pub amount_ore: i64,
    pub vat_code: Option<String>,
    pub description: Option<String>,
    pub party_no: Option<String>,
    pub department: Option<String>,
    pub project: Option<String>,
}

pub struct VoucherWithLines {
    pub voucher_id: Uuid,
    pub journal_code: String,
    pub fiscal_year: i32,
    pub voucher_number: i64,
    pub voucher_date: chrono::NaiveDate,
    pub description: String,
    pub lines: Vec<VoucherLine>,
}

pub struct VoucherPage {
    pub total: i64,
    pub vouchers: Vec<VoucherWithLines>,
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
    without_attachment: bool,
) -> Result<VoucherPage> {
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
                           and (a.number like $4 || '%' or a.name ilike '%' || $4 || '%')))
         and (not $7::bool or (
              not exists (select 1 from attachment att where att.voucher_id = v.id)
              and not exists (select 1 from journal ji
                              where ji.id = v.journal_id and ji.code = 'IMP')))";

    let total: i64 = sqlx::query_scalar(&format!("select count(*) from voucher v where {filter}"))
        .bind(company_id)
        .bind(from)
        .bind(to)
        .bind(sok)
        .bind(limit)
        .bind(offset)
        .bind(without_attachment)
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
    .bind(without_attachment)
    .fetch_all(pool)
    .await?;

    let ids: Vec<Uuid> = headers.iter().map(|r| r.get("id")).collect();
    let mut vouchers: Vec<VoucherWithLines> = headers
        .into_iter()
        .map(|r| VoucherWithLines {
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
        return Ok(VoucherPage { total, vouchers });
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
            v.lines.push(VoucherLine {
                account: row.get("number"),
                account_name: row.get("name"),
                amount_ore: row.get("amount_ore"),
                vat_code: row.get("vat_code"),
                description: row.get("description"),
                party_no: row.get("party_no"),
                department: row.get("avdeling"),
                project: row.get("prosjekt"),
            });
        }
    }

    Ok(VoucherPage { total, vouchers })
}

/// Post a free-form manual voucher. Everything post_voucher already
/// enforces applies (period lock, active accounts, dims, reskontro,
/// double entry); what this adds is the attestering boundary: when the
/// company runs an active attestering policy and the voucher is at or
/// over the beløpsgrense, a manual posting would BYPASS the intern
/// check (#47) — attestation targets inbox documents. Fail closed and
/// point at the innboks instead of quietly opening a side door.
/// A voucher posted WITHOUT a document (manual, or from a bank line)
/// cannot side-step attestering (#47): with an active policy, anything
/// at or above the limit must come through the innboks, where someone
/// else attests it.
pub(crate) async fn refuse_if_attestation_required(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    draft: &VoucherDraft,
) -> Result<()> {
    if let Some(policy) = crate::approval::current_policy(&mut **tx, company_id).await?
        && policy.active
    {
        let debetsum: i64 = draft.entries.iter().map(|e| e.amount.0.max(0)).sum();
        if policy
            .amount_limit_ore
            .is_none_or(|limit| debetsum >= limit)
        {
            bail!(
                "attesteringspolicyen krever attestering for bilag på dette beløpet — \
                 last dokumentet opp i innboksen og bokfør det derfra"
            );
        }
    }
    Ok(())
}

pub async fn post_manual_voucher(
    pool: &PgPool,
    company_id: Uuid,
    draft: &VoucherDraft,
    created_by: &str,
) -> Result<PostedVoucher> {
    let mut tx = pool.begin().await?;
    refuse_if_attestation_required(&mut tx, company_id, draft).await?;
    let posted = post_voucher_in(&mut tx, company_id, draft, created_by)
        .await
        .context("bokføringen feilet")?;
    tx.commit().await?;
    Ok(posted)
}

/// What owns a voucher whose correction must go through its own flow
/// rather than a plain reversal — and where that flow is. A reversal of
/// an invoice voucher would leave the invoice "issued" with nothing
/// behind it; a reversed payroll month would disagree with the payroll
/// run; and so on. `None` = an ordinary voucher, reversible here.
async fn owned_by<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    voucher_id: Uuid,
) -> Result<Option<String>> {
    let owner: Option<String> = sqlx::query_scalar(
        "select case
           when exists (select 1 from invoice where voucher_id = $1)
             then 'en faktura — rett den med en kreditnota under Faktura'
           when exists (select 1 from invoice_reminder where voucher_id = $1)
             then 'en purring — rettes fra fakturaen under Faktura'
           when exists (select 1 from expense where voucher_id = $1 or utbetalt_voucher_id = $1)
             then 'et utlegg — rettes under Utlegg'
           when exists (select 1 from payment_run where settled_voucher_id = $1)
             then 'en betalingsliste — rettes under Bank'
           when exists (select 1 from asset where anskaffelse_voucher_id = $1 or avhending_voucher_id = $1)
                or exists (select 1 from asset_depreciation where voucher_id = $1)
             then 'et anlegg — rettes under Anlegg'
           when exists (select 1 from payroll_run where voucher_id = $1)
             then 'en lønnskjøring — rettes under Lønn'
           when exists (select 1 from share_event where voucher_id = $1)
                or exists (select 1 from dividend where voucher_id = $1)
             then 'aksjeeierboken — rettes under Aksjonærer'
           when exists (select 1 from periodisering_run where voucher_id = $1)
             then 'en periodisering — stopp planen under Periodisering'
           when exists (select 1 from arsavslutning where voucher_id = $1)
             then 'årsavslutningen'
           when exists (select 1 from voucher v join journal j on j.id = v.journal_id
                        where v.id = $1 and j.code = 'IMP')
             then 'importert historikk (SAF-T) — den rettes ved å importere på nytt, ikke her'
         end",
    )
    .bind(voucher_id)
    .fetch_one(executor)
    .await?;
    Ok(owner)
}

/// One voucher as the hovedbok's voucher page shows it: the header, its
/// lines (with currency), how many attachments it has, the reversal it
/// is or that undid it, and — when another part of the system owns it —
/// where it is corrected instead.
pub struct VoucherDetail {
    pub voucher: VoucherWithLines,
    pub created_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub reverses: Option<(Uuid, String)>,
    pub reversed_by: Option<(Uuid, String)>,
    pub owner: Option<String>,
    pub attachments: i64,
    pub currency: Vec<Option<(String, i64)>>,
}

pub async fn voucher_detail(
    pool: &PgPool,
    company_id: Uuid,
    voucher_id: Uuid,
) -> Result<Option<VoucherDetail>> {
    let Some(h) = sqlx::query(
        "select v.id, j.code as journal_code, v.fiscal_year, v.voucher_number, v.voucher_date,
                v.description, v.created_by, v.created_at,
                o.id as rev_id, o.fiscal_year as rev_year, o.voucher_number as rev_no,
                r.id as by_id, r.fiscal_year as by_year, r.voucher_number as by_no,
                (select count(*) from attachment a where a.voucher_id = v.id) as attachments
         from voucher v
         join journal j on j.id = v.journal_id
         left join voucher o on o.id = v.reverses_voucher_id
         left join voucher r on r.reverses_voucher_id = v.id
         where v.id = $1 and v.company_id = $2",
    )
    .bind(voucher_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let rows = sqlx::query(
        "select a.number, a.name, e.amount_ore, e.vat_code, e.description, p.party_no,
                da.code as avdeling, dp.code as prosjekt, e.valuta, e.valutabelop_cent
         from entry e
         join account a on a.id = e.account_id
         left join party p on p.id = e.party_id
         left join dimension da on da.id = e.avdeling_id
         left join dimension dp on dp.id = e.prosjekt_id
         where e.voucher_id = $1
         order by e.line_no",
    )
    .bind(voucher_id)
    .fetch_all(pool)
    .await?;
    let label = |y: Option<i32>, n: Option<i64>| match (y, n) {
        (Some(y), Some(n)) => Some(format!("{y}-{n}")),
        _ => None,
    };
    let pair = |id: Option<Uuid>, l: Option<String>| id.zip(l);
    Ok(Some(VoucherDetail {
        voucher: VoucherWithLines {
            voucher_id,
            journal_code: h.get("journal_code"),
            fiscal_year: h.get("fiscal_year"),
            voucher_number: h.get("voucher_number"),
            voucher_date: h.get("voucher_date"),
            description: h.get("description"),
            lines: rows
                .iter()
                .map(|r| VoucherLine {
                    account: r.get("number"),
                    account_name: r.get("name"),
                    amount_ore: r.get("amount_ore"),
                    vat_code: r.get("vat_code"),
                    description: r.get("description"),
                    party_no: r.get("party_no"),
                    department: r.get("avdeling"),
                    project: r.get("prosjekt"),
                })
                .collect(),
        },
        created_by: h.get("created_by"),
        created_at: h.get("created_at"),
        reverses: pair(h.get("rev_id"), label(h.get("rev_year"), h.get("rev_no"))),
        reversed_by: pair(h.get("by_id"), label(h.get("by_year"), h.get("by_no"))),
        owner: owned_by(pool, voucher_id).await?,
        attachments: h.get("attachments"),
        currency: rows
            .iter()
            .map(|r| {
                r.get::<Option<String>, _>("valuta")
                    .zip(r.get::<Option<i64>, _>("valutabelop_cent"))
            })
            .collect(),
    }))
}

/// Reverses an ordinary voucher (bokføringsloven § 9: a correction is a
/// new, documented posting, never an edit): every line mirrored — party,
/// dimensions and currency included — on `date`, linked to the original
/// through `reverses_voucher_id`, which the hash covers. In the SAME
/// transaction each reskontro line is matched against its mirror for
/// what was still open, so the reversed invoice or payment does not
/// linger as an open item. Refused for a voucher that is itself a
/// reversal or already reversed, and for one owned by another flow
/// (invoice, payroll, …) — those are corrected where they live.
///
/// No attestering gate: a reversal adds no new economic content, it
/// undoes a posting that already passed whatever gate applied to it.
pub async fn reverse_voucher(
    pool: &PgPool,
    company_id: Uuid,
    voucher_id: Uuid,
    date: chrono::NaiveDate,
    created_by: &str,
) -> Result<PostedVoucher> {
    let mut tx = pool.begin().await?;
    // One reversal per voucher, even when two people press at once.
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text, 1))")
        .bind(voucher_id)
        .execute(&mut *tx)
        .await?;
    let head = sqlx::query(
        "select v.fiscal_year, v.voucher_number, v.description, v.reverses_voucher_id,
                exists (select 1 from voucher r where r.reverses_voucher_id = v.id) as reversed
         from voucher v where v.id = $1 and v.company_id = $2",
    )
    .bind(voucher_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("bilaget finnes ikke")?;
    let label = format!(
        "{}-{}",
        head.get::<i32, _>("fiscal_year"),
        head.get::<i64, _>("voucher_number")
    );
    ensure!(
        head.get::<Option<Uuid>, _>("reverses_voucher_id").is_none(),
        "bilag {label} er selv en tilbakeføring — før det riktige bilaget på nytt i stedet"
    );
    ensure!(
        !head.get::<bool, _>("reversed"),
        "bilag {label} er allerede tilbakeført"
    );
    if let Some(owner) = owned_by(&mut *tx, voucher_id).await? {
        bail!("bilag {label} hører til {owner}");
    }

    let lines = sqlx::query(
        "select e.id, a.number, e.amount_ore, e.vat_code, e.description, p.party_no, e.party_id,
                da.code as avdeling, dp.code as prosjekt, e.valuta, e.valutabelop_cent, e.kurs_micro
         from entry e
         join account a on a.id = e.account_id
         left join party p on p.id = e.party_id
         left join dimension da on da.id = e.avdeling_id
         left join dimension dp on dp.id = e.prosjekt_id
         where e.voucher_id = $1
         order by e.line_no",
    )
    .bind(voucher_id)
    .fetch_all(&mut *tx)
    .await?;
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: format!(
            "Tilbakeføring av bilag {label}: {}",
            head.get::<String, _>("description")
        ),
        reverses: Some(voucher_id),
        entries: lines
            .iter()
            .map(|r| regnmed_core::voucher::EntryDraft {
                account_number: r.get("number"),
                amount: regnmed_core::Ore(-r.get::<i64, _>("amount_ore")),
                vat_code: r.get("vat_code"),
                description: r.get("description"),
                party_no: r.get("party_no"),
                department: r.get("avdeling"),
                project: r.get("prosjekt"),
                currency: r.get::<Option<String>, _>("valuta").map(|code| {
                    regnmed_core::currency::Currency {
                        code,
                        amount_cent: -r.get::<Option<i64>, _>("valutabelop_cent").unwrap_or(0),
                        rate_micro: r.get::<Option<i64>, _>("kurs_micro").unwrap_or(0),
                    }
                }),
            })
            .collect(),
    };
    let posted = post_voucher_in(&mut tx, company_id, &draft, created_by)
        .await
        .context("tilbakeføringen feilet")?;

    // Close each original reskontro line against its mirror, for what
    // was still open on it (an invoice already paid stays matched to the
    // payment; the mirror is then the customer's credit, open as it
    // should be).
    let mirrors: Vec<(i32, Uuid)> = sqlx::query_as(
        "select line_no, id from entry where voucher_id = $1 and party_id is not null order by line_no",
    )
    .bind(posted.id)
    .fetch_all(&mut *tx)
    .await?;
    for (i, original) in lines.iter().enumerate() {
        if original.get::<Option<Uuid>, _>("party_id").is_none() {
            continue;
        }
        let original_id: Uuid = original.get("id");
        let line_no = (i + 1) as i32;
        let Some((_, mirror_id)) = mirrors.iter().find(|(n, _)| *n == line_no) else {
            continue;
        };
        let remaining: i64 = sqlx::query_scalar(
            "select remaining_ore::bigint from subledger_entry_open where entry_id = $1",
        )
        .bind(original_id)
        .fetch_one(&mut *tx)
        .await?;
        if remaining == 0 {
            continue;
        }
        let (entry_a, entry_b) = if remaining > 0 {
            (original_id, *mirror_id)
        } else {
            (*mirror_id, original_id)
        };
        sqlx::query(
            "insert into reskontro_match (id, entry_a, entry_b, amount_ore, matched_by)
             values ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(entry_a)
        .bind(entry_b)
        .bind(remaining.abs())
        .bind(created_by)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(posted)
}
