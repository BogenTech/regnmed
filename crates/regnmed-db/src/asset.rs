//! Anleggsregister (docs/anlegg.md, #40): insert + one-way avhending,
//! lineære avskrivninger generated as ordinary vouchers (one
//! transaction per asset-month, insert-only run log, partial unique
//! index against double depreciation), and the skattemessige
//! saldoberegning as a pure computation over the register.
//!
//! Bokført verdi is never stored: kostpris − SUM(posted depreciation).

use anyhow::{Context, Result, bail, ensure};
use chrono::{Datelike, NaiveDate};
use regnmed_core::Ore;
use regnmed_core::assets::{
    DEPRECIATION_GROUPS, is_valid_depreciation_group, monthly_amount, tax_depreciation_year,
};
use regnmed_core::rates::value_on;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::post_voucher_in;
use crate::rates::load_rates;

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
fn month_number(acquisition_date: NaiveDate, period: NaiveDate) -> i32 {
    (period.year() - acquisition_date.year()) * 12
        + (period.month() as i32 - acquisition_date.month() as i32)
        + 1
}

#[derive(Debug, Clone)]
pub struct AssetDraft {
    pub name: String,
    pub acquisition_date: NaiveDate,
    pub cost_price_ore: i64,
    pub residual_value_ore: i64,
    pub lifetime_months: i32,
    pub balance_account: String,
    pub depreciation_account: String,
    pub depreciation_group: String,
    pub anskaffelse_voucher_id: Option<Uuid>,
}

/// Makes sure `number` is an active account of the company. An account
/// from the standard catalog the company has not used yet is added with
/// its standard name — the picker offers those — while a deactivated or
/// unknown one is refused, naming it.
async fn adopt_account(
    conn: &mut sqlx::PgConnection,
    company_id: Uuid,
    number: &str,
) -> Result<()> {
    let active: Option<bool> =
        sqlx::query_scalar("select active from account where company_id = $1 and number = $2")
            .bind(company_id)
            .bind(number)
            .fetch_optional(&mut *conn)
            .await?;
    match active {
        Some(true) => Ok(()),
        Some(false) => bail!("Konto {number} er deaktivert i kontoplanen"),
        None => {
            let standard = number.parse::<u32>().ok().and_then(|n| {
                regnmed_core::saft::standard_accounts()
                    .iter()
                    .find(|(num, _)| *num == n)
                    .map(|(_, name)| *name)
            });
            let Some(name) = standard else {
                bail!("Konto {number} finnes ikke i kontoplanen")
            };
            sqlx::query(
                "insert into account (id, company_id, number, name) values ($1, $2, $3, $4)
                 on conflict (company_id, number) do nothing",
            )
            .bind(Uuid::now_v7())
            .bind(company_id)
            .bind(number)
            .bind(name)
            .execute(&mut *conn)
            .await?;
            Ok(())
        }
    }
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
    ensure!(!draft.name.trim().is_empty(), "Gi driftsmidlet et navn");
    ensure!(
        is_valid_depreciation_group(&draft.depreciation_group),
        "Ukjent saldogruppe «{}» (a–j)",
        draft.depreciation_group
    );
    ensure!(draft.cost_price_ore > 0, "Kostprisen må være større enn 0");
    ensure!(
        draft.lifetime_months > 0,
        "Levetiden må være minst én måned"
    );
    ensure!(
        draft.residual_value_ore >= 0 && draft.residual_value_ore < draft.cost_price_ore,
        "Restverdien må være mellom 0 og kostprisen"
    );
    // At least 1 øre per month: rules out zero-amount months, so every
    // generated period always posts a voucher (the run-log invariant).
    ensure!(
        draft.cost_price_ore - draft.residual_value_ore >= draft.lifetime_months as i64,
        "Avskrivbart beløp må være minst 1 øre per måned av levetiden"
    );
    let mut tx = pool.begin().await?;
    for account in [&draft.balance_account, &draft.depreciation_account] {
        adopt_account(&mut tx, company_id, account).await?;
    }
    if let Some(voucher_id) = draft.anskaffelse_voucher_id {
        let ok: Option<i32> =
            sqlx::query_scalar("select 1 from voucher where id = $1 and company_id = $2")
                .bind(voucher_id)
                .bind(company_id)
                .fetch_optional(&mut *tx)
                .await?;
        ensure!(ok.is_some(), "Fant ikke anskaffelsesbilaget");
    }

    let rates = load_rates(pool).await?;
    let limit = value_on(&rates, "aktiveringsgrense", draft.acquisition_date);
    let mut warning = None;
    if let Some(limit) = limit
        && draft.cost_price_ore < limit
    {
        warning = Some(format!(
            "kostpris under aktiveringsgrensen ({} kr) — kan kostnadsføres direkte",
            limit / 100
        ));
    } else if draft.lifetime_months < 36 {
        warning = Some("levetid under 3 år — driftsmidlet kan kostnadsføres direkte".to_string());
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
    .bind(draft.name.trim())
    .bind(draft.acquisition_date)
    .bind(draft.cost_price_ore)
    .bind(draft.residual_value_ore)
    .bind(draft.lifetime_months)
    .bind(&draft.balance_account)
    .bind(&draft.depreciation_account)
    .bind(&draft.depreciation_group)
    .bind(draft.anskaffelse_voucher_id)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((id, warning))
}

#[derive(Debug)]
pub struct AssetRow {
    pub id: Uuid,
    pub name: String,
    pub acquisition_date: NaiveDate,
    pub cost_price_ore: i64,
    pub residual_value_ore: i64,
    pub lifetime_months: i32,
    pub balance_account: String,
    pub depreciation_account: String,
    pub depreciation_group: String,
    pub akkumulert_ore: i64,
    pub posted_ore: i64,
    pub disposed_date: Option<NaiveDate>,
    pub consideration_ore: Option<i64>,
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
            let cost_price: i64 = r.get("kostpris_ore");
            let akkumulert: i64 = r.get("akkumulert");
            let avhendet: Option<NaiveDate> = r.get("avhendet_dato");
            AssetRow {
                id: r.get("id"),
                name: r.get("navn"),
                acquisition_date: r.get("anskaffelsesdato"),
                cost_price_ore: cost_price,
                residual_value_ore: r.get("restverdi_ore"),
                lifetime_months: r.get("levetid_maneder"),
                balance_account: r.get("balansekonto"),
                depreciation_account: r.get("avskrivningskonto"),
                depreciation_group: r.get("saldogruppe"),
                akkumulert_ore: akkumulert,
                posted_ore: if avhendet.is_some() {
                    0
                } else {
                    cost_price - akkumulert
                },
                disposed_date: avhendet,
                consideration_ore: r.get("vederlag_ore"),
            }
        })
        .collect())
}

#[derive(Debug)]
pub struct DepreciationOutcome {
    pub asset_id: Uuid,
    pub name: String,
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
        let name: String = asset.get("navn");
        loop {
            match depreciate_next(pool, company_id, asset_id, through, created_by).await {
                Ok(Some(outcome)) => {
                    let failed = outcome.voucher.is_none();
                    outcomes.push(DepreciationOutcome {
                        name: name.clone(),
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
                        name: name.clone(),
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
    let acquisition_date: NaiveDate = asset.get("anskaffelsesdato");
    let lifetime: i32 = asset.get("levetid_maneder");
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
        None => month_start(acquisition_date),
    };
    let number = month_number(acquisition_date, period);
    let voucher_date = month_end(period);
    // Due: month ended, plan not exhausted, asset still owned that month.
    if voucher_date > through
        || number > lifetime
        || avhendet.is_some_and(|d| month_start(d) <= period)
    {
        return Ok(None);
    }

    // create_asset guarantees ≥1 øre per month, so amount > 0 always.
    let amount = monthly_amount(
        asset.get("kostpris_ore"),
        asset.get("restverdi_ore"),
        lifetime,
        number,
    );
    // A month that ended inside a locked period can no longer be posted
    // on its own date (bokføringsloven §9): it is caught up on the first
    // open day instead, still one voucher per month and saying so. Without
    // this one locked month stopped the asset's plan for good, since the
    // months are posted in order.
    let lock: Option<NaiveDate> = sqlx::query_scalar("select current_period_lock($1)")
        .bind(company_id)
        .fetch_one(&mut *tx)
        .await?;
    let (voucher_date, caught_up) = match lock {
        Some(locked) if voucher_date <= locked && locked < through => {
            (locked.succ_opt().unwrap(), true)
        }
        _ => (voucher_date, false),
    };
    let name: String = asset.get("navn");
    let result = async {
        let draft = VoucherDraft {
            journal_code: "GL".into(),
            voucher_date,
            description: format!(
                "Avskrivning {name} {}-{:02}{}",
                period.year(),
                period.month(),
                if caught_up {
                    " (innhentet, perioden var låst)"
                } else {
                    ""
                }
            ),
            reverses: None,
            entries: vec![
                EntryDraft {
                    account_number: asset.get("avskrivningskonto"),
                    amount: Ore(amount),
                    vat_code: None,
                    description: Some(format!("Lineær avskrivning måned {number}/{lifetime}")),
                    party_no: None,
                    department: None,
                    project: None,
                    currency: None,
                },
                EntryDraft {
                    account_number: asset.get("balansekonto"),
                    amount: Ore(-amount),
                    vat_code: None,
                    description: None,
                    party_no: None,
                    department: None,
                    project: None,
                    currency: None,
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
                name,
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
                name,
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
        outcomes.extend(depreciate_due(pool, company_id, through, "system (avskrivning)").await?);
    }
    Ok(outcomes)
}

#[derive(Debug)]
pub struct Disposal {
    pub posted_ore: i64,
    pub gain_ore: i64,
    /// What the voucher posts (or would post, on a preview): account,
    /// signed amount, text.
    pub lines: Vec<(String, i64, String)>,
    pub voucher: Option<(i32, i64)>,
}

/// Avhending (salg/utrangering) in ONE transaction: the remaining
/// bokført verdi leaves the balansekonto, the consideration enters motkonto,
/// and the difference posts as gevinst (credit) or tap (debit). The
/// asset is closed one-way; no voucher when there is nothing to post.
///
/// `preview` does all of it and rolls back, so a locked period or a
/// missing account shows before anyone confirms (the counter rolls back
/// too, so no voucher number is burnt). The confirm carries the book
/// value the person saw, and is refused if a depreciation posted since.
#[allow(clippy::too_many_arguments)]
pub async fn dispose_asset(
    pool: &PgPool,
    company_id: Uuid,
    asset_id: Uuid,
    date: NaiveDate,
    consideration_ore: i64,
    contra_account: &str,
    gain_account: &str,
    loss_account: &str,
    preview: bool,
    expected_posted_ore: Option<i64>,
    created_by: &str,
) -> Result<Disposal> {
    ensure!(consideration_ore >= 0, "Vederlaget kan ikke være negativt");
    let mut tx = pool.begin().await?;
    let asset = sqlx::query(
        "select navn, anskaffelsesdato, kostpris_ore, balansekonto, avhendet_dato,
                coalesce((select sum(d.amount_ore) from asset_depreciation d
                          where d.asset_id = asset.id and d.voucher_id is not null), 0)::bigint
                    as akkumulert,
                (select max(d.period) from asset_depreciation d
                  where d.asset_id = asset.id and d.voucher_id is not null) as last_period
         from asset where id = $1 and company_id = $2 for update",
    )
    .bind(asset_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("Fant ikke driftsmidlet")?;
    ensure!(
        asset.get::<Option<NaiveDate>, _>("avhendet_dato").is_none(),
        "Driftsmidlet er allerede avhendet"
    );
    let acquired: NaiveDate = asset.get("anskaffelsesdato");
    ensure!(
        date >= acquired,
        "Avhendingsdatoen kan ikke være før anskaffelsesdatoen ({})",
        acquired.format("%d.%m.%Y")
    );
    // The disposal month is never depreciated, so a date in or before an
    // already depreciated month would leave that month's depreciation in
    // the book value and get the gain or loss wrong.
    if let Some(last) = asset.get::<Option<NaiveDate>, _>("last_period") {
        ensure!(
            month_start(date) > last,
            "Avskrivningen er bokført til og med {}. Avhendingen må dateres etter den måneden.",
            last.format("%m.%Y")
        );
    }
    let posted = asset.get::<i64, _>("kostpris_ore") - asset.get::<i64, _>("akkumulert");
    if let Some(expected) = expected_posted_ore {
        ensure!(
            expected == posted,
            "Bokført verdi har endret seg siden du så over avhendingen. Se over den på nytt."
        );
    }
    let gain = consideration_ore - posted;

    let mut entries = Vec::new();
    if consideration_ore != 0 {
        entries.push(EntryDraft {
            account_number: contra_account.to_string(),
            amount: Ore(consideration_ore),
            vat_code: None,
            description: Some("Vederlag ved avhending".into()),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }
    if posted != 0 {
        entries.push(EntryDraft {
            account_number: asset.get("balansekonto"),
            amount: Ore(-posted),
            vat_code: None,
            description: Some("Utgang bokført verdi".into()),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }
    if gain != 0 {
        entries.push(EntryDraft {
            account_number: if gain > 0 {
                gain_account.to_string()
            } else {
                loss_account.to_string()
            },
            amount: Ore(-gain),
            vat_code: None,
            description: Some(
                if gain > 0 {
                    "Gevinst ved avgang"
                } else {
                    "Tap ved avgang"
                }
                .into(),
            ),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }

    for e in &entries {
        adopt_account(&mut tx, company_id, &e.account_number).await?;
    }
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
    let voucher = if entries.is_empty() {
        None
    } else {
        let name: String = asset.get("navn");
        let draft = VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: date,
            description: format!("Avhending {name}"),
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
    .bind(date)
    .bind(consideration_ore)
    .bind(voucher.as_ref().map(|p| p.id))
    .execute(&mut *tx)
    .await?;
    if preview {
        tx.rollback().await?;
        return Ok(Disposal {
            posted_ore: posted,
            gain_ore: gain,
            lines,
            voucher: None,
        });
    }
    tx.commit().await?;
    Ok(Disposal {
        posted_ore: posted,
        gain_ore: gain,
        lines,
        voucher: voucher.map(|p| (p.fiscal_year, p.voucher_number)),
    })
}

#[derive(Debug)]
pub struct DepreciationGroupRow {
    pub group: String,
    pub description: String,
    pub opening_ore: i64,
    pub additions_ore: i64,
    pub consideration_ore: i64,
    pub basis_ore: i64,
    pub rate_bp: i64,
    pub avskrivning_ore: i64,
    pub closing_ore: i64,
}

#[derive(Debug)]
pub struct BalanceReport {
    pub year: i32,
    pub groups: Vec<DepreciationGroupRow>,
    /// Regnskapsmessig bokført verdi ved årets utgang.
    pub posted_ore: i64,
    /// Sum utgående skattemessig saldo.
    pub skattemessig_ore: i64,
    /// Midlertidig forskjell: bokført − skattemessig.
    pub forskjell_ore: i64,
}

/// Skattemessig saldoavskrivning per gruppe for `year`, computed from
/// scratch over the whole register (additions and consideration per year,
/// rates from the satsregister per year) — nothing stored, identical
/// re-runs forever. Assets acquired before the register's rate
/// coverage make the year fail loudly rather than guess.
pub async fn balance_report(pool: &PgPool, company_id: Uuid, year: i32) -> Result<BalanceReport> {
    let assets = sqlx::query(
        "select anskaffelsesdato, kostpris_ore, saldogruppe, avhendet_dato, vederlag_ore
         from asset where company_id = $1",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let rates = load_rates(pool).await?;

    let first_year = assets
        .iter()
        .map(|r| r.get::<NaiveDate, _>("anskaffelsesdato").year())
        .min()
        .unwrap_or(year);
    let mut groups = Vec::new();
    for (group, description) in DEPRECIATION_GROUPS {
        let mut opening = 0i64;
        let mut rad: Option<DepreciationGroupRow> = None;
        for y in first_year..=year {
            let addition: i64 = assets
                .iter()
                .filter(|r| {
                    r.get::<String, _>("saldogruppe") == *group
                        && r.get::<NaiveDate, _>("anskaffelsesdato").year() == y
                })
                .map(|r| r.get::<i64, _>("kostpris_ore"))
                .sum();
            let consideration: i64 = assets
                .iter()
                .filter(|r| {
                    r.get::<String, _>("saldogruppe") == *group
                        && r.get::<Option<NaiveDate>, _>("avhendet_dato")
                            .is_some_and(|d| d.year() == y)
                })
                .map(|r| r.get::<Option<i64>, _>("vederlag_ore").unwrap_or(0))
                .sum();
            if opening == 0 && addition == 0 && consideration == 0 {
                continue;
            }
            let jan1 = NaiveDate::from_ymd_opt(y, 1, 1).unwrap();
            let rate_bp =
                value_on(&rates, &format!("saldogruppe_{group}"), jan1).with_context(|| {
                    format!(
                        "ingen sats for saldogruppe {group} i {y} — satsregisteret dekker ikke året"
                    )
                })?;
            let year = tax_depreciation_year(opening, addition, consideration, rate_bp);
            rad = Some(DepreciationGroupRow {
                group: (*group).to_string(),
                description: (*description).to_string(),
                opening_ore: opening,
                additions_ore: addition,
                consideration_ore: consideration,
                basis_ore: year.basis_ore,
                rate_bp,
                avskrivning_ore: year.depreciation_ore,
                closing_ore: year.closing_ore,
            });
            opening = year.closing_ore;
        }
        if let Some(rad) = rad {
            groups.push(rad);
        }
    }

    // Regnskapsmessig bokført verdi ved årsslutt: kostpris − posted
    // depreciation dated within the year, for assets owned at year end.
    let year_end = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
    let posted: i64 = sqlx::query_scalar(
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
    let skattemessig: i64 = groups.iter().map(|g| g.closing_ore).sum();
    Ok(BalanceReport {
        year,
        groups,
        posted_ore: posted,
        skattemessig_ore: skattemessig,
        forskjell_ore: posted - skattemessig,
    })
}

#[derive(Debug)]
pub struct DepreciationRun {
    pub period: NaiveDate,
    pub amount_ore: i64,
    pub voucher_number: Option<i64>,
    pub fiscal_year: Option<i32>,
    /// Differs from the month's last day when the month was caught up
    /// after a period lock.
    pub voucher_date: Option<NaiveDate>,
    pub detail: Option<String>,
}

pub async fn list_depreciations(
    pool: &PgPool,
    company_id: Uuid,
    asset_id: Uuid,
) -> Result<Vec<DepreciationRun>> {
    let rows = sqlx::query(
        // One row per month: the posted one, or else the latest failure.
        // Every failed attempt is logged (the log is insert-only), so a
        // month that failed twice would otherwise show twice.
        "select distinct on (d.period)
                d.period, d.amount_ore, v.voucher_number, v.fiscal_year,
                v.voucher_date, d.detail
         from asset_depreciation d
         join asset a on a.id = d.asset_id
         left join voucher v on v.id = d.voucher_id
         where d.asset_id = $1 and a.company_id = $2
         order by d.period desc, (d.voucher_id is null), d.created_at desc
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
            voucher_date: r.get("voucher_date"),
            detail: r.get("detail"),
        })
        .collect())
}
