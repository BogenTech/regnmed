//! Anleggsregister (docs/anlegg.md, #40): insert + one-way avhending,
//! lineære avskrivninger generated as ordinary vouchers (one
//! transaction per asset-month, insert-only run log, partial unique
//! index against double depreciation), and the skattemessige
//! saldoberegning as a pure computation over the register.
//!
//! Bokført verdi is never stored: kostpris − SUM(posted depreciation).

use anyhow::{Context, Result, ensure};
use chrono::{Datelike, NaiveDate};
use regnmed_core::Ore;
use regnmed_core::anlegg::{SALDOGRUPPER, gyldig_saldogruppe, manedsbelop, saldo_ar};
use regnmed_core::sats::sats_on;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::post_voucher_in;
use crate::sats::load_satser;

fn month_start(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap()
}

fn month_end(month_start: NaiveDate) -> NaiveDate {
    month_start
        .checked_add_months(chrono::Months::new(1))
        .unwrap()
        .pred_opt()
        .unwrap()
}

/// 1-based month number of `period` in a plan starting at
/// `anskaffelsesdato`'s month.
fn maned_nr(anskaffelsesdato: NaiveDate, period: NaiveDate) -> i32 {
    (period.year() - anskaffelsesdato.year()) * 12
        + (period.month() as i32 - anskaffelsesdato.month() as i32)
        + 1
}

#[derive(Debug, Clone)]
pub struct AssetDraft {
    pub navn: String,
    pub anskaffelsesdato: NaiveDate,
    pub kostpris_ore: i64,
    pub restverdi_ore: i64,
    pub levetid_maneder: i32,
    pub balansekonto: String,
    pub avskrivningskonto: String,
    pub saldogruppe: String,
    pub anskaffelse_voucher_id: Option<Uuid>,
}

/// Registers an asset. Returns the id and an optional warning when the
/// driftsmiddel is under the aktiveringsgrense or under 3 års levetid —
/// the register never refuses (frivillig aktivering is legal), it
/// informs.
pub async fn create_asset(
    pool: &PgPool,
    company_id: Uuid,
    draft: &AssetDraft,
    created_by: &str,
) -> Result<(Uuid, Option<String>)> {
    ensure!(
        gyldig_saldogruppe(&draft.saldogruppe),
        "ukjent saldogruppe {} (a–j)",
        draft.saldogruppe
    );
    ensure!(
        draft.restverdi_ore >= 0 && draft.restverdi_ore < draft.kostpris_ore,
        "restverdi må være mellom 0 og kostpris"
    );
    // At least 1 øre per month: rules out zero-amount months, so every
    // generated period always posts a voucher (the run-log invariant).
    ensure!(
        draft.kostpris_ore - draft.restverdi_ore >= draft.levetid_maneder as i64,
        "avskrivbart beløp må være minst 1 øre per måned av levetiden"
    );
    for konto in [&draft.balansekonto, &draft.avskrivningskonto] {
        let exists: Option<i32> = sqlx::query_scalar(
            "select 1 from account where company_id = $1 and number = $2 and active",
        )
        .bind(company_id)
        .bind(konto)
        .fetch_optional(pool)
        .await?;
        ensure!(exists.is_some(), "no active account {konto} for this company");
    }
    if let Some(voucher_id) = draft.anskaffelse_voucher_id {
        let ok: Option<i32> =
            sqlx::query_scalar("select 1 from voucher where id = $1 and company_id = $2")
                .bind(voucher_id)
                .bind(company_id)
                .fetch_optional(pool)
                .await?;
        ensure!(ok.is_some(), "anskaffelsesbilaget finnes ikke");
    }

    let satser = load_satser(pool).await?;
    let grense = sats_on(&satser, "aktiveringsgrense", draft.anskaffelsesdato);
    let mut warning = None;
    if let Some(grense) = grense
        && draft.kostpris_ore < grense
    {
        warning = Some(format!(
            "kostpris under aktiveringsgrensen ({} kr) — kan kostnadsføres direkte",
            grense / 100
        ));
    } else if draft.levetid_maneder < 36 {
        warning = Some(
            "levetid under 3 år — driftsmidlet kan kostnadsføres direkte".to_string(),
        );
    }

    let id = Uuid::now_v7();
    sqlx::query(
        "insert into asset (id, company_id, navn, anskaffelsesdato, kostpris_ore,
                            restverdi_ore, levetid_maneder, balansekonto, avskrivningskonto,
                            saldogruppe, anskaffelse_voucher_id, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(id)
    .bind(company_id)
    .bind(&draft.navn)
    .bind(draft.anskaffelsesdato)
    .bind(draft.kostpris_ore)
    .bind(draft.restverdi_ore)
    .bind(draft.levetid_maneder)
    .bind(&draft.balansekonto)
    .bind(&draft.avskrivningskonto)
    .bind(&draft.saldogruppe)
    .bind(draft.anskaffelse_voucher_id)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok((id, warning))
}

#[derive(Debug)]
pub struct AssetRow {
    pub id: Uuid,
    pub navn: String,
    pub anskaffelsesdato: NaiveDate,
    pub kostpris_ore: i64,
    pub restverdi_ore: i64,
    pub levetid_maneder: i32,
    pub balansekonto: String,
    pub avskrivningskonto: String,
    pub saldogruppe: String,
    pub akkumulert_ore: i64,
    pub bokfort_ore: i64,
    pub avhendet_dato: Option<NaiveDate>,
    pub vederlag_ore: Option<i64>,
}

pub async fn list_assets(pool: &PgPool, company_id: Uuid) -> Result<Vec<AssetRow>> {
    let rows = sqlx::query(
        "select a.id, a.navn, a.anskaffelsesdato, a.kostpris_ore, a.restverdi_ore,
                a.levetid_maneder, a.balansekonto, a.avskrivningskonto, a.saldogruppe,
                a.avhendet_dato, a.vederlag_ore,
                coalesce((select sum(d.amount_ore) from asset_depreciation d
                          where d.asset_id = a.id and d.voucher_id is not null), 0)::bigint
                    as akkumulert
         from asset a where a.company_id = $1
         order by a.anskaffelsesdato, a.created_at",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let kostpris: i64 = r.get("kostpris_ore");
            let akkumulert: i64 = r.get("akkumulert");
            let avhendet: Option<NaiveDate> = r.get("avhendet_dato");
            AssetRow {
                id: r.get("id"),
                navn: r.get("navn"),
                anskaffelsesdato: r.get("anskaffelsesdato"),
                kostpris_ore: kostpris,
                restverdi_ore: r.get("restverdi_ore"),
                levetid_maneder: r.get("levetid_maneder"),
                balansekonto: r.get("balansekonto"),
                avskrivningskonto: r.get("avskrivningskonto"),
                saldogruppe: r.get("saldogruppe"),
                akkumulert_ore: akkumulert,
                bokfort_ore: if avhendet.is_some() {
                    0
                } else {
                    kostpris - akkumulert
                },
                avhendet_dato: avhendet,
                vederlag_ore: r.get("vederlag_ore"),
            }
        })
        .collect())
}

#[derive(Debug)]
pub struct DepreciationOutcome {
    pub asset_id: Uuid,
    pub navn: String,
    pub period: NaiveDate,
    /// None = failed; detail holds the error.
    pub voucher: Option<(i32, i64)>,
    pub amount_ore: i64,
    pub detail: Option<String>,
}

/// Generates every due monthly depreciation for the company, oldest
/// first: a period is due when its month has ended on `through`, the
/// asset was not yet avhendet in that month, the plan still has months
/// left, and no posted run exists. One transaction per asset-month —
/// a failure (e.g. locked period) is logged and never blocks the rest.
pub async fn depreciate_due(
    pool: &PgPool,
    company_id: Uuid,
    through: NaiveDate,
    created_by: &str,
) -> Result<Vec<DepreciationOutcome>> {
    let assets = sqlx::query(
        "select id, navn from asset where company_id = $1 order by anskaffelsesdato, created_at",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let mut outcomes = Vec::new();
    for asset in &assets {
        let asset_id: Uuid = asset.get("id");
        let navn: String = asset.get("navn");
        loop {
            match depreciate_next(pool, company_id, asset_id, through, created_by).await {
                Ok(Some(outcome)) => {
                    let failed = outcome.voucher.is_none();
                    outcomes.push(DepreciationOutcome {
                        navn: navn.clone(),
                        ..outcome
                    });
                    if failed {
                        break; // logged; retried by the next run
                    }
                }
                Ok(None) => break, // nothing (more) due
                Err(err) => {
                    outcomes.push(DepreciationOutcome {
                        asset_id,
                        navn: navn.clone(),
                        period: through,
                        voucher: None,
                        amount_ore: 0,
                        detail: Some(format!("{err:#}")),
                    });
                    break;
                }
            }
        }
    }
    Ok(outcomes)
}

/// The next due period for one asset, posted in ONE transaction
/// (voucher + run row); Ok(None) when nothing is due.
async fn depreciate_next(
    pool: &PgPool,
    company_id: Uuid,
    asset_id: Uuid,
    through: NaiveDate,
    created_by: &str,
) -> Result<Option<DepreciationOutcome>> {
    let mut tx = pool.begin().await?;
    let Some(asset) = sqlx::query(
        "select navn, anskaffelsesdato, kostpris_ore, restverdi_ore, levetid_maneder,
                balansekonto, avskrivningskonto, avhendet_dato
         from asset where id = $1 and company_id = $2 for update",
    )
    .bind(asset_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    let anskaffelsesdato: NaiveDate = asset.get("anskaffelsesdato");
    let levetid: i32 = asset.get("levetid_maneder");
    let avhendet: Option<NaiveDate> = asset.get("avhendet_dato");

    let last_done: Option<NaiveDate> = sqlx::query_scalar(
        "select max(period) from asset_depreciation
         where asset_id = $1 and voucher_id is not null",
    )
    .bind(asset_id)
    .fetch_one(&mut *tx)
    .await?;
    let period = match last_done {
        Some(p) => p.checked_add_months(chrono::Months::new(1)).unwrap(),
        None => month_start(anskaffelsesdato),
    };
    let nr = maned_nr(anskaffelsesdato, period);
    let voucher_date = month_end(period);
    // Due: month ended, plan not exhausted, asset still owned that month.
    if voucher_date > through
        || nr > levetid
        || avhendet.is_some_and(|d| month_start(d) <= period)
    {
        return Ok(None);
    }

    // create_asset guarantees ≥1 øre per month, so amount > 0 always.
    let amount = manedsbelop(
        asset.get("kostpris_ore"),
        asset.get("restverdi_ore"),
        levetid,
        nr,
    );
    let navn: String = asset.get("navn");
    let result = async {
        let draft = VoucherDraft {
            journal_code: "GL".into(),
            voucher_date,
            description: format!("Avskrivning {navn} {}-{:02}", period.year(), period.month()),
            reverses: None,
            entries: vec![
                EntryDraft {
                    account_number: asset.get("avskrivningskonto"),
                    amount: Ore(amount),
                    vat_code: None,
                    description: Some(format!("Lineær avskrivning måned {nr}/{levetid}")),
                    party_no: None,
                    avdeling: None,
                    prosjekt: None,
                    valuta: None,
                },
                EntryDraft {
                    account_number: asset.get("balansekonto"),
                    amount: Ore(-amount),
                    vat_code: None,
                    description: None,
                    party_no: None,
                    avdeling: None,
                    prosjekt: None,
                    valuta: None,
                },
            ],
        };
        draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
        let posted = post_voucher_in(&mut tx, company_id, &draft, created_by).await?;
        sqlx::query(
            "insert into asset_depreciation (id, asset_id, period, amount_ore, voucher_id)
             values ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(asset_id)
        .bind(period)
        .bind(amount)
        .bind(posted.id)
        .execute(&mut *tx)
        .await?;
        anyhow::Ok(posted)
    }
    .await;

    match result {
        Ok(posted) => {
            tx.commit().await?;
            Ok(Some(DepreciationOutcome {
                asset_id,
                navn,
                period,
                voucher: Some((posted.fiscal_year, posted.voucher_number)),
                amount_ore: amount,
                detail: None,
            }))
        }
        Err(err) => {
            drop(tx);
            let detail = format!("{err:#}");
            sqlx::query(
                "insert into asset_depreciation (id, asset_id, period, amount_ore, detail)
                 values ($1, $2, $3, $4, $5)",
            )
            .bind(Uuid::now_v7())
            .bind(asset_id)
            .bind(period)
            .bind(amount)
            .bind(&detail)
            .execute(pool)
            .await?;
            Ok(Some(DepreciationOutcome {
                asset_id,
                navn,
                period,
                voucher: None,
                amount_ore: amount,
                detail: Some(detail),
            }))
        }
    }
}

/// All companies (the monthly CronJob).
pub async fn depreciate_all(pool: &PgPool, through: NaiveDate) -> Result<Vec<DepreciationOutcome>> {
    let companies: Vec<Uuid> =
        sqlx::query_scalar("select distinct company_id from asset order by company_id")
            .fetch_all(pool)
            .await?;
    let mut outcomes = Vec::new();
    for company_id in companies {
        outcomes.extend(
            depreciate_due(pool, company_id, through, "system (avskrivning)").await?,
        );
    }
    Ok(outcomes)
}

#[derive(Debug)]
pub struct Disposal {
    pub bokfort_ore: i64,
    pub gevinst_ore: i64,
    pub voucher: Option<(i32, i64)>,
}

/// Avhending (salg/utrangering) in ONE transaction: the remaining
/// bokført verdi leaves the balansekonto, vederlaget enters motkonto,
/// and the difference posts as gevinst (credit) or tap (debit). The
/// asset is closed one-way; no voucher when there is nothing to post.
#[allow(clippy::too_many_arguments)]
pub async fn dispose_asset(
    pool: &PgPool,
    company_id: Uuid,
    asset_id: Uuid,
    dato: NaiveDate,
    vederlag_ore: i64,
    motkonto: &str,
    gevinstkonto: &str,
    tapskonto: &str,
    created_by: &str,
) -> Result<Disposal> {
    ensure!(vederlag_ore >= 0, "vederlag kan ikke være negativt");
    let mut tx = pool.begin().await?;
    let asset = sqlx::query(
        "select navn, kostpris_ore, balansekonto, avhendet_dato,
                coalesce((select sum(d.amount_ore) from asset_depreciation d
                          where d.asset_id = asset.id and d.voucher_id is not null), 0)::bigint
                    as akkumulert
         from asset where id = $1 and company_id = $2 for update",
    )
    .bind(asset_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such asset")?;
    ensure!(
        asset.get::<Option<NaiveDate>, _>("avhendet_dato").is_none(),
        "driftsmidlet er allerede avhendet"
    );
    let bokfort = asset.get::<i64, _>("kostpris_ore") - asset.get::<i64, _>("akkumulert");
    let gevinst = vederlag_ore - bokfort;

    let mut entries = Vec::new();
    if vederlag_ore != 0 {
        entries.push(EntryDraft {
            account_number: motkonto.to_string(),
            amount: Ore(vederlag_ore),
            vat_code: None,
            description: Some("Vederlag ved avhending".into()),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
    }
    if bokfort != 0 {
        entries.push(EntryDraft {
            account_number: asset.get("balansekonto"),
            amount: Ore(-bokfort),
            vat_code: None,
            description: Some("Utgang bokført verdi".into()),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
    }
    if gevinst != 0 {
        entries.push(EntryDraft {
            account_number: if gevinst > 0 {
                gevinstkonto.to_string()
            } else {
                tapskonto.to_string()
            },
            amount: Ore(-gevinst),
            vat_code: None,
            description: Some(
                if gevinst > 0 { "Gevinst ved avgang" } else { "Tap ved avgang" }.into(),
            ),
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
    }

    let voucher = if entries.is_empty() {
        None
    } else {
        let navn: String = asset.get("navn");
        let draft = VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: dato,
            description: format!("Avhending {navn}"),
            reverses: None,
            entries,
        };
        draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
        let posted = post_voucher_in(&mut tx, company_id, &draft, created_by).await?;
        Some(posted)
    };

    sqlx::query(
        "update asset set avhendet_dato = $3, vederlag_ore = $4, avhending_voucher_id = $5
         where id = $1 and company_id = $2",
    )
    .bind(asset_id)
    .bind(company_id)
    .bind(dato)
    .bind(vederlag_ore)
    .bind(voucher.as_ref().map(|p| p.id))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Disposal {
        bokfort_ore: bokfort,
        gevinst_ore: gevinst,
        voucher: voucher.map(|p| (p.fiscal_year, p.voucher_number)),
    })
}

#[derive(Debug)]
pub struct SaldoGruppeRad {
    pub gruppe: String,
    pub beskrivelse: String,
    pub inngaende_ore: i64,
    pub tilgang_ore: i64,
    pub vederlag_ore: i64,
    pub grunnlag_ore: i64,
    pub sats_bp: i64,
    pub avskrivning_ore: i64,
    pub utgaende_ore: i64,
}

#[derive(Debug)]
pub struct SaldoRapport {
    pub year: i32,
    pub grupper: Vec<SaldoGruppeRad>,
    /// Regnskapsmessig bokført verdi ved årets utgang.
    pub bokfort_ore: i64,
    /// Sum utgående skattemessig saldo.
    pub skattemessig_ore: i64,
    /// Midlertidig forskjell: bokført − skattemessig.
    pub forskjell_ore: i64,
}

/// Skattemessig saldoavskrivning per gruppe for `year`, computed from
/// scratch over the whole register (tilganger and vederlag per year,
/// rates from the satsregister per year) — nothing stored, identical
/// re-runs forever. Assets acquired before the register's rate
/// coverage make the year fail loudly rather than guess.
pub async fn saldo_rapport(pool: &PgPool, company_id: Uuid, year: i32) -> Result<SaldoRapport> {
    let assets = sqlx::query(
        "select anskaffelsesdato, kostpris_ore, saldogruppe, avhendet_dato, vederlag_ore
         from asset where company_id = $1",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let satser = load_satser(pool).await?;

    let first_year = assets
        .iter()
        .map(|r| r.get::<NaiveDate, _>("anskaffelsesdato").year())
        .min()
        .unwrap_or(year);
    let mut grupper = Vec::new();
    for (gruppe, beskrivelse) in SALDOGRUPPER {
        let mut inngaende = 0i64;
        let mut rad: Option<SaldoGruppeRad> = None;
        for y in first_year..=year {
            let tilgang: i64 = assets
                .iter()
                .filter(|r| {
                    r.get::<String, _>("saldogruppe") == *gruppe
                        && r.get::<NaiveDate, _>("anskaffelsesdato").year() == y
                })
                .map(|r| r.get::<i64, _>("kostpris_ore"))
                .sum();
            let vederlag: i64 = assets
                .iter()
                .filter(|r| {
                    r.get::<String, _>("saldogruppe") == *gruppe
                        && r.get::<Option<NaiveDate>, _>("avhendet_dato")
                            .is_some_and(|d| d.year() == y)
                })
                .map(|r| r.get::<Option<i64>, _>("vederlag_ore").unwrap_or(0))
                .sum();
            if inngaende == 0 && tilgang == 0 && vederlag == 0 {
                continue;
            }
            let jan1 = NaiveDate::from_ymd_opt(y, 1, 1).unwrap();
            let sats_bp = sats_on(&satser, &format!("saldogruppe_{gruppe}"), jan1)
                .with_context(|| {
                    format!("ingen sats for saldogruppe {gruppe} i {y} — satsregisteret dekker ikke året")
                })?;
            let ar = saldo_ar(inngaende, tilgang, vederlag, sats_bp);
            rad = Some(SaldoGruppeRad {
                gruppe: (*gruppe).to_string(),
                beskrivelse: (*beskrivelse).to_string(),
                inngaende_ore: inngaende,
                tilgang_ore: tilgang,
                vederlag_ore: vederlag,
                grunnlag_ore: ar.grunnlag_ore,
                sats_bp,
                avskrivning_ore: ar.avskrivning_ore,
                utgaende_ore: ar.utgaende_ore,
            });
            inngaende = ar.utgaende_ore;
        }
        if let Some(rad) = rad {
            grupper.push(rad);
        }
    }

    // Regnskapsmessig bokført verdi ved årsslutt: kostpris − posted
    // depreciation dated within the year, for assets owned at year end.
    let year_end = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    let bokfort: i64 = sqlx::query_scalar(
        "select coalesce(sum(a.kostpris_ore
                    - coalesce((select sum(d.amount_ore) from asset_depreciation d
                                where d.asset_id = a.id and d.voucher_id is not null
                                  and d.period <= $2), 0)), 0)::bigint
         from asset a
         where a.company_id = $1 and a.anskaffelsesdato <= $2
           and (a.avhendet_dato is null or a.avhendet_dato > $2)",
    )
    .bind(company_id)
    .bind(year_end)
    .fetch_one(pool)
    .await?;
    let skattemessig: i64 = grupper.iter().map(|g| g.utgaende_ore).sum();
    Ok(SaldoRapport {
        year,
        grupper,
        bokfort_ore: bokfort,
        skattemessig_ore: skattemessig,
        forskjell_ore: bokfort - skattemessig,
    })
}

#[derive(Debug)]
pub struct DepreciationRun {
    pub period: NaiveDate,
    pub amount_ore: i64,
    pub voucher_number: Option<i64>,
    pub fiscal_year: Option<i32>,
    pub detail: Option<String>,
}

pub async fn list_depreciations(
    pool: &PgPool,
    company_id: Uuid,
    asset_id: Uuid,
) -> Result<Vec<DepreciationRun>> {
    let rows = sqlx::query(
        "select d.period, d.amount_ore, v.voucher_number, v.fiscal_year, d.detail
         from asset_depreciation d
         join asset a on a.id = d.asset_id
         left join voucher v on v.id = d.voucher_id
         where d.asset_id = $1 and a.company_id = $2
         order by d.period desc, d.created_at desc
         limit 100",
    )
    .bind(asset_id)
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| DepreciationRun {
            period: r.get("period"),
            amount_ore: r.get("amount_ore"),
            voucher_number: r.get("voucher_number"),
            fiscal_year: r.get("fiscal_year"),
            detail: r.get("detail"),
        })
        .collect())
}
