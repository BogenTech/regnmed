//! Skattekort fetched from Skatteetaten (migration 0078, docs/lonn.md).
//!
//! The order and the answer are insert-only evidence. Applying a card
//! sets the employee's withholding in the SAME transaction as the card is
//! stored, so the employee never carries a withholding no card explains.

use anyhow::{Result, ensure};
use chrono::{DateTime, NaiveDate, Utc};
use regnmed_core::payroll::Withholding;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// One employee's card, already decided by the caller
/// (`regnmed_gov::tax_card::decide`).
pub struct ReceivedCard {
    pub nin: String,
    pub result: String,
    pub card_id: Option<i64>,
    pub issued: Option<NaiveDate>,
    pub raw: serde_json::Value,
    /// `Ok` = set on the employee; `Err` = kept, with the reason.
    pub decision: std::result::Result<Withholding, String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CardView {
    pub employee_id: Uuid,
    pub name: String,
    pub result: String,
    pub card_id: Option<i64>,
    pub issued_date: Option<NaiveDate>,
    pub applied: Option<String>,
    pub applied_percent_bp: Option<i32>,
    pub applied_table: Option<i32>,
    pub applied_twelve_months: bool,
    pub applied_tax_free_card_limit_ore: Option<i64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OrderView {
    pub id: Uuid,
    pub income_year: i32,
    /// None when Skatteetaten never accepted the request.
    pub reference: Option<String>,
    /// Skatteetaten's refusal or rejection, verbatim (newest). Behind the
    /// payroll rights like the rest of this view.
    pub failure: Option<String>,
    pub employee_count: i32,
    pub requested_by: String,
    pub created_at: DateTime<Utc>,
    /// Empty until the answer has been fetched.
    pub cards: Vec<CardView>,
}

/// Employees with a fødselsnummer employed at any time in `year`: the
/// ones a card is ordered for.
pub async fn employees_in_year(
    pool: &PgPool,
    company_id: Uuid,
    year: i32,
) -> Result<Vec<(Uuid, String)>> {
    let rows = sqlx::query(
        "select id, fodselsnummer from employee
          where company_id = $1
            and ansatt_fra <= make_date($2, 12, 31)
            and (ansatt_til is null or ansatt_til >= make_date($2, 1, 1))
          order by navn",
    )
    .bind(company_id)
    .bind(year)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get("id"), r.get("fodselsnummer")))
        .collect())
}

/// Written BEFORE the call to Skatteetaten, so the terms' §7.1 register
/// shows a request even when the call fails or its answer is lost. `id`
/// is also the idempotensid and the id of the order it may become.
pub async fn record_request(
    pool: &PgPool,
    id: Uuid,
    company_id: Uuid,
    income_year: i32,
    employee_count: usize,
    requested_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into tax_card_request
             (id, company_id, income_year, employee_count, requested_by)
         values ($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(company_id)
    .bind(income_year)
    .bind(employee_count as i32)
    .bind(requested_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// Skatteetaten refused the request or rejected its answer.
pub async fn record_failure(
    pool: &PgPool,
    request_id: Uuid,
    detail: &str,
    created_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into tax_card_request_failure (id, request_id, detail, created_by)
         values ($1,$2,$3,$4)",
    )
    .bind(Uuid::new_v4())
    .bind(request_id)
    .bind(detail)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// Warnings for a payroll run: employees whose newest skattekort for the
/// year was NOT applied, so the run used whatever withholding they had.
/// The card's own note says what to do.
pub async fn unapplied_warnings(
    pool: &PgPool,
    company_id: Uuid,
    year: i32,
    employees: &[(Uuid, String)],
) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "select distinct on (employee_id) employee_id, applied, note
           from tax_card where company_id = $1 and income_year = $2
          order by employee_id, created_at desc",
    )
    .bind(company_id)
    .bind(year)
    .fetch_all(pool)
    .await?;
    Ok(employees
        .iter()
        .filter_map(|(id, name)| {
            let r = rows
                .iter()
                .find(|r| r.get::<Uuid, _>("employee_id") == *id)?;
            if r.get::<Option<String>, _>("applied").is_some() {
                return None;
            }
            let note: Option<String> = r.get("note");
            Some(format!(
                "{name}: skattekortet for {year} er ikke satt på den ansatte, så kjøringen \
                 brukte trekket som var registrert. {}",
                note.unwrap_or_default()
            ))
        })
        .collect())
}

#[allow(clippy::too_many_arguments)]
pub async fn record_order(
    pool: &PgPool,
    id: Uuid,
    company_id: Uuid,
    income_year: i32,
    reference: &str,
    dialog_reference: &str,
    employee_count: usize,
    requested_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into tax_card_order
             (id, company_id, income_year, reference, dialog_reference, employee_count,
              requested_by)
         values ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(company_id)
    .bind(income_year)
    .bind(reference)
    .bind(dialog_reference)
    .bind(employee_count as i32)
    .bind(requested_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// The order's reference and income year, if it belongs to the company,
/// its answer has not been stored and it was not rejected.
pub async fn unanswered_order(
    pool: &PgPool,
    company_id: Uuid,
    order_id: Uuid,
) -> Result<Option<(String, i32)>> {
    let row = sqlx::query(
        "select o.reference, o.income_year from tax_card_order o
          where o.id = $1 and o.company_id = $2
            and not exists (select 1 from tax_card c where c.order_id = o.id)
            -- A rejected answer is final: asking again would only call
            -- Skatteetaten again and log the same rejection.
            and not exists (select 1 from tax_card_request_failure f
                             where f.request_id = o.id)",
    )
    .bind(order_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get("reference"), r.get("income_year"))))
}

/// Stores the answer and applies each decided card to its employee, all
/// in one transaction. An order answered twice keeps the first answer.
pub async fn record_answer(
    pool: &PgPool,
    company_id: Uuid,
    order_id: Uuid,
    cards: &[ReceivedCard],
) -> Result<()> {
    let mut tx = pool.begin().await?;
    // Two concurrent fetches of the same answer: the second waits here and
    // then finds the cards already stored.
    let year: i32 = sqlx::query_scalar(
        "select income_year from tax_card_order where id = $1 and company_id = $2 for update",
    )
    .bind(order_id)
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await?;
    let answered: bool =
        sqlx::query_scalar("select exists (select 1 from tax_card where order_id = $1)")
            .bind(order_id)
            .fetch_one(&mut *tx)
            .await?;
    if answered {
        return Ok(());
    }
    for card in cards {
        let Some(employee_id) = sqlx::query_scalar::<_, Uuid>(
            "select id from employee where company_id = $1 and fodselsnummer = $2",
        )
        .bind(company_id)
        .bind(&card.nin)
        .fetch_optional(&mut *tx)
        .await?
        else {
            // Skatteetaten answers only for the numbers we sent.
            continue;
        };
        // (stored kind, percent, table, 12 months, frikort limit, note)
        let (applied, bp, table, twelve, limit, note) = match &card.decision {
            Ok(Withholding::Percent(bp)) => {
                (Some("prosent"), Some(*bp as i32), None, false, None, None)
            }
            Ok(Withholding::PercentTwelveMonths(bp)) => {
                (Some("prosent"), Some(*bp as i32), None, true, None, None)
            }
            Ok(Withholding::Table(n)) => (Some("tabell"), None, Some(*n), false, None, None),
            Ok(Withholding::TaxFreeCard { limit_ore }) => {
                (Some("frikort"), None, None, false, Some(*limit_ore), None)
            }
            Ok(Withholding::None) => (Some("ingen"), None, None, false, None, None),
            Err(reason) => (None, None, None, false, None, Some(reason.as_str())),
        };
        if let Some(kind) = applied {
            let updated = sqlx::query(
                "update employee set trekk_type = $3, trekk_prosent_bp = $4, trekk_tabell = $5,
                        withholding_twelve_months = $6, tax_free_card_limit_ore = $7
                  where id = $1 and company_id = $2",
            )
            .bind(employee_id)
            .bind(company_id)
            .bind(kind)
            .bind(bp)
            .bind(table)
            .bind(twelve)
            .bind(limit)
            .execute(&mut *tx)
            .await?;
            ensure!(updated.rows_affected() == 1, "ukjent ansatt");
        }
        sqlx::query(
            "insert into tax_card
                 (id, order_id, company_id, employee_id, income_year, result, card_id,
                  issued_date, card, applied, applied_percent_bp, applied_table, note,
                  applied_twelve_months, applied_tax_free_card_limit_ore)
             values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
        )
        .bind(Uuid::new_v4())
        .bind(order_id)
        .bind(company_id)
        .bind(employee_id)
        .bind(year)
        .bind(&card.result)
        .bind(card.card_id)
        .bind(card.issued)
        .bind(&card.raw)
        .bind(applied)
        .bind(bp)
        .bind(table)
        .bind(note)
        .bind(twelve)
        .bind(limit)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Every order, newest first, with what came back for each employee.
pub async fn list_orders(pool: &PgPool, company_id: Uuid) -> Result<Vec<OrderView>> {
    let orders = sqlx::query(
        "select r.id, r.income_year, o.reference, r.employee_count, r.requested_by,
                r.created_at,
                (select f.detail from tax_card_request_failure f where f.request_id = r.id
                  order by f.created_at desc limit 1) as failure
           from tax_card_request r left join tax_card_order o on o.id = r.id
          where r.company_id = $1 order by r.created_at desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    let cards = sqlx::query(
        "select c.order_id, c.employee_id, e.navn, c.result, c.card_id, c.issued_date,
                c.applied, c.applied_percent_bp, c.applied_table, c.note,
                c.applied_twelve_months, c.applied_tax_free_card_limit_ore
           from tax_card c join employee e on e.id = c.employee_id
          where c.company_id = $1 order by e.navn",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(orders
        .into_iter()
        .map(|o| {
            let id: Uuid = o.get("id");
            OrderView {
                id,
                income_year: o.get("income_year"),
                reference: o.get("reference"),
                failure: o.get("failure"),
                employee_count: o.get("employee_count"),
                requested_by: o.get("requested_by"),
                created_at: o.get("created_at"),
                cards: cards
                    .iter()
                    .filter(|c| c.get::<Uuid, _>("order_id") == id)
                    .map(|c| CardView {
                        employee_id: c.get("employee_id"),
                        name: c.get("navn"),
                        result: c.get("result"),
                        card_id: c.get("card_id"),
                        issued_date: c.get("issued_date"),
                        applied: c.get("applied"),
                        applied_percent_bp: c.get("applied_percent_bp"),
                        applied_table: c.get("applied_table"),
                        applied_twelve_months: c.get("applied_twelve_months"),
                        applied_tax_free_card_limit_ore: c.get("applied_tax_free_card_limit_ore"),
                        note: c.get("note"),
                    })
                    .collect(),
            }
        })
        .collect())
}
