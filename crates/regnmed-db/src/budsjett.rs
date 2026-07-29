//! Budget and variance report (docs/budsjett.md, #41).
//!
//! The budget is a working document while it is a draft and is frozen
//! when it is fastsatt; a revision is a new version for the same year
//! (migration 0031 enforces both). The variance report always names which
//! version it compares against.
//!
//! Actuals are read from the hovedbok with the same plain SUM query as
//! the rest of the reports — the budget module stores no truth about
//! reality, it fetches it.

use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use regnmed_core::budsjett::{Avviksrapport, KontoTall, avvik, juster_ore};
use regnmed_core::regnskap::{class_of, presentasjon_ore};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct BudgetRow {
    pub id: Uuid,
    pub year: i32,
    pub versjon: i32,
    pub navn: String,
    pub status: String,
    pub note: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub fastsatt_by: Option<String>,
    pub fastsatt_at: Option<DateTime<Utc>>,
    pub sum_ore: i64,
}

#[derive(Debug, Clone)]
pub struct BudgetLine {
    pub account_number: String,
    pub account_name: String,
    pub maned: i32,
    /// Presentasjonsfortegn: inntekt positiv, kostnad positiv.
    pub belop_ore: i64,
}

fn budget_from_row(r: &sqlx::postgres::PgRow) -> BudgetRow {
    BudgetRow {
        id: r.get("id"),
        year: r.get("year"),
        versjon: r.get("versjon"),
        navn: r.get("navn"),
        status: r.get("status"),
        note: r.get("note"),
        created_by: r.get("created_by"),
        created_at: r.get("created_at"),
        fastsatt_by: r.get("fastsatt_by"),
        fastsatt_at: r.get("fastsatt_at"),
        sum_ore: r.get("sum_ore"),
    }
}

const BUDGET_SELECT: &str = "select b.id, b.year, b.versjon, b.navn, b.status, b.note,
                b.created_by, b.created_at, b.fastsatt_by, b.fastsatt_at,
                coalesce((select sum(l.belop_ore) from budget_line l
                          where l.budget_id = b.id), 0)::bigint as sum_ore
         from budget b";

pub async fn list_budgets(
    pool: &PgPool,
    company_id: Uuid,
    year: Option<i32>,
) -> Result<Vec<BudgetRow>> {
    let rows = sqlx::query(&format!(
        "{BUDGET_SELECT}
         where b.company_id = $1 and ($2::int is null or b.year = $2)
         order by b.year desc, b.versjon desc"
    ))
    .bind(company_id)
    .bind(year)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(budget_from_row).collect())
}

pub async fn get_budget(pool: &PgPool, company_id: Uuid, budget_id: Uuid) -> Result<BudgetRow> {
    let row = sqlx::query(&format!(
        "{BUDGET_SELECT} where b.id = $1 and b.company_id = $2"
    ))
    .bind(budget_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such budget")?;
    Ok(budget_from_row(&row))
}

pub async fn budget_lines(
    pool: &PgPool,
    company_id: Uuid,
    budget_id: Uuid,
) -> Result<Vec<BudgetLine>> {
    let rows = sqlx::query(
        "select a.number, a.name, l.maned, l.belop_ore
         from budget_line l
         join budget b on b.id = l.budget_id
         join account a on a.id = l.account_id
         where l.budget_id = $1 and b.company_id = $2
         order by a.number, l.maned",
    )
    .bind(budget_id)
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| BudgetLine {
            account_number: r.get("number"),
            account_name: r.get("name"),
            maned: r.get("maned"),
            belop_ore: r.get("belop_ore"),
        })
        .collect())
}

/// The version to compare against by default: the newest FASTSATT
/// budget for the year, or — when nothing is fastsatt yet — the newest
/// utkast, so an avviksrapport is useful while you are still planning.
pub async fn default_budget(
    pool: &PgPool,
    company_id: Uuid,
    year: i32,
) -> Result<Option<BudgetRow>> {
    let row = sqlx::query(&format!(
        "{BUDGET_SELECT}
         where b.company_id = $1 and b.year = $2
         order by (b.status = 'fastsatt') desc, b.versjon desc
         limit 1"
    ))
    .bind(company_id)
    .bind(year)
    .fetch_optional(pool)
    .await?;
    Ok(row.as_ref().map(budget_from_row))
}

/// Creates a new budget for the year — always the next version, so a
/// revision never overwrites the plan an earlier report cited.
/// `fra_ar` + `justering_bp` seeds the lines from that year's ACTUALS
/// («lag budsjett fra fjoråret ±X %»); the numbers are a starting
/// point, and the human edits from there.
pub async fn create_budget(
    pool: &PgPool,
    company_id: Uuid,
    year: i32,
    navn: &str,
    note: Option<&str>,
    fra_ar: Option<i32>,
    justering_bp: i64,
    created_by: &str,
) -> Result<Uuid> {
    ensure!(!navn.trim().is_empty(), "budsjettet trenger et navn");
    ensure!(
        (1900..=2999).contains(&year),
        "året {year} er utenfor rekkevidde"
    );
    let mut tx = pool.begin().await?;
    let versjon: i32 = sqlx::query_scalar(
        "select coalesce(max(versjon), 0) + 1 from budget where company_id = $1 and year = $2",
    )
    .bind(company_id)
    .bind(year)
    .fetch_one(&mut *tx)
    .await?;
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into budget (id, company_id, year, versjon, navn, note, created_by)
         values ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(company_id)
    .bind(year)
    .bind(versjon)
    .bind(navn.trim())
    .bind(note.map(str::trim).filter(|n| !n.is_empty()))
    .bind(created_by)
    .execute(&mut *tx)
    .await?;

    if let Some(fra) = fra_ar {
        let rows = monthly_actuals(&mut tx, company_id, fra).await?;
        for (account_id, number, maned, ledger_ore) in rows {
            let belop = juster_ore(presentasjon_ore(&number, ledger_ore), justering_bp);
            if belop == 0 {
                continue;
            }
            sqlx::query(
                "insert into budget_line (id, budget_id, account_id, maned, belop_ore)
                 values ($1, $2, $3, $4, $5)",
            )
            .bind(Uuid::now_v7())
            .bind(id)
            .bind(account_id)
            .bind(maned)
            .bind(belop)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(id)
}

/// Per (account, month) ledger sums for a year's resultatkontoer.
async fn monthly_actuals(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    year: i32,
) -> Result<Vec<(Uuid, String, i32, i64)>> {
    let rows = sqlx::query(
        "select a.id, a.number,
                extract(month from v.voucher_date)::int as maned,
                sum(e.amount_ore)::bigint as belop
         from entry e
         join voucher v on v.id = e.voucher_id
         join account a on a.id = e.account_id
         where v.company_id = $1 and extract(year from v.voucher_date) = $2
           and a.number >= '3000'
         group by a.id, a.number, maned
         order by a.number, maned",
    )
    .bind(company_id)
    .bind(year)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get("id"), r.get("number"), r.get("maned"), r.get("belop")))
        .collect())
}

#[derive(Debug)]
pub struct BudgetLineDraft {
    pub account_number: String,
    pub maned: i32,
    pub belop_ore: i64,
}

/// Replaces the budget's lines wholesale (utkast only — the trigger
/// refuses once fastsatt). Zero amounts are simply not stored: an empty
/// cell and a budgeted zero are the same statement.
pub async fn set_budget_lines(
    pool: &PgPool,
    company_id: Uuid,
    budget_id: Uuid,
    lines: &[BudgetLineDraft],
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let status: String = sqlx::query_scalar(
        "select status from budget where id = $1 and company_id = $2 for update",
    )
    .bind(budget_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such budget")?;
    ensure!(
        status == "utkast",
        "budsjettet er fastsatt — lag en ny versjon for å endre"
    );

    sqlx::query("delete from budget_line where budget_id = $1")
        .bind(budget_id)
        .execute(&mut *tx)
        .await?;
    for (i, line) in lines.iter().enumerate() {
        ensure!(
            (1..=12).contains(&line.maned),
            "linje {}: måned må være 1–12",
            i + 1
        );
        ensure!(
            class_of(&line.account_number).is_some_and(|c| (3..=8).contains(&c)),
            "linje {}: {} er ikke en resultatkonto — resultatbudsjett dekker klasse 3–8",
            i + 1,
            line.account_number
        );
        if line.belop_ore == 0 {
            continue;
        }
        let account_id: Uuid = sqlx::query_scalar(
            "select id from account where company_id = $1 and number = $2 and active",
        )
        .bind(company_id)
        .bind(&line.account_number)
        .fetch_optional(&mut *tx)
        .await?
        .with_context(|| format!("linje {}: ingen aktiv konto {}", i + 1, line.account_number))?;
        sqlx::query(
            "insert into budget_line (id, budget_id, account_id, maned, belop_ore)
             values ($1, $2, $3, $4, $5)
             on conflict (budget_id, account_id, maned)
             do update set belop_ore = budget_line.belop_ore + excluded.belop_ore",
        )
        .bind(Uuid::now_v7())
        .bind(budget_id)
        .bind(account_id)
        .bind(line.maned)
        .bind(line.belop_ore)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Freezes the budget: one-way utkast → fastsatt, recorded with who and
/// when. From here the numbers are quotable — an avviksrapport that
/// names this version means the same thing tomorrow.
pub async fn fastsett_budget(
    pool: &PgPool,
    company_id: Uuid,
    budget_id: Uuid,
    fastsatt_by: &str,
) -> Result<()> {
    let updated = sqlx::query(
        "update budget set status = 'fastsatt', fastsatt_by = $3, fastsatt_at = now()
         where id = $1 and company_id = $2 and status = 'utkast'",
    )
    .bind(budget_id)
    .bind(company_id)
    .bind(fastsatt_by)
    .execute(pool)
    .await?
    .rows_affected();
    if updated != 1 {
        bail!("budsjettet finnes ikke eller er allerede fastsatt");
    }
    Ok(())
}

/// Discards a draft. A fastsatt budget is history — the trigger refuses.
pub async fn delete_budget(pool: &PgPool, company_id: Uuid, budget_id: Uuid) -> Result<()> {
    let deleted = sqlx::query("delete from budget where id = $1 and company_id = $2")
        .bind(budget_id)
        .bind(company_id)
        .execute(pool)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .rows_affected();
    ensure!(deleted == 1, "no such budget");
    Ok(())
}

fn konto_entry<'a>(
    tall: &'a mut std::collections::BTreeMap<String, KontoTall>,
    number: &str,
    name: String,
) -> &'a mut KontoTall {
    tall.entry(number.to_string()).or_insert_with(|| KontoTall {
        number: number.to_string(),
        name,
        budsjett: [0; 12],
        faktisk: [0; 12],
    })
}

#[derive(Debug)]
pub struct AvvikMedBudsjett {
    /// Which budget the numbers were compared against — None when the
    /// year has none, and then everything budgeted reads zero.
    pub budsjett: Option<BudgetRow>,
    pub rapport: Avviksrapport,
}

/// Avviksrapport for a year: the budget's months against the ledger's,
/// per konto and per NS 4102-seksjon. `t_o_m_maned` decides how far
/// "hittil" reaches (the caller passes the current month for a running
/// year, 12 for a finished one).
pub async fn avviksrapport(
    pool: &PgPool,
    company_id: Uuid,
    year: i32,
    budget_id: Option<Uuid>,
    t_o_m_maned: u32,
) -> Result<AvvikMedBudsjett> {
    let budsjett = match budget_id {
        Some(id) => Some(get_budget(pool, company_id, id).await?),
        None => default_budget(pool, company_id, year).await?,
    };
    if let Some(b) = &budsjett {
        ensure!(
            b.year == year,
            "budsjettet gjelder {} — ikke {year}",
            b.year
        );
    }

    let mut tall: std::collections::BTreeMap<String, KontoTall> = std::collections::BTreeMap::new();

    // Faktisk: hovedbokens egne summer, per konto per måned.
    let rows = sqlx::query(
        "select a.number, a.name,
                extract(month from v.voucher_date)::int as maned,
                sum(e.amount_ore)::bigint as belop
         from entry e
         join voucher v on v.id = e.voucher_id
         join account a on a.id = e.account_id
         where v.company_id = $1 and extract(year from v.voucher_date) = $2
           and a.number >= '3000'
         group by a.number, a.name, maned",
    )
    .bind(company_id)
    .bind(year)
    .fetch_all(pool)
    .await?;
    for r in &rows {
        let number: String = r.get("number");
        let maned: i32 = r.get("maned");
        let belop: i64 = r.get("belop");
        let konto = konto_entry(&mut tall, &number, r.get("name"));
        konto.faktisk[(maned - 1) as usize] += presentasjon_ore(&number, belop);
    }

    // Budsjett: allerede i presentasjonsfortegn.
    if let Some(b) = &budsjett {
        for line in budget_lines(pool, company_id, b.id).await? {
            let konto = konto_entry(&mut tall, &line.account_number, line.account_name);
            konto.budsjett[(line.maned - 1) as usize] += line.belop_ore;
        }
    }

    let tall: Vec<KontoTall> = tall.into_values().collect();
    Ok(AvvikMedBudsjett {
        budsjett,
        rapport: avvik(&tall, t_o_m_maned),
    })
}
