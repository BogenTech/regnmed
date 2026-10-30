//! Periodisering (#87, docs/periodisering.md): the plan, and the monthly
//! run that posts it.
//!
//! The allocation itself is pure (`regnmed_core::accrual`); this
//! module owns the table and the transaction. Shape follows the
//! avskrivninger (#40): ONE transaction per (plan, month) — voucher plus
//! run row — with a partial unique index making a double posting
//! impossible, and failures logged with their detail instead of
//! stopping the run for everyone else.
//!
//! **Only the net amount is periodisert, never mva.** The tax was
//! settled on the source bilag, dated by the salgsdokument (mval.
//! §15-9); the entries this module posts carry no vat_code at all.

use anyhow::{Context, Result, bail, ensure};
use chrono::{Datelike, NaiveDate};
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::post_voucher_in;

#[derive(Debug)]
pub struct PeriodiseringDraft {
    pub kilde_voucher: Option<Uuid>,
    pub description: String,
    pub income_account: String,
    pub balance_account: String,
    /// Net amount in øre, ledger signs: a prepaid COST is positive.
    pub total_ore: i64,
    pub from: (i32, u32),
    pub to: (i32, u32),
    pub department: Option<String>,
    pub project: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug)]
pub struct Periodisering {
    pub id: Uuid,
    pub description: String,
    pub income_account: String,
    pub balance_account: String,
    pub total_ore: i64,
    pub fra_maned: NaiveDate,
    pub til_maned: NaiveDate,
    pub department: Option<String>,
    pub project: Option<String>,
    pub note: Option<String>,
    pub stopped_date: Option<NaiveDate>,
    /// How much has actually been posted, and over how many months —
    /// computed from the run log, never stored.
    pub fort_ore: i64,
    pub forte_maneder: i64,
}

fn first_of(year: i32, month: u32) -> Result<NaiveDate> {
    NaiveDate::from_ymd_opt(year, month, 1).context("ugyldig måned")
}

/// Creates a plan. The accounts are the caller's — we never guess one.
pub async fn create_periodisering(
    pool: &PgPool,
    company_id: Uuid,
    draft: &PeriodiseringDraft,
    created_by: &str,
) -> Result<Uuid> {
    ensure!(draft.total_ore != 0, "beløpet kan ikke være null");
    ensure!(
        !draft.description.trim().is_empty(),
        "periodiseringen må ha en beskrivelse"
    );
    let from = first_of(draft.from.0, draft.from.1)?;
    let to = first_of(draft.to.0, draft.to.1)?;
    let antall = regnmed_core::accrual::month_count(draft.from, draft.to);
    ensure!(
        antall > 0,
        "til-måneden kan ikke være før fra-måneden ({from} → {to})"
    );
    // A plan whose parts round to nothing is a mistake, not a plan.
    ensure!(
        draft.total_ore.abs() >= i64::from(antall),
        "{} øre fordelt på {antall} måneder gir under ett øre i måneden",
        draft.total_ore.abs()
    );

    // Same reason the dimensions are resolved below: a plan is a standing
    // instruction, so a bad account must fail HERE and not once a month
    // for a year. Found in browser verification — 1500 is a reskontro
    // account, the plan was accepted, and all three months then failed
    // with "requires a party" where only the run log could see it.
    for (account, role) in [
        (&draft.income_account, "resultatkontoen"),
        (&draft.balance_account, "balansekontoen"),
    ] {
        let funnet: Option<(bool, Option<String>)> = sqlx::query_as(
            "select active, reskontro_kind from account where company_id = $1 and number = $2",
        )
        .bind(company_id)
        .bind(account)
        .fetch_optional(pool)
        .await?;
        let (active, subledger) =
            funnet.with_context(|| format!("{role} {account} finnes ikke i kontoplanen"))?;
        ensure!(active, "{role} {account} er deaktivert");
        // Periodisering posts without a party, so a reskontro account
        // could never succeed — refusing now beats refusing monthly.
        ensure!(
            subledger.is_none(),
            "{role} {account} er en reskontrokonto og krever part på hver postering. \
             Periodisering fører uten part, så velg en annen konto"
        );
    }

    // Dimensions are resolved here so a typo fails the plan rather than
    // every monthly posting for the next year.
    let dim = |code: Option<&str>, kind: &'static str| {
        let code = code.map(str::to_owned);
        async move {
            match code {
                None => anyhow::Ok(None),
                Some(code) => {
                    let id: Option<Uuid> = sqlx::query_scalar(
                        "select id from dimension
                         where company_id = $1 and kind = $2 and code = $3 and aktiv",
                    )
                    .bind(company_id)
                    .bind(kind)
                    .bind(&code)
                    .fetch_optional(pool)
                    .await?;
                    Ok(Some(id.with_context(|| {
                        format!("ukjent eller avsluttet {kind} «{code}»")
                    })?))
                }
            }
        }
    };
    let avdeling_id = dim(draft.department.as_deref(), "avdeling").await?;
    let prosjekt_id = dim(draft.project.as_deref(), "prosjekt").await?;

    let id = Uuid::now_v7();
    sqlx::query(
        "insert into periodisering
             (id, company_id, kilde_voucher, beskrivelse, resultatkonto, balansekonto,
              total_ore, fra_maned, til_maned, avdeling_id, prosjekt_id, notat, created_by)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(id)
    .bind(company_id)
    .bind(draft.kilde_voucher)
    .bind(draft.description.trim())
    .bind(&draft.income_account)
    .bind(&draft.balance_account)
    .bind(draft.total_ore)
    .bind(from)
    .bind(to)
    .bind(avdeling_id)
    .bind(prosjekt_id)
    .bind(draft.note.as_deref())
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn list_periodiseringer(pool: &PgPool, company_id: Uuid) -> Result<Vec<Periodisering>> {
    let rows = sqlx::query(
        "select p.id, p.beskrivelse, p.resultatkonto, p.balansekonto, p.total_ore,
                p.fra_maned, p.til_maned, p.notat, p.stoppet_dato,
                da.code as avdeling, dp.code as prosjekt,
                coalesce((select sum(r.belop_ore) from periodisering_run r
                          where r.periodisering_id = p.id and r.voucher_id is not null), 0)
                    ::bigint as fort_ore,
                (select count(*) from periodisering_run r
                  where r.periodisering_id = p.id and r.voucher_id is not null)
                    ::bigint as forte_maneder
         from periodisering p
         left join dimension da on da.id = p.avdeling_id
         left join dimension dp on dp.id = p.prosjekt_id
         where p.company_id = $1
         order by p.fra_maned desc, p.created_at desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Periodisering {
            id: r.get("id"),
            description: r.get("beskrivelse"),
            income_account: r.get("resultatkonto"),
            balance_account: r.get("balansekonto"),
            total_ore: r.get("total_ore"),
            fra_maned: r.get("fra_maned"),
            til_maned: r.get("til_maned"),
            department: r.get("avdeling"),
            project: r.get("prosjekt"),
            note: r.get("notat"),
            stopped_date: r.get("stoppet_dato"),
            fort_ore: r.get("fort_ore"),
            forte_maneder: r.get("forte_maneder"),
        })
        .collect())
}

/// Stops a plan: months already posted stand, the remaining ones are
/// never posted. One-way — a plan is never deleted, because the vouchers
/// already posted point at it.
pub async fn stopp_periodisering(
    pool: &PgPool,
    company_id: Uuid,
    id: Uuid,
    date: NaiveDate,
) -> Result<()> {
    let n = sqlx::query(
        "update periodisering set stoppet_dato = $3
         where id = $1 and company_id = $2 and stoppet_dato is null",
    )
    .bind(id)
    .bind(company_id)
    .bind(date)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(n > 0, "ingen aktiv periodisering å stoppe");
    Ok(())
}

#[derive(Debug)]
pub struct PeriodiseringUtfall {
    pub accrual_id: Uuid,
    pub description: String,
    pub period: NaiveDate,
    pub amount_ore: i64,
    pub voucher: Option<(i32, i64)>,
    pub detail: Option<String>,
}

/// Posts every month that has ended and is not yet posted, for every
/// company. The monthly CronJob's entry point.
pub async fn periodiser_all(pool: &PgPool, through: NaiveDate) -> Result<Vec<PeriodiseringUtfall>> {
    let planer: Vec<(Uuid, Uuid)> =
        sqlx::query("select id, company_id from periodisering order by company_id, created_at")
            .fetch_all(pool)
            .await?
            .iter()
            .map(|r| (r.get("id"), r.get("company_id")))
            .collect();
    let mut utfall = Vec::new();
    for (id, company_id) in planer {
        utfall.extend(periodiser_plan(pool, company_id, id, through).await?);
    }
    Ok(utfall)
}

/// Every due month for ONE plan. Each month is its own transaction, so a
/// failure in March does not roll back February.
pub async fn periodiser_plan(
    pool: &PgPool,
    company_id: Uuid,
    accrual_id: Uuid,
    through: NaiveDate,
) -> Result<Vec<PeriodiseringUtfall>> {
    let plan = sqlx::query(
        "select p.beskrivelse, p.resultatkonto, p.balansekonto, p.total_ore,
                p.fra_maned, p.til_maned, p.stoppet_dato,
                da.code as avdeling, dp.code as prosjekt
         from periodisering p
         left join dimension da on da.id = p.avdeling_id
         left join dimension dp on dp.id = p.prosjekt_id
         where p.id = $1 and p.company_id = $2",
    )
    .bind(accrual_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("ukjent periodisering")?;

    let from: NaiveDate = plan.get("fra_maned");
    let to: NaiveDate = plan.get("til_maned");
    let stoppet: Option<NaiveDate> = plan.get("stoppet_dato");
    let description: String = plan.get("beskrivelse");
    let income_account: String = plan.get("resultatkonto");
    let balance_account: String = plan.get("balansekonto");
    let department: Option<String> = plan.get("avdeling");
    let project: Option<String> = plan.get("prosjekt");
    let rader = regnmed_core::accrual::plan(
        plan.get("total_ore"),
        (from.year(), from.month()),
        (to.year(), to.month()),
    );
    let antall = rader.len();

    let ferdige: Vec<NaiveDate> = sqlx::query_scalar(
        "select period from periodisering_run
         where periodisering_id = $1 and voucher_id is not null",
    )
    .bind(accrual_id)
    .fetch_all(pool)
    .await?;

    let mut utfall = Vec::new();
    for (i, rad) in rader.iter().enumerate() {
        let period = NaiveDate::from_ymd_opt(rad.year, rad.month, 1).expect("gyldig måned");
        // Due: the month has ended, it is not already posted, and the
        // plan was not stopped before it. Stopping is inclusive of the
        // month it happens in only if that month already ended.
        if rad.date > through || ferdige.contains(&period) || stoppet.is_some_and(|d| d <= rad.date)
        {
            continue;
        }
        utfall.push(
            post_en_maned(
                pool,
                company_id,
                accrual_id,
                period,
                rad.date,
                rad.amount_ore,
                &description,
                &income_account,
                &balance_account,
                department.as_deref(),
                project.as_deref(),
                i + 1,
                antall,
            )
            .await?,
        );
    }
    Ok(utfall)
}

#[allow(clippy::too_many_arguments)]
async fn post_en_maned(
    pool: &PgPool,
    company_id: Uuid,
    accrual_id: Uuid,
    period: NaiveDate,
    voucher_date: NaiveDate,
    amount_ore: i64,
    description: &str,
    income_account: &str,
    balance_account: &str,
    department: Option<&str>,
    project: Option<&str>,
    nr: usize,
    antall: usize,
) -> Result<PeriodiseringUtfall> {
    let mut tx = pool.begin().await?;
    let result = async {
        let linje = |account: &str, amount: i64, dims: bool| EntryDraft {
            account_number: account.to_string(),
            amount: Ore(amount),
            // No vat_code, ever: the tax belongs to the source bilag.
            vat_code: None,
            description: Some(format!("Periodisering måned {nr}/{antall}")),
            party_no: None,
            department: if dims {
                department.map(str::to_owned)
            } else {
                None
            },
            project: if dims {
                project.map(str::to_owned)
            } else {
                None
            },
            currency: None,
        };
        let draft = VoucherDraft {
            journal_code: "GL".into(),
            voucher_date,
            description: format!(
                "Periodisering {description} {}-{:02}",
                period.year(),
                period.month()
            ),
            reverses: None,
            entries: vec![
                linje(income_account, amount_ore, true),
                linje(balance_account, -amount_ore, false),
            ],
        };
        draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
        let posted = post_voucher_in(&mut tx, company_id, &draft, "periodisering").await?;
        sqlx::query(
            "insert into periodisering_run (id, periodisering_id, period, belop_ore, voucher_id)
             values ($1,$2,$3,$4,$5)",
        )
        .bind(Uuid::now_v7())
        .bind(accrual_id)
        .bind(period)
        .bind(amount_ore)
        .bind(posted.id)
        .execute(&mut *tx)
        .await?;
        anyhow::Ok(posted)
    }
    .await;

    match result {
        Ok(posted) => {
            tx.commit().await?;
            Ok(PeriodiseringUtfall {
                accrual_id,
                description: description.to_string(),
                period,
                amount_ore,
                voucher: Some((posted.fiscal_year, posted.voucher_number)),
                detail: None,
            })
        }
        Err(err) => {
            drop(tx);
            let detail = format!("{err:#}");
            sqlx::query(
                "insert into periodisering_run (id, periodisering_id, period, belop_ore, detail)
                 values ($1,$2,$3,$4,$5)",
            )
            .bind(Uuid::now_v7())
            .bind(accrual_id)
            .bind(period)
            .bind(amount_ore)
            .bind(&detail)
            .execute(pool)
            .await?;
            Ok(PeriodiseringUtfall {
                accrual_id,
                description: description.to_string(),
                period,
                amount_ore,
                voucher: None,
                detail: Some(detail),
            })
        }
    }
}

/// The run log for one plan (newest first) — what was posted, and what
/// failed and why.
pub async fn list_runs(
    pool: &PgPool,
    company_id: Uuid,
    accrual_id: Uuid,
) -> Result<Vec<(NaiveDate, i64, Option<String>)>> {
    let finnes: bool = sqlx::query_scalar(
        "select exists(select 1 from periodisering where id = $1 and company_id = $2)",
    )
    .bind(accrual_id)
    .bind(company_id)
    .fetch_one(pool)
    .await?;
    if !finnes {
        bail!("ukjent periodisering");
    }
    Ok(sqlx::query(
        "select period, belop_ore, detail from periodisering_run
         where periodisering_id = $1 order by period desc",
    )
    .bind(accrual_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| (r.get("period"), r.get("belop_ore"), r.get("detail")))
    .collect())
}
