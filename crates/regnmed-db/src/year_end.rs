//! Årsavslutning (#84, docs/arsavslutning.md): resultatdisponering og
//! skattekostnad, as an ordinary voucher.
//!
//! The disposition account is 8800, and that is Skatteetatens choice
//! rather than ours — its næringsspesifikasjon code list gives 8800 its
//! own grouping category, separate from every income-statement line
//! (`regnmed_core::statements::ALLOCATION_ACCOUNT`).
//!
//! Ordering: the closing SETS the period lock rather than requiring it,
//! because the voucher is dated 31.12 and the database trigger from
//! migration 0011 refuses postings into a locked period regardless of
//! what the application thinks. Same protection, opposite order — see
//! the migration for the full reasoning.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Konti the closing uses. Named here rather than guessed per call:
/// 8300/2500 is the ordinary betalbar-skatt pair, 8800/2050 the
/// disposition.
pub const TAX_EXPENSE_ACCOUNT: &str = "8300";
pub const TAX_PAYABLE_ACCOUNT: &str = "2500";
pub const RETAINED_EARNINGS_ACCOUNT: &str = "2050";

#[derive(Debug)]
pub struct Arsavslutning {
    pub year: i32,
    pub voucher: (i32, i64),
    pub result_before_tax_ore: i64,
    pub tax_expense_ore: i64,
    pub allocated_ore: i64,
    pub created_by: String,
}

/// The fiscal year's span — the calendar-year assumption lives in ONE
/// place (`regnmed_core::fiscal_year`, docs/regelverk.md), and this goes
/// through that seam like the posting and the SAF-T export do.
fn period(year: i32) -> Result<(NaiveDate, NaiveDate)> {
    regnmed_core::fiscal_year::period(year).with_context(|| format!("ugyldig år {year}"))
}

/// The year's result before tax, from the ledger — presentation sign,
/// positive = overskudd. Read here rather than taken from the caller:
/// the number that gets disponert must be the ledger's own.
pub async fn result_for_the_year(pool: &PgPool, company_id: Uuid, year: i32) -> Result<i64> {
    let (from, to) = period(year)?;
    let lines = crate::balance_lines(pool, company_id, Some(from), to, None, None).await?;
    Ok(regnmed_core::statements::result(&lines).annual_result_ore)
}

/// What a closing posts, or would post.
#[derive(Debug)]
pub struct Closing {
    pub closing: Arsavslutning,
    /// (account, amount, text) — the voucher's lines.
    pub lines: Vec<(String, i64, String)>,
    /// The lock that was in place past 31.12 and was put back after the
    /// voucher, when the closing had to reopen the year to post it.
    pub relocked_through: Option<NaiveDate>,
}

/// Closes a year: accrues the tax, transfers the result to equity, and
/// locks the year — one voucher, one transaction.
///
/// `skattekostnad_ore` is the CALLER'S number, not a percentage of the
/// accounting profit. Taxable income is not accounting income
/// (permanent and temporary differences; `balance_report` already
/// computes the latter for anleggsmidler), so deriving it here would be
/// inventing a tax return. Zero is a legitimate answer and must be
/// stated, never assumed.
///
/// The closing is done in spring, when the first months of the new year
/// are usually locked already for VAT. The voucher is dated 31.12, so it
/// then has to go into a locked period: with `allow_reopen` (admin) the
/// lock is moved back to 30.12, the voucher posted, and the lock put
/// back — three rows in the insert-only history, one transaction, so no
/// other posting ever sees the year open.
///
/// `preview` posts and rolls back, so the voucher numbering keeps no gap.
/// `expected_result_ore` is the result the person saw in the preview; a
/// ledger that has moved since is refused rather than closed on a number
/// nobody looked at.
#[allow(clippy::too_many_arguments)]
pub async fn close_year(
    pool: &PgPool,
    company_id: Uuid,
    year: i32,
    tax_expense_ore: i64,
    created_by: &str,
    allow_reopen: bool,
    preview: bool,
    expected_result_ore: Option<i64>,
) -> Result<Closing> {
    ensure!(
        tax_expense_ore >= 0,
        "Skattekostnaden kan ikke være negativ. En skattefordel er utsatt skatt, som ikke er bygget ennå."
    );
    let (_, end_of_year) = period(year)?;

    let mut tx = pool.begin().await?;
    crate::period::lock_period_changes(&mut tx, company_id).await?;

    let today: NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&mut *tx)
        .await?;
    ensure!(
        end_of_year < today,
        "{year} er ikke over ennå. Året kan avsluttes fra 1. januar {}.",
        year + 1
    );

    let alt_avsluttet: Option<i32> =
        sqlx::query_scalar("select ar from arsavslutning where company_id = $1 and ar = $2")
            .bind(company_id)
            .bind(year)
            .fetch_optional(&mut *tx)
            .await?;
    ensure!(
        alt_avsluttet.is_none(),
        "{year} er allerede avsluttet. En korreksjon er et reverserende bilag, ikke en ny avslutning."
    );

    // A lock on or past 31.12 would refuse our own voucher. Moving it
    // back is a reopening, so it takes the same right as any reopening.
    let las: Option<NaiveDate> = sqlx::query_scalar("select current_period_lock($1)")
        .bind(company_id)
        .fetch_one(&mut *tx)
        .await?;
    let relock = las.filter(|l| *l >= end_of_year);
    if let Some(l) = relock {
        ensure!(
            allow_reopen,
            "Perioden er låst til og med {}, så bilaget for {year} (datert {}) må føres i en låst \
             periode. Bare en administrator kan avslutte året: låsen flyttes tilbake mens bilaget \
             føres og settes på plass igjen, og begge deler logges.",
            crate::period::display_date(l),
            crate::period::display_date(end_of_year)
        );
    }

    let result_before_tax_ore = result_for_the_year(pool, company_id, year).await?;
    if let Some(expected) = expected_result_ore {
        ensure!(
            expected == result_before_tax_ore,
            "Resultatet for {year} har endret seg siden du så over avslutningen. Se over den på nytt."
        );
    }
    let allocated_ore = result_before_tax_ore - tax_expense_ore;

    let linje = |account: &str, amount: i64, text: &str| EntryDraft {
        account_number: account.to_string(),
        amount: Ore(amount),
        vat_code: None,
        description: Some(text.to_string()),
        party_no: None,
        department: None,
        project: None,
        currency: None,
    };
    let mut entries = Vec::with_capacity(4);
    if tax_expense_ore != 0 {
        entries.push(linje(TAX_EXPENSE_ACCOUNT, tax_expense_ore, "Skattekostnad"));
        entries.push(linje(
            TAX_PAYABLE_ACCOUNT,
            -tax_expense_ore,
            "Betalbar skatt",
        ));
    }
    // A zero result still closes the year — the year happened, and the
    // arsavslutning row is what says so. The voucher just has no
    // disposition lines then.
    if allocated_ore != 0 {
        entries.push(linje(
            regnmed_core::statements::ALLOCATION_ACCOUNT,
            allocated_ore,
            "Disponering av årets resultat",
        ));
        entries.push(linje(
            RETAINED_EARNINGS_ACCOUNT,
            -allocated_ore,
            "Overført til annen egenkapital",
        ));
    }
    ensure!(
        !entries.is_empty(),
        "{year} har verken resultat eller skattekostnad å disponere."
    );
    let lines = entries
        .iter()
        .map(|e| {
            (
                e.account_number.clone(),
                e.amount.0,
                e.description.clone().unwrap_or_default(),
            )
        })
        .collect();

    for (number, name) in [
        (TAX_EXPENSE_ACCOUNT, "Skattekostnad"),
        (TAX_PAYABLE_ACCOUNT, "Betalbar skatt"),
        (
            regnmed_core::statements::ALLOCATION_ACCOUNT,
            "Disponering av årets resultat",
        ),
        (RETAINED_EARNINGS_ACCOUNT, "Annen egenkapital"),
    ] {
        sqlx::query(
            "insert into account (id, company_id, number, name) values ($1, $2, $3, $4)
             on conflict (company_id, number) do nothing",
        )
        .bind(Uuid::now_v7())
        .bind(company_id)
        .bind(number)
        .bind(name)
        .execute(&mut *tx)
        .await?;
    }

    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: end_of_year,
        description: format!("Årsavslutning {year}"),
        reverses: None,
        entries,
    };

    let insert_lock = |through: NaiveDate, by: String| {
        sqlx::query(
            "insert into period_lock (id, company_id, locked_through, set_by)
             values ($1,$2,$3,$4)",
        )
        .bind(Uuid::now_v7())
        .bind(company_id)
        .bind(through)
        .bind(by)
    };
    if relock.is_some() {
        insert_lock(
            end_of_year.pred_opt().expect("31.12 has a day before it"),
            format!("årsavslutning {year}, gjenåpnet for bilaget ({created_by})"),
        )
        .execute(&mut *tx)
        .await?;
    }
    let posted = crate::post_voucher_in(&mut tx, company_id, &draft, created_by).await?;
    sqlx::query(
        "insert into arsavslutning
             (id, company_id, ar, voucher_id, resultat_for_skatt_ore,
              skattekostnad_ore, disponert_ore, created_by)
         values ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(year)
    .bind(posted.id)
    .bind(result_before_tax_ore)
    .bind(tax_expense_ore)
    .bind(allocated_ore)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    // The lock is part of the same act: after this, the year is both
    // disponert and closed to new postings — and a lock that reached
    // past it is back where it was.
    insert_lock(
        relock.unwrap_or(end_of_year),
        format!("årsavslutning {year} ({created_by})"),
    )
    .execute(&mut *tx)
    .await?;

    if preview {
        tx.rollback().await?;
    } else {
        tx.commit().await?;
    }

    Ok(Closing {
        closing: Arsavslutning {
            year,
            voucher: (posted.fiscal_year, posted.voucher_number),
            result_before_tax_ore,
            tax_expense_ore,
            allocated_ore,
            created_by: created_by.to_string(),
        },
        lines,
        relocked_through: relock,
    })
}

/// The closed years, newest first.
pub async fn list_arsavslutninger(pool: &PgPool, company_id: Uuid) -> Result<Vec<Arsavslutning>> {
    Ok(sqlx::query(
        "select a.ar, v.fiscal_year, v.voucher_number, a.resultat_for_skatt_ore,
                a.skattekostnad_ore, a.disponert_ore, a.created_by
         from arsavslutning a join voucher v on v.id = a.voucher_id
         where a.company_id = $1 order by a.ar desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| Arsavslutning {
        year: r.get("ar"),
        voucher: (r.get("fiscal_year"), r.get("voucher_number")),
        result_before_tax_ore: r.get("resultat_for_skatt_ore"),
        tax_expense_ore: r.get("skattekostnad_ore"),
        allocated_ore: r.get("disponert_ore"),
        created_by: r.get("created_by"),
    })
    .collect())
}
