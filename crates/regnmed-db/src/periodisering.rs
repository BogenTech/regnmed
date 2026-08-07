//! Periodisering (#87, docs/periodisering.md): the plan, and the monthly
//! run that posts it.
//!
//! The allocation itself is pure (`regnmed_core::periodisering`); this
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
    pub beskrivelse: String,
    pub resultatkonto: String,
    pub balansekonto: String,
    /// Net amount in øre, ledger signs: a prepaid COST is positive.
    pub total_ore: i64,
    pub fra: (i32, u32),
    pub til: (i32, u32),
    pub avdeling: Option<String>,
    pub prosjekt: Option<String>,
    pub notat: Option<String>,
}

#[derive(Debug)]
pub struct Periodisering {
    pub id: Uuid,
    pub beskrivelse: String,
    pub resultatkonto: String,
    pub balansekonto: String,
    pub total_ore: i64,
    pub fra_maned: NaiveDate,
    pub til_maned: NaiveDate,
    pub avdeling: Option<String>,
    pub prosjekt: Option<String>,
    pub notat: Option<String>,
    pub stoppet_dato: Option<NaiveDate>,
    /// How much has actually been posted, and over how many months —
    /// computed from the run log, never stored.
    pub fort_ore: i64,
    pub forte_maneder: i64,
}

fn first_of(ar: i32, maned: u32) -> Result<NaiveDate> {
    NaiveDate::from_ymd_opt(ar, maned, 1).context("ugyldig måned")
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
        !draft.beskrivelse.trim().is_empty(),
        "periodiseringen må ha en beskrivelse"
    );
    let fra = first_of(draft.fra.0, draft.fra.1)?;
    let til = first_of(draft.til.0, draft.til.1)?;
    let antall = regnmed_core::periodisering::antall_maneder(draft.fra, draft.til);
    ensure!(
        antall > 0,
        "til-måneden kan ikke være før fra-måneden ({fra} → {til})"
    );
    // A plan whose parts round to nothing is a mistake, not a plan.
    ensure!(
        draft.total_ore.abs() >= i64::from(antall),
        "{} øre fordelt på {antall} måneder gir under ett øre i måneden",
        draft.total_ore.abs()
    );

    // Dimensions are resolved here so a typo fails the plan rather than
    // every monthly posting for the next year.
    let dim = |kode: Option<&str>, kind: &'static str| {
        let kode = kode.map(str::to_owned);
        async move {
            match kode {
                None => anyhow::Ok(None),
                Some(kode) => {
                    let id: Option<Uuid> = sqlx::query_scalar(
                        "select id from dimension
                         where company_id = $1 and kind = $2 and code = $3 and aktiv",
                    )
                    .bind(company_id)
                    .bind(kind)
                    .bind(&kode)
                    .fetch_optional(pool)
                    .await?;
                    Ok(Some(id.with_context(|| {
                        format!("ukjent eller avsluttet {kind} «{kode}»")
                    })?))
                }
            }
        }
    };
    let avdeling_id = dim(draft.avdeling.as_deref(), "avdeling").await?;
    let prosjekt_id = dim(draft.prosjekt.as_deref(), "prosjekt").await?;

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
    .bind(draft.beskrivelse.trim())
    .bind(&draft.resultatkonto)
    .bind(&draft.balansekonto)
    .bind(draft.total_ore)
    .bind(fra)
    .bind(til)
    .bind(avdeling_id)
    .bind(prosjekt_id)
    .bind(draft.notat.as_deref())
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
            beskrivelse: r.get("beskrivelse"),
            resultatkonto: r.get("resultatkonto"),
            balansekonto: r.get("balansekonto"),
            total_ore: r.get("total_ore"),
            fra_maned: r.get("fra_maned"),
            til_maned: r.get("til_maned"),
            avdeling: r.get("avdeling"),
            prosjekt: r.get("prosjekt"),
            notat: r.get("notat"),
            stoppet_dato: r.get("stoppet_dato"),
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
    dato: NaiveDate,
) -> Result<()> {
    let n = sqlx::query(
        "update periodisering set stoppet_dato = $3
         where id = $1 and company_id = $2 and stoppet_dato is null",
    )
    .bind(id)
    .bind(company_id)
    .bind(dato)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(n > 0, "ingen aktiv periodisering å stoppe");
    Ok(())
}

#[derive(Debug)]
pub struct PeriodiseringUtfall {
    pub periodisering_id: Uuid,
    pub beskrivelse: String,
    pub period: NaiveDate,
    pub belop_ore: i64,
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
    periodisering_id: Uuid,
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
    .bind(periodisering_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("ukjent periodisering")?;

    let fra: NaiveDate = plan.get("fra_maned");
    let til: NaiveDate = plan.get("til_maned");
    let stoppet: Option<NaiveDate> = plan.get("stoppet_dato");
    let beskrivelse: String = plan.get("beskrivelse");
    let resultatkonto: String = plan.get("resultatkonto");
    let balansekonto: String = plan.get("balansekonto");
    let avdeling: Option<String> = plan.get("avdeling");
    let prosjekt: Option<String> = plan.get("prosjekt");
    let rader = regnmed_core::periodisering::plan(
        plan.get("total_ore"),
        (fra.year(), fra.month()),
        (til.year(), til.month()),
    );
    let antall = rader.len();

    let ferdige: Vec<NaiveDate> = sqlx::query_scalar(
        "select period from periodisering_run
         where periodisering_id = $1 and voucher_id is not null",
    )
    .bind(periodisering_id)
    .fetch_all(pool)
    .await?;

    let mut utfall = Vec::new();
    for (i, rad) in rader.iter().enumerate() {
        let period = NaiveDate::from_ymd_opt(rad.ar, rad.maned, 1).expect("gyldig måned");
        // Due: the month has ended, it is not already posted, and the
        // plan was not stopped before it. Stopping is inclusive of the
        // month it happens in only if that month already ended.
        if rad.dato > through || ferdige.contains(&period) || stoppet.is_some_and(|d| d <= rad.dato)
        {
            continue;
        }
        utfall.push(
            post_en_maned(
                pool,
                company_id,
                periodisering_id,
                period,
                rad.dato,
                rad.belop_ore,
                &beskrivelse,
                &resultatkonto,
                &balansekonto,
                avdeling.as_deref(),
                prosjekt.as_deref(),
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
    periodisering_id: Uuid,
    period: NaiveDate,
    voucher_date: NaiveDate,
    belop_ore: i64,
    beskrivelse: &str,
    resultatkonto: &str,
    balansekonto: &str,
    avdeling: Option<&str>,
    prosjekt: Option<&str>,
    nr: usize,
    antall: usize,
) -> Result<PeriodiseringUtfall> {
    let mut tx = pool.begin().await?;
    let result = async {
        let linje = |konto: &str, belop: i64, dims: bool| EntryDraft {
            account_number: konto.to_string(),
            amount: Ore(belop),
            // No vat_code, ever: the tax belongs to the source bilag.
            vat_code: None,
            description: Some(format!("Periodisering måned {nr}/{antall}")),
            party_no: None,
            avdeling: if dims {
                avdeling.map(str::to_owned)
            } else {
                None
            },
            prosjekt: if dims {
                prosjekt.map(str::to_owned)
            } else {
                None
            },
            valuta: None,
        };
        let draft = VoucherDraft {
            journal_code: "GL".into(),
            voucher_date,
            description: format!(
                "Periodisering {beskrivelse} {}-{:02}",
                period.year(),
                period.month()
            ),
            reverses: None,
            entries: vec![
                linje(resultatkonto, belop_ore, true),
                linje(balansekonto, -belop_ore, false),
            ],
        };
        draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
        let posted = post_voucher_in(&mut tx, company_id, &draft, "periodisering").await?;
        sqlx::query(
            "insert into periodisering_run (id, periodisering_id, period, belop_ore, voucher_id)
             values ($1,$2,$3,$4,$5)",
        )
        .bind(Uuid::now_v7())
        .bind(periodisering_id)
        .bind(period)
        .bind(belop_ore)
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
                periodisering_id,
                beskrivelse: beskrivelse.to_string(),
                period,
                belop_ore,
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
            .bind(periodisering_id)
            .bind(period)
            .bind(belop_ore)
            .bind(&detail)
            .execute(pool)
            .await?;
            Ok(PeriodiseringUtfall {
                periodisering_id,
                beskrivelse: beskrivelse.to_string(),
                period,
                belop_ore,
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
    periodisering_id: Uuid,
) -> Result<Vec<(NaiveDate, i64, Option<String>)>> {
    let finnes: bool = sqlx::query_scalar(
        "select exists(select 1 from periodisering where id = $1 and company_id = $2)",
    )
    .bind(periodisering_id)
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
    .bind(periodisering_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| (r.get("period"), r.get("belop_ore"), r.get("detail")))
    .collect())
}
